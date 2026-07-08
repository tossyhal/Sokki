use serde::Serialize;
use std::sync::Arc;
use tauri::Manager;

use crate::audio::decode::decode_audio_file;
use crate::audio::devices::{self, AudioDevices};
use crate::audio::pipeline::SharedCaptureErrorHandler;
use crate::bootstrap::{MODELS_DIR, RECORDINGS_DIR};
use crate::db::{Db, Language, Segment, Session, SessionStatus};
use crate::error::{AppError, DB_ERROR, IO_ERROR};
use crate::export::{self, ExportFormat};
use crate::import::{
    available_space_for_path, enqueue_batch_transcription, BatchJobEnqueuer,
    BatchTranscriptionInput, ImportFileResult, ImportFilesRequest, ImportPipeline,
    ImportSessionIdGenerator,
};
use crate::recording::{
    RecordingManager, RecordingStartDeps, RecordingStateSnapshot, StartRecordingRequest,
    TauriRecordingEventSink,
};
use crate::settings::{GpuMode, Settings, SettingsPatch, SettingsStore};
use crate::sound_check::{
    SoundCheckManager, SoundCheckRequest, SoundCheckResult, TauriSoundCheckEventSink,
};
use crate::transcription::context::{ActiveBackend, WhisperContextManager};
use crate::transcription::jobs::JobTracker;
use crate::transcription::worker::TranscribeWorkerState;

const MAX_RETRANSCRIBE_DURATION_MS: u64 = 3 * 60 * 60 * 1_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub app_version: String,
    pub compiled_gpu_support: bool,
    pub requested_backend: GpuMode,
    pub active_backend: ActiveBackend,
    pub gpu_error_message: Option<String>,
    pub models_dir: String,
    pub data_dir: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetranscribeSessionRequest {
    pub id: String,
    pub language: Language,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSessionRequest {
    pub session_id: String,
    pub format: ExportFormat,
    pub path: String,
}

#[tauri::command]
pub fn get_system_info(
    app: tauri::AppHandle,
    settings_store: tauri::State<'_, SettingsStore>,
    whisper_context: tauri::State<'_, Arc<WhisperContextManager>>,
) -> Result<SystemInfo, AppError> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| AppError::new(IO_ERROR, err.to_string()))?;
    let models_dir = data_dir.join(MODELS_DIR);
    let settings = settings_store.load()?;
    let runtime = whisper_context.runtime_info(settings.gpu_mode);

    Ok(SystemInfo {
        app_version: app.package_info().version.to_string(),
        compiled_gpu_support: runtime.compiled_gpu_support,
        requested_backend: runtime.requested_backend,
        active_backend: runtime.active_backend,
        gpu_error_message: runtime.gpu_error_message,
        models_dir: models_dir.display().to_string(),
        data_dir: data_dir.display().to_string(),
    })
}

#[tauri::command]
pub fn get_settings(settings_store: tauri::State<'_, SettingsStore>) -> Result<Settings, AppError> {
    settings_store.load()
}

#[tauri::command]
pub fn update_settings(
    settings_store: tauri::State<'_, SettingsStore>,
    patch: SettingsPatch,
) -> Result<Settings, AppError> {
    settings_store.update(patch)
}

#[tauri::command]
pub fn list_audio_devices() -> Result<AudioDevices, AppError> {
    devices::list_audio_devices()
}

#[tauri::command]
pub fn start_recording(
    app: tauri::AppHandle,
    db: tauri::State<'_, Db>,
    settings_store: tauri::State<'_, SettingsStore>,
    recording_manager: tauri::State<'_, RecordingManager>,
    tracker: tauri::State<'_, Arc<JobTracker>>,
    worker: tauri::State<'_, Arc<TranscribeWorkerState>>,
    request: StartRecordingRequest,
) -> Result<String, AppError> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| AppError::new(IO_ERROR, err.to_string()))?;
    let settings = settings_store.load()?;
    let deps = RecordingStartDeps {
        tracker: Arc::clone(tracker.inner()),
        enqueuer: Arc::clone(worker.inner())
            as Arc<dyn crate::recording::RealtimeJobEnqueuer + Send + Sync>,
        events: Arc::new(TauriRecordingEventSink::new(app.clone())),
        on_capture_error: capture_error_handler(app.clone()),
        on_max_duration: max_duration_handler(app),
    };
    recording_manager.start(&db, &data_dir, &settings, request, deps)
}

/// Auto-stops the active recording when a capture stream or WAV write fails.
/// Runs the stop on a fresh thread: the handler is invoked from audio-adjacent
/// threads that must not block, and `stop_with_error` joins the mixer thread.
fn capture_error_handler(app: tauri::AppHandle) -> SharedCaptureErrorHandler {
    Arc::new(move |error| {
        let app = app.clone();
        std::thread::spawn(move || {
            let db = app.state::<Db>();
            let recording_manager = app.state::<RecordingManager>();
            let tracker = app.state::<Arc<JobTracker>>();
            let worker = app.state::<Arc<TranscribeWorkerState>>();
            let events = Arc::new(TauriRecordingEventSink::new(app.clone()));
            match recording_manager.stop_with_error(
                &db,
                &tracker,
                worker.inner().as_ref(),
                error,
                events,
            ) {
                Ok(Some(session)) => {
                    log::warn!("recording auto-stopped after capture error: {}", session.id);
                }
                Ok(None) => {}
                Err(stop_error) => {
                    log::error!(
                        "failed to auto-stop recording after capture error: {}",
                        stop_error.message
                    );
                }
            }
        });
    })
}

/// Stops the recording through the normal stop path once MAX_RECORDING_MS is
/// reached, then notifies the frontend of the resulting session status.
fn max_duration_handler(app: tauri::AppHandle) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        let app = app.clone();
        std::thread::spawn(move || {
            let db = app.state::<Db>();
            let recording_manager = app.state::<RecordingManager>();
            let tracker = app.state::<Arc<JobTracker>>();
            let worker = app.state::<Arc<TranscribeWorkerState>>();
            match recording_manager.stop(&db, &tracker, worker.inner().as_ref()) {
                Ok(session) => {
                    log::info!(
                        "recording auto-stopped at max duration: {} ({})",
                        session.id,
                        session.duration_ms
                    );
                    let events = TauriRecordingEventSink::new(app.clone());
                    use crate::recording::RecordingEventSink;
                    events.emit_session_status(
                        crate::transcription::worker::SessionStatusPayload {
                            session_id: session.id,
                            status: session.status,
                            message: session.error_message,
                        },
                    );
                }
                Err(stop_error) => {
                    log::error!(
                        "failed to auto-stop recording at max duration: {}",
                        stop_error.message
                    );
                }
            }
        });
    })
}

#[tauri::command]
pub fn pause_recording(
    recording_manager: tauri::State<'_, RecordingManager>,
) -> Result<(), AppError> {
    recording_manager.pause()
}

#[tauri::command]
pub fn resume_recording(
    recording_manager: tauri::State<'_, RecordingManager>,
) -> Result<(), AppError> {
    recording_manager.resume()
}

#[tauri::command]
pub fn stop_recording(
    db: tauri::State<'_, Db>,
    recording_manager: tauri::State<'_, RecordingManager>,
    tracker: tauri::State<'_, Arc<JobTracker>>,
    worker: tauri::State<'_, Arc<TranscribeWorkerState>>,
) -> Result<Session, AppError> {
    recording_manager.stop(&db, &tracker, worker.inner().as_ref())
}

#[tauri::command]
pub fn get_recording_state(
    recording_manager: tauri::State<'_, RecordingManager>,
) -> Result<RecordingStateSnapshot, AppError> {
    recording_manager.state()
}

#[tauri::command]
pub fn run_sound_check(
    app: tauri::AppHandle,
    recording_manager: tauri::State<'_, RecordingManager>,
    sound_check_manager: tauri::State<'_, SoundCheckManager>,
    request: SoundCheckRequest,
) -> Result<SoundCheckResult, AppError> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| AppError::new(IO_ERROR, err.to_string()))?;
    sound_check_manager.run(
        &recording_manager,
        &data_dir,
        request,
        &TauriSoundCheckEventSink::new(app),
    )
}

#[tauri::command]
pub fn import_files(
    app: tauri::AppHandle,
    db: tauri::State<'_, Db>,
    settings_store: tauri::State<'_, SettingsStore>,
    tracker: tauri::State<'_, Arc<JobTracker>>,
    worker: tauri::State<'_, Arc<TranscribeWorkerState>>,
    id_generator: tauri::State<'_, ImportSessionIdGenerator>,
    request: ImportFilesRequest,
) -> Result<Vec<ImportFileResult>, AppError> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| AppError::new(IO_ERROR, err.to_string()))?;
    let recordings_dir = data_dir.join(RECORDINGS_DIR);
    let settings = settings_store.load()?;
    let available_space_bytes = match available_space_for_path(&recordings_dir) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Ok(request
                .paths
                .into_iter()
                .map(|path| ImportFileResult::failed(path, &error))
                .collect());
        }
    };
    let next_id = || id_generator.next_id();
    let pipeline = ImportPipeline {
        db: &db,
        data_dir: &data_dir,
        tracker: tracker.as_ref(),
        enqueuer: worker.inner().as_ref(),
        vad_threshold_db: settings.vad_threshold_db,
        available_space_bytes,
        session_id_generator: &next_id,
    };

    Ok(pipeline.import_files(&request))
}

#[tauri::command]
pub fn retranscribe_session(
    db: tauri::State<'_, Db>,
    settings_store: tauri::State<'_, SettingsStore>,
    tracker: tauri::State<'_, Arc<JobTracker>>,
    worker: tauri::State<'_, Arc<TranscribeWorkerState>>,
    request: RetranscribeSessionRequest,
) -> Result<(), AppError> {
    let settings = settings_store.load()?;
    retranscribe_session_impl(
        &db,
        tracker.as_ref(),
        worker.inner().as_ref(),
        settings.vad_threshold_db,
        request,
    )
}

#[tauri::command]
pub fn export_session(
    db: tauri::State<'_, Db>,
    request: ExportSessionRequest,
) -> Result<(), AppError> {
    export::export_session(&db, &request.session_id, request.format, request.path)
}

#[tauri::command]
pub fn get_sessions(db: tauri::State<'_, Db>) -> Result<Vec<Session>, AppError> {
    db.list_sessions().map_err(db_error)
}

#[tauri::command]
pub fn get_session(db: tauri::State<'_, Db>, id: String) -> Result<Session, AppError> {
    get_session_impl(&db, &id)
}

#[tauri::command]
pub fn get_segments(
    db: tauri::State<'_, Db>,
    session_id: String,
) -> Result<Vec<Segment>, AppError> {
    get_segments_impl(&db, &session_id)
}

#[tauri::command]
pub fn rename_session(
    db: tauri::State<'_, Db>,
    id: String,
    title: String,
) -> Result<Session, AppError> {
    let title = validate_session_title(&title)?;
    let changed = db.rename_session(&id, title).map_err(db_error)?;
    if !changed {
        return Err(session_not_found(&id));
    }
    get_session_impl(&db, &id)
}

#[tauri::command]
pub fn delete_session(db: tauri::State<'_, Db>, id: String) -> Result<(), AppError> {
    let changed = db.delete_session(&id).map_err(db_error)?;
    if changed {
        Ok(())
    } else {
        Err(session_not_found(&id))
    }
}

#[tauri::command]
pub fn cancel_transcription(
    db: tauri::State<'_, Db>,
    tracker: tauri::State<'_, Arc<JobTracker>>,
    id: String,
) -> Result<Session, AppError> {
    cancel_transcription_impl(&db, tracker.as_ref(), &id)
}

fn get_session_impl(db: &Db, id: &str) -> Result<Session, AppError> {
    db.get_session(id)
        .map_err(db_error)?
        .ok_or_else(|| session_not_found(id))
}

fn get_segments_impl(db: &Db, session_id: &str) -> Result<Vec<Segment>, AppError> {
    let _ = get_session_impl(db, session_id)?;
    db.list_segments(session_id).map_err(db_error)
}

fn retranscribe_session_impl(
    db: &Db,
    tracker: &JobTracker,
    enqueuer: &dyn BatchJobEnqueuer,
    vad_threshold_db: i32,
    request: RetranscribeSessionRequest,
) -> Result<(), AppError> {
    if tracker.pending_count(&request.id) > 0 {
        return Err(AppError::new(
            DB_ERROR,
            format!("transcription is already pending: {}", request.id),
        ));
    }
    let session = get_session_impl(db, &request.id)?;
    let audio_path = session.audio_path.as_deref().ok_or_else(|| {
        AppError::new(
            DB_ERROR,
            format!("session has no audio file: {}", request.id),
        )
    })?;
    let decoded = decode_audio_file(audio_path)?;
    if decoded.duration_ms > MAX_RETRANSCRIBE_DURATION_MS {
        return Err(AppError::new(
            crate::error::AUDIO_TOO_LONG,
            "session audio is longer than 3 hours",
        ));
    }

    db.delete_segments_for_session(&request.id)
        .map_err(db_error)?;
    db.update_session_transcription(
        &request.id,
        request.language,
        &request.model,
        SessionStatus::Transcribing,
        None,
    )
    .map_err(db_error)?;

    let chunk_count = enqueue_batch_transcription(
        db,
        tracker,
        enqueuer,
        BatchTranscriptionInput {
            session_id: &request.id,
            samples: &decoded.samples,
            sample_rate: decoded.sample_rate,
            duration_ms: decoded.duration_ms,
            language: request.language,
            model: &request.model,
            vad_threshold_db,
        },
    )?;

    if chunk_count == 0 {
        db.update_session_status(&request.id, SessionStatus::Done, None)
            .map_err(db_error)?;
    }

    Ok(())
}

fn cancel_transcription_impl(db: &Db, tracker: &JobTracker, id: &str) -> Result<Session, AppError> {
    let session = get_session_impl(db, id)?;
    if !tracker.cancel(id) {
        return Err(AppError::new(
            DB_ERROR,
            format!("transcription is not pending: {id}"),
        ));
    }
    Ok(session)
}

fn session_not_found(id: &str) -> AppError {
    AppError::new(DB_ERROR, format!("session not found: {id}"))
}

fn validate_session_title(title: &str) -> Result<&str, AppError> {
    let title = title.trim();
    if title.is_empty() {
        Err(AppError::new(DB_ERROR, "session title must not be empty"))
    } else {
        Ok(title)
    }
}

fn db_error(error: rusqlite::Error) -> AppError {
    AppError::new(DB_ERROR, format!("database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Language, SessionStatus, Source};
    use crate::transcription::jobs::TranscribeJob;
    use serde_json::json;
    use std::path::Path;
    use std::sync::Mutex;

    #[test]
    fn system_info_serializes_with_camel_case_fields() {
        let info = SystemInfo {
            app_version: "0.0.0".to_string(),
            compiled_gpu_support: false,
            requested_backend: GpuMode::Auto,
            active_backend: ActiveBackend::None,
            gpu_error_message: None,
            models_dir: "C:/Users/example/AppData/Roaming/com.sokki.app/models".to_string(),
            data_dir: "C:/Users/example/AppData/Roaming/com.sokki.app".to_string(),
        };

        let value = serde_json::to_value(info).expect("SystemInfo should serialize");

        assert_eq!(
            value,
            json!({
                "appVersion": "0.0.0",
                "compiledGpuSupport": false,
                "requestedBackend": "auto",
                "activeBackend": "none",
                "gpuErrorMessage": null,
                "modelsDir": "C:/Users/example/AppData/Roaming/com.sokki.app/models",
                "dataDir": "C:/Users/example/AppData/Roaming/com.sokki.app",
            })
        );
    }

    #[test]
    fn get_session_impl_returns_db_error_when_missing() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");

        let error = get_session_impl(&db, "missing").unwrap_err();

        assert_eq!(error.code, DB_ERROR);
        assert_eq!(error.message, "session not found: missing");
    }

    #[test]
    fn get_segments_impl_returns_ordered_session_segments() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        db.insert_session(&sample_session("session-a", SessionStatus::Done))
            .expect("session should insert");
        db.insert_segment(&crate::db::NewSegment {
            session_id: "session-a".to_string(),
            start_ms: 2_000,
            end_ms: 3_000,
            text: "second".to_string(),
            lang: Some("ja".to_string()),
        })
        .expect("segment should insert");
        db.insert_segment(&crate::db::NewSegment {
            session_id: "session-a".to_string(),
            start_ms: 1_000,
            end_ms: 1_500,
            text: "first".to_string(),
            lang: Some("ja".to_string()),
        })
        .expect("segment should insert");

        let segments = get_segments_impl(&db, "session-a").expect("segments should load");

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "first");
        assert_eq!(segments[1].text, "second");
        let value = serde_json::to_value(&segments[0]).expect("Segment should serialize");
        assert_eq!(value["sessionId"], json!("session-a"));
        assert_eq!(value["startMs"], json!(1_000));
    }

    #[test]
    fn get_segments_impl_rejects_missing_session() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");

        let error = get_segments_impl(&db, "missing").unwrap_err();

        assert_eq!(error.code, DB_ERROR);
        assert_eq!(error.message, "session not found: missing");
    }

    #[test]
    fn rename_session_rejects_empty_title_contract() {
        let error = validate_session_title("  ").unwrap_err();

        assert_eq!(error.code, DB_ERROR);
        assert_eq!(error.message, "session title must not be empty");
    }

    #[test]
    fn cancel_transcription_sets_pending_cancel_flag() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        let tracker = JobTracker::new();
        let flag = tracker.enqueue("session-a");
        db.insert_session(&sample_session("session-a", SessionStatus::Transcribing))
            .expect("session should insert");

        let session = cancel_transcription_impl(&db, &tracker, "session-a")
            .expect("pending session should cancel");

        assert_eq!(session.id, "session-a");
        assert!(flag.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(tracker.pending_count("session-a"), 1);
    }

    #[test]
    fn cancel_transcription_rejects_missing_session() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        let tracker = JobTracker::new();

        let error = cancel_transcription_impl(&db, &tracker, "missing").unwrap_err();

        assert_eq!(error.code, DB_ERROR);
        assert_eq!(error.message, "session not found: missing");
    }

    #[test]
    fn cancel_transcription_rejects_session_without_pending_job() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        let tracker = JobTracker::new();
        db.insert_session(&sample_session("session-a", SessionStatus::Done))
            .expect("session should insert");

        let error = cancel_transcription_impl(&db, &tracker, "session-a").unwrap_err();

        assert_eq!(error.code, DB_ERROR);
        assert_eq!(error.message, "transcription is not pending: session-a");
    }

    #[test]
    fn retranscribe_session_deletes_segments_updates_session_and_enqueues_jobs() {
        let data_dir = temp_dir("retranscribe-success");
        let wav_path = data_dir.join("source.wav");
        write_mono_wav(&wav_path, 16_000, 31_000);
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        let tracker = JobTracker::new();
        let enqueuer = RecordingEnqueuer::default();
        let mut session = sample_session("session-a", SessionStatus::Done);
        session.audio_path = Some(wav_path.display().to_string());
        db.insert_session(&session).expect("session should insert");
        db.insert_segment(&crate::db::NewSegment {
            session_id: "session-a".to_string(),
            start_ms: 0,
            end_ms: 500,
            text: "old".to_string(),
            lang: Some("ja".to_string()),
        })
        .expect("segment should insert");

        retranscribe_session_impl(
            &db,
            &tracker,
            &enqueuer,
            -40,
            RetranscribeSessionRequest {
                id: "session-a".to_string(),
                language: Language::En,
                model: "small".to_string(),
            },
        )
        .expect("retranscription should enqueue");

        assert!(db
            .list_segments("session-a")
            .expect("segments should list")
            .is_empty());
        let updated = db
            .get_session("session-a")
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(updated.status, SessionStatus::Transcribing);
        assert_eq!(updated.language, Language::En);
        assert_eq!(updated.model, "small");
        assert_eq!(tracker.pending_count("session-a"), 3);
        assert_eq!(enqueuer.jobs.lock().unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn retranscribe_session_rejects_missing_audio_path() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        let tracker = JobTracker::new();
        let enqueuer = RecordingEnqueuer::default();
        db.insert_session(&sample_session("session-a", SessionStatus::Done))
            .expect("session should insert");

        let error = retranscribe_session_impl(
            &db,
            &tracker,
            &enqueuer,
            -40,
            RetranscribeSessionRequest {
                id: "session-a".to_string(),
                language: Language::Ja,
                model: "medium-q5_0".to_string(),
            },
        )
        .unwrap_err();

        assert_eq!(error.code, DB_ERROR);
        assert_eq!(error.message, "session has no audio file: session-a");
    }

    #[test]
    fn retranscribe_session_rejects_pending_session() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        let tracker = JobTracker::new();
        let enqueuer = RecordingEnqueuer::default();
        tracker.enqueue("session-a");

        let error = retranscribe_session_impl(
            &db,
            &tracker,
            &enqueuer,
            -40,
            RetranscribeSessionRequest {
                id: "session-a".to_string(),
                language: Language::Ja,
                model: "medium-q5_0".to_string(),
            },
        )
        .unwrap_err();

        assert_eq!(error.code, DB_ERROR);
        assert_eq!(error.message, "transcription is already pending: session-a");
    }

    #[test]
    fn session_serializes_for_library_cards() {
        let session = Session {
            id: "session-a".to_string(),
            title: "Session A".to_string(),
            created_at: 1_000,
            duration_ms: 12_000,
            audio_path: Some(
                "C:/Users/example/AppData/Roaming/com.sokki.app/recordings/a.wav".to_string(),
            ),
            source: Source::Mic,
            language: Language::Ja,
            model: "medium-q5_0".to_string(),
            status: SessionStatus::Interrupted,
            error_message: Some("recording interrupted by app shutdown".to_string()),
            drop_count: 3,
        };

        let value = serde_json::to_value(session).expect("Session should serialize");

        assert_eq!(value["createdAt"], json!(1_000));
        assert_eq!(value["durationMs"], json!(12_000));
        assert_eq!(value["status"], json!("interrupted"));
        assert_eq!(value["dropCount"], json!(3));
    }

    fn sample_session(id: &str, status: SessionStatus) -> Session {
        Session {
            id: id.to_string(),
            title: id.to_string(),
            created_at: 1_000,
            duration_ms: 12_000,
            audio_path: None,
            source: Source::Import,
            language: Language::Ja,
            model: "medium-q5_0".to_string(),
            status,
            error_message: None,
            drop_count: 0,
        }
    }

    #[derive(Default)]
    struct RecordingEnqueuer {
        jobs: Mutex<Vec<TranscribeJob>>,
    }

    impl BatchJobEnqueuer for RecordingEnqueuer {
        fn enqueue_batch_job(&self, job: TranscribeJob) -> Result<(), AppError> {
            self.jobs.lock().unwrap().push(job);
            Ok(())
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("sokki-commands-{name}-{}", unix_time_ms()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_mono_wav(path: &Path, sample_rate: u32, duration_ms: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        let sample_count = sample_rate as u64 * duration_ms as u64 / 1_000;
        for _ in 0..sample_count {
            writer.write_sample::<i16>(i16::MAX / 4).unwrap();
        }
        writer.finalize().unwrap();
    }

    fn unix_time_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}
