use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::audio::devices::{self, AudioDevice, AudioDevices};
use crate::audio::wav::StreamingWavWriter;
use crate::bootstrap::RECORDINGS_DIR;
use crate::db::{Db, Language, Session, SessionStatus, Source};
use crate::error::{
    AppError, ALREADY_RECORDING, DB_ERROR, DEVICE_NOT_FOUND, IO_ERROR, NOT_RECORDING,
};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartRecordingRequest {
    pub source: Source,
    pub language: Language,
    pub model: String,
    pub mic_device: Option<String>,
    pub loopback_device: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStateSnapshot {
    pub active: bool,
    pub session_id: Option<String>,
    pub paused: bool,
    pub elapsed_ms: u64,
}

pub struct RecordingManager {
    active: AtomicBool,
    id_counter: AtomicU64,
    state: Mutex<Option<ActiveRecording>>,
}

struct ActiveRecording {
    session_id: String,
    started_at: Instant,
    paused_at: Option<Instant>,
    paused_total: Duration,
    writer: Option<StreamingWavWriter>,
    drop_count: AtomicU64,
}

impl RecordingManager {
    pub fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            id_counter: AtomicU64::new(0),
            state: Mutex::new(None),
        }
    }

    pub fn start(
        &self,
        db: &Db,
        data_dir: &Path,
        request: StartRecordingRequest,
    ) -> Result<String, AppError> {
        let audio_devices = devices::list_audio_devices()?;
        self.start_with_devices(db, data_dir, request, &audio_devices)
    }

    fn start_with_devices(
        &self,
        db: &Db,
        data_dir: &Path,
        request: StartRecordingRequest,
        audio_devices: &AudioDevices,
    ) -> Result<String, AppError> {
        if request.source == Source::Import {
            return Err(AppError::new(
                IO_ERROR,
                "import source cannot be used for recording",
            ));
        }
        validate_requested_devices(&request, audio_devices)?;

        let mut state = self
            .state
            .lock()
            .expect("recording state mutex should not be poisoned");
        if state.is_some() {
            return Err(AppError::new(ALREADY_RECORDING, "recording already active"));
        }

        let session_id = self.next_session_id();
        let wav_path = recording_wav_path(data_dir, &session_id);
        let writer = StreamingWavWriter::create(&wav_path)?;
        let now_ms = unix_time_ms() as i64;
        let session = Session {
            id: session_id.clone(),
            title: format!("Recording {session_id}"),
            created_at: now_ms,
            duration_ms: 0,
            audio_path: Some(wav_path.display().to_string()),
            source: request.source,
            language: request.language,
            model: request.model,
            status: SessionStatus::Recording,
            error_message: None,
            drop_count: 0,
        };
        db.insert_session(&session).map_err(db_error)?;

        *state = Some(ActiveRecording {
            session_id: session_id.clone(),
            started_at: Instant::now(),
            paused_at: None,
            paused_total: Duration::ZERO,
            writer: Some(writer),
            drop_count: AtomicU64::new(0),
        });
        self.active.store(true, Ordering::SeqCst);

        Ok(session_id)
    }

    pub fn pause(&self) -> Result<(), AppError> {
        let mut state = self
            .state
            .lock()
            .expect("recording state mutex should not be poisoned");
        let active = state
            .as_mut()
            .ok_or_else(|| AppError::new(NOT_RECORDING, "recording is not active"))?;
        if active.paused_at.is_none() {
            active.paused_at = Some(Instant::now());
        }
        Ok(())
    }

    pub fn resume(&self) -> Result<(), AppError> {
        let mut state = self
            .state
            .lock()
            .expect("recording state mutex should not be poisoned");
        let active = state
            .as_mut()
            .ok_or_else(|| AppError::new(NOT_RECORDING, "recording is not active"))?;
        if let Some(paused_at) = active.paused_at.take() {
            active.paused_total += paused_at.elapsed();
        }
        Ok(())
    }

    pub fn stop(&self, db: &Db) -> Result<Session, AppError> {
        let active = self
            .state
            .lock()
            .expect("recording state mutex should not be poisoned")
            .take()
            .ok_or_else(|| AppError::new(NOT_RECORDING, "recording is not active"))?;
        self.active.store(false, Ordering::SeqCst);

        let duration_ms = active
            .writer
            .ok_or_else(|| AppError::new(IO_ERROR, "wav writer is already closed"))?
            .finalize()? as i64;
        let drop_count = active.drop_count.load(Ordering::Relaxed) as i64;
        db.update_session_duration(&active.session_id, duration_ms, drop_count)
            .map_err(db_error)?;
        db.update_session_status(&active.session_id, SessionStatus::Done, None)
            .map_err(db_error)?;
        db.get_session(&active.session_id)
            .map_err(db_error)?
            .ok_or_else(|| AppError::new(DB_ERROR, "recording session not found after stop"))
    }

    pub fn state(&self) -> Result<RecordingStateSnapshot, AppError> {
        let state = self
            .state
            .lock()
            .expect("recording state mutex should not be poisoned");
        let Some(active) = state.as_ref() else {
            return Ok(RecordingStateSnapshot {
                active: false,
                session_id: None,
                paused: false,
                elapsed_ms: 0,
            });
        };

        Ok(RecordingStateSnapshot {
            active: true,
            session_id: Some(active.session_id.clone()),
            paused: active.paused_at.is_some(),
            elapsed_ms: active.elapsed().as_millis() as u64,
        })
    }

    pub fn recording_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    fn next_session_id(&self) -> String {
        let counter = self.id_counter.fetch_add(1, Ordering::Relaxed);
        format!("rec-{}-{counter}", unix_time_ms())
    }
}

impl Default for RecordingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ActiveRecording {
    fn elapsed(&self) -> Duration {
        let paused = self
            .paused_at
            .map(|paused_at| paused_at.elapsed())
            .unwrap_or(Duration::ZERO);
        self.started_at
            .elapsed()
            .saturating_sub(self.paused_total)
            .saturating_sub(paused)
    }
}

fn recording_wav_path(data_dir: &Path, session_id: &str) -> PathBuf {
    data_dir
        .join(RECORDINGS_DIR)
        .join(format!("{session_id}.wav"))
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_millis()
}

fn db_error(error: rusqlite::Error) -> AppError {
    AppError::new(DB_ERROR, format!("database error: {error}"))
}

fn validate_requested_devices(
    request: &StartRecordingRequest,
    audio_devices: &AudioDevices,
) -> Result<(), AppError> {
    match request.source {
        Source::Mic => validate_input_device(request.mic_device.as_deref(), audio_devices),
        Source::System => validate_output_device(request.loopback_device.as_deref(), audio_devices),
        Source::Mix => {
            validate_input_device(request.mic_device.as_deref(), audio_devices)?;
            validate_output_device(request.loopback_device.as_deref(), audio_devices)
        }
        Source::Import => Ok(()),
    }
}

fn validate_input_device(
    requested: Option<&str>,
    audio_devices: &AudioDevices,
) -> Result<(), AppError> {
    validate_device("input", requested, &audio_devices.inputs)
}

fn validate_output_device(
    requested: Option<&str>,
    audio_devices: &AudioDevices,
) -> Result<(), AppError> {
    validate_device("output", requested, &audio_devices.outputs)
}

fn validate_device(
    kind: &str,
    requested: Option<&str>,
    devices: &[AudioDevice],
) -> Result<(), AppError> {
    if devices.is_empty() {
        return Err(AppError::new(
            DEVICE_NOT_FOUND,
            format!("no {kind} audio device is available"),
        ));
    }

    if let Some(requested) = requested {
        let found = devices.iter().any(|device| device.id == requested);
        if !found {
            return Err(AppError::new(
                DEVICE_NOT_FOUND,
                format!("{kind} audio device not found: {requested}"),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, Language, SessionStatus, Source};

    #[test]
    fn start_recording_inserts_recording_session_and_sets_active() {
        let db = Db::open_in_memory().unwrap();
        let data_dir = temp_data_dir("start_recording_inserts_recording_session_and_sets_active");
        let manager = RecordingManager::new();

        let session_id = manager
            .start_with_devices(
                &db,
                &data_dir,
                StartRecordingRequest {
                    source: Source::Mic,
                    language: Language::Ja,
                    model: "medium-q5_0".to_string(),
                    mic_device: None,
                    loopback_device: None,
                },
                &sample_devices(),
            )
            .unwrap();

        let session = db.get_session(&session_id).unwrap().unwrap();
        assert_eq!(session.status, SessionStatus::Recording);
        assert_eq!(session.source, Source::Mic);
        assert!(session.audio_path.unwrap().ends_with(".wav"));
        assert!(manager.recording_active());
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn rejects_start_while_recording() {
        let db = Db::open_in_memory().unwrap();
        let data_dir = temp_data_dir("rejects_start_while_recording");
        let manager = RecordingManager::new();
        let request = StartRecordingRequest {
            source: Source::Mic,
            language: Language::Ja,
            model: "medium-q5_0".to_string(),
            mic_device: None,
            loopback_device: None,
        };

        manager
            .start_with_devices(&db, &data_dir, request.clone(), &sample_devices())
            .unwrap();
        let error = manager
            .start_with_devices(&db, &data_dir, request, &sample_devices())
            .unwrap_err();

        assert_eq!(error.code, crate::error::ALREADY_RECORDING);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn pause_resume_updates_recording_state() {
        let db = Db::open_in_memory().unwrap();
        let data_dir = temp_data_dir("pause_resume_updates_recording_state");
        let manager = RecordingManager::new();
        let session_id = manager
            .start_with_devices(
                &db,
                &data_dir,
                sample_request(Source::Mic),
                &sample_devices(),
            )
            .unwrap();

        manager.pause().unwrap();
        let paused = manager.state().unwrap();
        assert_eq!(paused.session_id.as_deref(), Some(session_id.as_str()));
        assert!(paused.paused);

        manager.resume().unwrap();
        assert!(!manager.state().unwrap().paused);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn stop_finalizes_wav_and_marks_session_done() {
        let db = Db::open_in_memory().unwrap();
        let data_dir = temp_data_dir("stop_finalizes_wav_and_marks_session_done");
        let manager = RecordingManager::new();
        let session_id = manager
            .start_with_devices(
                &db,
                &data_dir,
                sample_request(Source::Mic),
                &sample_devices(),
            )
            .unwrap();

        let session = manager.stop(&db).unwrap();

        assert_eq!(session.id, session_id);
        assert_eq!(session.status, SessionStatus::Done);
        assert!(!manager.recording_active());
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn validates_source_specific_devices_before_starting() {
        let db = Db::open_in_memory().unwrap();
        let data_dir = temp_data_dir("validates_source_specific_devices_before_starting");
        let manager = RecordingManager::new();

        manager
            .start_with_devices(
                &db,
                &data_dir,
                sample_request(Source::Mix),
                &sample_devices(),
            )
            .unwrap();

        assert!(manager.recording_active());
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn rejects_missing_requested_device() {
        let db = Db::open_in_memory().unwrap();
        let data_dir = temp_data_dir("rejects_missing_requested_device");
        let manager = RecordingManager::new();
        let request = StartRecordingRequest {
            mic_device: Some("missing mic".to_string()),
            ..sample_request(Source::Mic)
        };

        let error = manager
            .start_with_devices(&db, &data_dir, request, &sample_devices())
            .unwrap_err();

        assert_eq!(error.code, crate::error::DEVICE_NOT_FOUND);
        assert!(!manager.recording_active());
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn rejects_source_when_required_device_kind_is_unavailable() {
        let db = Db::open_in_memory().unwrap();
        let data_dir = temp_data_dir("rejects_source_when_required_device_kind_is_unavailable");
        let manager = RecordingManager::new();
        let devices = AudioDevices {
            inputs: sample_devices().inputs,
            outputs: vec![],
        };

        let error = manager
            .start_with_devices(&db, &data_dir, sample_request(Source::System), &devices)
            .unwrap_err();

        assert_eq!(error.code, crate::error::DEVICE_NOT_FOUND);
        assert!(!manager.recording_active());
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    fn sample_request(source: Source) -> StartRecordingRequest {
        StartRecordingRequest {
            source,
            language: Language::Ja,
            model: "medium-q5_0".to_string(),
            mic_device: None,
            loopback_device: None,
        }
    }

    fn sample_devices() -> AudioDevices {
        AudioDevices {
            inputs: vec![AudioDevice {
                id: "mic-1".to_string(),
                name: "Microphone".to_string(),
                is_default: true,
            }],
            outputs: vec![AudioDevice {
                id: "speaker-1".to_string(),
                name: "Speakers".to_string(),
                is_default: true,
            }],
        }
    }

    fn temp_data_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("sokki-recording-{name}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}
