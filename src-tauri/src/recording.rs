use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::audio::devices::{self, AudioDevice, AudioDevices};
use crate::audio::mixer::SourceLevels;
use crate::audio::wav::{StreamingWavWriter, WAV_SAMPLE_RATE};
use crate::bootstrap::RECORDINGS_DIR;
use crate::db::{Db, Language, Session, SessionStatus, Source};
use crate::error::{
    AppError, ALREADY_RECORDING, DB_ERROR, DEVICE_NOT_FOUND, IO_ERROR, NOT_RECORDING,
};
use crate::transcription::jobs::{JobKind, JobTracker, TranscribeJob};
use crate::transcription::segmenter::{RealtimeChunk, RealtimeSegmenter};
use crate::transcription::worker::{TranscribeWorkerHandle, TranscribeWorkerState};

pub const RECORDING_LEVEL_EVENT: &str = "recording://level";
pub const RECORDING_ELAPSED_EVENT: &str = "recording://elapsed";
pub const RECORDING_DROPS_EVENT: &str = "recording://drops";
pub const RECORDING_LIMIT_EVENT: &str = "recording://limit";
pub const MAX_RECORDING_MS: u64 = 3 * 60 * 60 * 1_000;
pub const RECORDING_LIMIT_WARNING_BEFORE_MS: u64 = 10 * 60 * 1_000;
const LEVEL_EVENT_INTERVAL: Duration = Duration::from_millis(100);
const ELAPSED_EVENT_INTERVAL_TICKS: u64 = 5;
const DEFAULT_VAD_THRESHOLD_DB: i32 = -40;

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

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingLevelPayload {
    pub mic: f32,
    pub system: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingElapsedPayload {
    pub elapsed_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDropsPayload {
    pub drop_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingLimitPayload {
    pub kind: RecordingLimitKind,
    pub elapsed_ms: u64,
    pub max_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingLimitKind {
    Warning,
    MaxReached,
}

pub trait RecordingEventSink: Send + Sync {
    fn emit_level(&self, payload: RecordingLevelPayload);
    fn emit_elapsed(&self, payload: RecordingElapsedPayload);
    fn emit_drops(&self, payload: RecordingDropsPayload);
    fn emit_limit(&self, payload: RecordingLimitPayload);
}

pub trait RealtimeJobEnqueuer {
    fn enqueue_rt_job(&self, job: TranscribeJob) -> Result<(), AppError>;
}

pub struct TauriRecordingEventSink {
    app: tauri::AppHandle,
}

pub struct RecordingManager {
    active: Arc<AtomicBool>,
    id_counter: AtomicU64,
    state: Mutex<Option<ActiveRecording>>,
}

struct ActiveRecording {
    session_id: String,
    timing: Arc<Mutex<RecordingTiming>>,
    writer: Option<StreamingWavWriter>,
    segmenter: RealtimeSegmenter,
    language: Language,
    model: String,
    levels: Arc<Mutex<RecordingLevelPayload>>,
    drop_count: Arc<AtomicU64>,
    event_stop_tx: mpsc::Sender<()>,
    event_thread: Option<JoinHandle<()>>,
}

#[derive(Debug)]
struct RecordingTiming {
    started_at: Instant,
    paused_at: Option<Instant>,
    paused_total: Duration,
}

impl RecordingManager {
    pub fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            id_counter: AtomicU64::new(0),
            state: Mutex::new(None),
        }
    }

    pub fn start(
        &self,
        db: &Db,
        data_dir: &Path,
        request: StartRecordingRequest,
        events: Arc<dyn RecordingEventSink>,
    ) -> Result<String, AppError> {
        let audio_devices = devices::list_audio_devices()?;
        self.start_with_devices(db, data_dir, request, &audio_devices, events)
    }

    fn start_with_devices(
        &self,
        db: &Db,
        data_dir: &Path,
        request: StartRecordingRequest,
        audio_devices: &AudioDevices,
        events: Arc<dyn RecordingEventSink>,
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
            model: request.model.clone(),
            status: SessionStatus::Recording,
            error_message: None,
            drop_count: 0,
        };
        db.insert_session(&session).map_err(db_error)?;

        let timing = Arc::new(Mutex::new(RecordingTiming::new()));
        let levels = Arc::new(Mutex::new(RecordingLevelPayload {
            mic: 0.0,
            system: 0.0,
        }));
        let drop_count = Arc::new(AtomicU64::new(0));
        let (event_stop_tx, event_stop_rx) = mpsc::channel();
        let event_thread = spawn_event_thread(
            Arc::clone(&timing),
            Arc::clone(&levels),
            Arc::clone(&drop_count),
            events,
            event_stop_rx,
        );

        *state = Some(ActiveRecording {
            session_id: session_id.clone(),
            timing,
            writer: Some(writer),
            segmenter: RealtimeSegmenter::new(WAV_SAMPLE_RATE, DEFAULT_VAD_THRESHOLD_DB),
            language: request.language,
            model: request.model,
            levels,
            drop_count,
            event_stop_tx,
            event_thread: Some(event_thread),
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
        let mut timing = active
            .timing
            .lock()
            .expect("recording timing mutex should not be poisoned");
        if timing.paused_at.is_none() {
            timing.paused_at = Some(Instant::now());
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
        let mut timing = active
            .timing
            .lock()
            .expect("recording timing mutex should not be poisoned");
        if let Some(paused_at) = timing.paused_at.take() {
            timing.paused_total += paused_at.elapsed();
        }
        Ok(())
    }

    pub fn stop(
        &self,
        db: &Db,
        tracker: &JobTracker,
        enqueuer: &dyn RealtimeJobEnqueuer,
    ) -> Result<Session, AppError> {
        let mut active = self
            .state
            .lock()
            .expect("recording state mutex should not be poisoned")
            .take()
            .ok_or_else(|| AppError::new(NOT_RECORDING, "recording is not active"))?;

        let result = (|| {
            let _ = active.event_stop_tx.send(());
            if let Some(event_thread) = active.event_thread.take() {
                let _ = event_thread.join();
            }

            if let Some(chunk) = active.segmenter.flush() {
                enqueue_realtime_chunk(&active, chunk, tracker, enqueuer)?;
            }

            let duration_ms = active
                .writer
                .ok_or_else(|| AppError::new(IO_ERROR, "wav writer is already closed"))?
                .finalize()? as i64;
            let drop_count = active.drop_count.load(Ordering::Relaxed) as i64;
            db.update_session_duration(&active.session_id, duration_ms, drop_count)
                .map_err(db_error)?;

            self.active.store(false, Ordering::SeqCst);
            tracker.recording_stopped(db, &active.session_id)?;
            db.get_session(&active.session_id)
                .map_err(db_error)?
                .ok_or_else(|| AppError::new(DB_ERROR, "recording session not found after stop"))
        })();

        if result.is_err() {
            self.active.store(false, Ordering::SeqCst);
        }

        result
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
        let timing = active
            .timing
            .lock()
            .expect("recording timing mutex should not be poisoned");

        Ok(RecordingStateSnapshot {
            active: true,
            session_id: Some(active.session_id.clone()),
            paused: timing.paused_at.is_some(),
            elapsed_ms: timing.elapsed().as_millis() as u64,
        })
    }

    pub fn recording_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    pub fn recording_active_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.active)
    }

    pub fn record_mixed_samples(
        &self,
        samples: &[f32],
        tracker: &JobTracker,
        enqueuer: &dyn RealtimeJobEnqueuer,
    ) -> Result<(), AppError> {
        let mut state = self
            .state
            .lock()
            .expect("recording state mutex should not be poisoned");
        let active = state
            .as_mut()
            .ok_or_else(|| AppError::new(NOT_RECORDING, "recording is not active"))?;
        active
            .writer
            .as_mut()
            .ok_or_else(|| AppError::new(IO_ERROR, "wav writer is already closed"))?
            .write_frame(samples)?;

        for chunk in active.segmenter.push(samples) {
            enqueue_realtime_chunk(active, chunk, tracker, enqueuer)?;
        }
        Ok(())
    }

    pub fn update_levels(&self, levels: SourceLevels) -> Result<(), AppError> {
        let state = self
            .state
            .lock()
            .expect("recording state mutex should not be poisoned");
        let active = state
            .as_ref()
            .ok_or_else(|| AppError::new(NOT_RECORDING, "recording is not active"))?;
        let mut current = active
            .levels
            .lock()
            .expect("recording levels mutex should not be poisoned");
        *current = RecordingLevelPayload {
            mic: levels.mic,
            system: levels.system,
        };
        Ok(())
    }

    fn next_session_id(&self) -> String {
        let counter = self.id_counter.fetch_add(1, Ordering::Relaxed);
        format!("rec-{}-{counter}", unix_time_ms())
    }
}

impl RealtimeJobEnqueuer for TranscribeWorkerHandle {
    fn enqueue_rt_job(&self, job: TranscribeJob) -> Result<(), AppError> {
        self.enqueue_rt(job)
    }
}

impl RealtimeJobEnqueuer for TranscribeWorkerState {
    fn enqueue_rt_job(&self, job: TranscribeJob) -> Result<(), AppError> {
        self.enqueue_rt(job)
    }
}

impl Default for RecordingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TauriRecordingEventSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl RecordingEventSink for TauriRecordingEventSink {
    fn emit_level(&self, payload: RecordingLevelPayload) {
        let _ = self.app.emit(RECORDING_LEVEL_EVENT, payload);
    }

    fn emit_elapsed(&self, payload: RecordingElapsedPayload) {
        let _ = self.app.emit(RECORDING_ELAPSED_EVENT, payload);
    }

    fn emit_drops(&self, payload: RecordingDropsPayload) {
        let _ = self.app.emit(RECORDING_DROPS_EVENT, payload);
    }

    fn emit_limit(&self, payload: RecordingLimitPayload) {
        let _ = self.app.emit(RECORDING_LIMIT_EVENT, payload);
    }
}

impl RecordingTiming {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            paused_at: None,
            paused_total: Duration::ZERO,
        }
    }

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

fn spawn_event_thread(
    timing: Arc<Mutex<RecordingTiming>>,
    levels: Arc<Mutex<RecordingLevelPayload>>,
    drop_count: Arc<AtomicU64>,
    events: Arc<dyn RecordingEventSink>,
    stop_rx: mpsc::Receiver<()>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut tick_count = 0;
        let mut last_drop_count = 0;
        let mut limit_state = RecordingLimitState::default();
        loop {
            match stop_rx.recv_timeout(LEVEL_EVENT_INTERVAL) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }

            tick_count += 1;
            let timing = timing
                .lock()
                .expect("recording timing mutex should not be poisoned");
            let paused = timing.paused_at.is_some();
            let elapsed_ms = timing.elapsed().as_millis() as u64;
            drop(timing);

            if !paused {
                let level = *levels
                    .lock()
                    .expect("recording levels mutex should not be poisoned");
                events.emit_level(level);
            }

            if tick_count % ELAPSED_EVENT_INTERVAL_TICKS == 0 {
                events.emit_elapsed(RecordingElapsedPayload { elapsed_ms });
            }

            if let Some(payload) = limit_state.next_payload(elapsed_ms) {
                events.emit_limit(payload);
            }

            let current_drop_count = drop_count.load(Ordering::Relaxed);
            if current_drop_count > last_drop_count {
                last_drop_count = current_drop_count;
                events.emit_drops(RecordingDropsPayload {
                    drop_count: current_drop_count,
                });
            }
        }
    })
}

#[derive(Default)]
struct RecordingLimitState {
    warning_sent: bool,
    max_sent: bool,
}

impl RecordingLimitState {
    fn next_payload(&mut self, elapsed_ms: u64) -> Option<RecordingLimitPayload> {
        if !self.max_sent && elapsed_ms >= MAX_RECORDING_MS {
            self.max_sent = true;
            self.warning_sent = true;
            return Some(RecordingLimitPayload {
                kind: RecordingLimitKind::MaxReached,
                elapsed_ms,
                max_ms: MAX_RECORDING_MS,
            });
        }

        if !self.warning_sent
            && elapsed_ms >= MAX_RECORDING_MS.saturating_sub(RECORDING_LIMIT_WARNING_BEFORE_MS)
        {
            self.warning_sent = true;
            return Some(RecordingLimitPayload {
                kind: RecordingLimitKind::Warning,
                elapsed_ms,
                max_ms: MAX_RECORDING_MS,
            });
        }

        None
    }
}

fn recording_wav_path(data_dir: &Path, session_id: &str) -> PathBuf {
    data_dir
        .join(RECORDINGS_DIR)
        .join(format!("{session_id}.wav"))
}

fn enqueue_realtime_chunk(
    active: &ActiveRecording,
    chunk: RealtimeChunk,
    tracker: &JobTracker,
    enqueuer: &dyn RealtimeJobEnqueuer,
) -> Result<(), AppError> {
    let canceled = tracker.enqueue(&active.session_id);
    let job = TranscribeJob {
        session_id: active.session_id.clone(),
        kind: JobKind::Rt,
        audio: chunk.audio,
        chunk_start_ms: chunk.chunk_start_ms,
        valid_start_ms: chunk.valid_start_ms,
        valid_end_ms: chunk.valid_end_ms,
        session_duration_ms: chunk.valid_end_ms,
        language: active.language,
        model: active.model.clone(),
        canceled,
    };

    if let Err(error) = enqueuer.enqueue_rt_job(job) {
        tracker.discard_enqueued(&active.session_id);
        return Err(error);
    }
    Ok(())
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
    use crate::transcription::jobs::{JobKind, JobTracker, TranscribeJob};
    use std::sync::Mutex as StdMutex;
    use std::time::Duration as StdDuration;

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
                Arc::new(TestRecordingEventSink::default()),
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
            .start_with_devices(
                &db,
                &data_dir,
                request.clone(),
                &sample_devices(),
                Arc::new(TestRecordingEventSink::default()),
            )
            .unwrap();
        let error = manager
            .start_with_devices(
                &db,
                &data_dir,
                request,
                &sample_devices(),
                Arc::new(TestRecordingEventSink::default()),
            )
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
                Arc::new(TestRecordingEventSink::default()),
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
                Arc::new(TestRecordingEventSink::default()),
            )
            .unwrap();

        let session = stop_without_pending(&manager, &db);

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
                Arc::new(TestRecordingEventSink::default()),
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
            .start_with_devices(
                &db,
                &data_dir,
                request,
                &sample_devices(),
                Arc::new(TestRecordingEventSink::default()),
            )
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
            .start_with_devices(
                &db,
                &data_dir,
                sample_request(Source::System),
                &devices,
                Arc::new(TestRecordingEventSink::default()),
            )
            .unwrap_err();

        assert_eq!(error.code, crate::error::DEVICE_NOT_FOUND);
        assert!(!manager.recording_active());
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn emits_recording_level_and_elapsed_events_while_active() {
        let db = Db::open_in_memory().unwrap();
        let data_dir = temp_data_dir("emits_recording_level_and_elapsed_events_while_active");
        let manager = RecordingManager::new();
        let events = Arc::new(TestRecordingEventSink::default());

        manager
            .start_with_devices(
                &db,
                &data_dir,
                sample_request(Source::Mic),
                &sample_devices(),
                events.clone(),
            )
            .unwrap();
        manager
            .update_levels(SourceLevels {
                mic: 0.25,
                system: 0.0,
            })
            .unwrap();

        wait_until(StdDuration::from_secs(2), || {
            events
                .levels
                .lock()
                .unwrap()
                .iter()
                .any(|level| level.mic == 0.25)
                && !events.elapsed.lock().unwrap().is_empty()
        });

        assert!(events
            .levels
            .lock()
            .unwrap()
            .iter()
            .any(|level| level.mic == 0.25));
        assert!(!events.elapsed.lock().unwrap().is_empty());
        stop_without_pending(&manager, &db);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn record_mixed_samples_writes_wav_and_enqueues_realtime_jobs() {
        let db = Db::open_in_memory().unwrap();
        let data_dir = temp_data_dir("record_mixed_samples_writes_wav_and_enqueues_realtime_jobs");
        let manager = RecordingManager::new();
        let tracker = JobTracker::new();
        let enqueuer = TestRealtimeJobEnqueuer::default();
        let session_id = manager
            .start_with_devices(
                &db,
                &data_dir,
                sample_request(Source::Mic),
                &sample_devices(),
                Arc::new(TestRecordingEventSink::default()),
            )
            .unwrap();

        manager
            .record_mixed_samples(&silence_ms(300), &tracker, &enqueuer)
            .unwrap();
        manager
            .record_mixed_samples(&tone_ms(900), &tracker, &enqueuer)
            .unwrap();
        manager
            .record_mixed_samples(&silence_ms(720), &tracker, &enqueuer)
            .unwrap();

        let jobs = enqueuer.jobs.lock().unwrap();
        assert_eq!(jobs.len(), 1);
        let job = &jobs[0];
        assert_eq!(job.session_id, session_id);
        assert_eq!(job.kind, JobKind::Rt);
        assert_eq!(job.chunk_start_ms, 0);
        assert_eq!(job.valid_start_ms, 300);
        assert_eq!(job.valid_end_ms, 1_200);
        assert_eq!(job.session_duration_ms, 1_200);
        assert_eq!(job.language, Language::Ja);
        assert_eq!(job.model, "medium-q5_0");
        assert_eq!(tracker.pending_count(&session_id), 1);
        assert_eq!(
            db.get_session(&session_id).unwrap().unwrap().status,
            SessionStatus::Recording
        );
        drop(jobs);

        let session = manager.stop(&db, &tracker, &enqueuer).unwrap();
        assert_eq!(session.status, SessionStatus::Transcribing);
        assert!(session.duration_ms >= 1_900);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn recording_active_flag_is_shared_for_worker_preemption() {
        let db = Db::open_in_memory().unwrap();
        let data_dir = temp_data_dir("recording_active_flag_is_shared_for_worker_preemption");
        let manager = RecordingManager::new();
        let active = manager.recording_active_flag();

        assert!(!active.load(Ordering::SeqCst));
        manager
            .start_with_devices(
                &db,
                &data_dir,
                sample_request(Source::Mic),
                &sample_devices(),
                Arc::new(TestRecordingEventSink::default()),
            )
            .unwrap();

        assert!(active.load(Ordering::SeqCst));
        stop_without_pending(&manager, &db);
        assert!(!active.load(Ordering::SeqCst));
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn record_mixed_samples_rolls_back_pending_when_rt_enqueue_fails() {
        let db = Db::open_in_memory().unwrap();
        let data_dir =
            temp_data_dir("record_mixed_samples_rolls_back_pending_when_rt_enqueue_fails");
        let manager = RecordingManager::new();
        let tracker = JobTracker::new();
        let session_id = manager
            .start_with_devices(
                &db,
                &data_dir,
                sample_request(Source::Mic),
                &sample_devices(),
                Arc::new(TestRecordingEventSink::default()),
            )
            .unwrap();

        manager
            .record_mixed_samples(
                &[silence_ms(300), tone_ms(900), silence_ms(720)].concat(),
                &tracker,
                &FailingRealtimeJobEnqueuer,
            )
            .unwrap_err();

        assert_eq!(tracker.pending_count(&session_id), 0);
        assert_eq!(
            db.get_session(&session_id).unwrap().unwrap().status,
            SessionStatus::Recording
        );
        stop_without_pending(&manager, &db);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn stop_flushes_active_speech_and_returns_transcribing_when_pending() {
        let db = Db::open_in_memory().unwrap();
        let data_dir =
            temp_data_dir("stop_flushes_active_speech_and_returns_transcribing_when_pending");
        let manager = RecordingManager::new();
        let tracker = JobTracker::new();
        let enqueuer = TestRealtimeJobEnqueuer::default();
        let session_id = manager
            .start_with_devices(
                &db,
                &data_dir,
                sample_request(Source::Mic),
                &sample_devices(),
                Arc::new(TestRecordingEventSink::default()),
            )
            .unwrap();

        manager
            .record_mixed_samples(&silence_ms(300), &tracker, &enqueuer)
            .unwrap();
        manager
            .record_mixed_samples(&tone_ms(500), &tracker, &enqueuer)
            .unwrap();

        let session = manager.stop(&db, &tracker, &enqueuer).unwrap();

        assert_eq!(session.id, session_id);
        assert_eq!(session.status, SessionStatus::Transcribing);
        assert!(!manager.recording_active());
        assert_eq!(tracker.pending_count(&session_id), 1);
        let jobs = enqueuer.jobs.lock().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].chunk_start_ms, 0);
        assert_eq!(jobs[0].valid_start_ms, 300);
        assert_eq!(jobs[0].valid_end_ms, 800);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn stop_clears_active_flag_when_flush_enqueue_fails() {
        let db = Db::open_in_memory().unwrap();
        let data_dir = temp_data_dir("stop_clears_active_flag_when_flush_enqueue_fails");
        let manager = RecordingManager::new();
        let tracker = JobTracker::new();
        let session_id = manager
            .start_with_devices(
                &db,
                &data_dir,
                sample_request(Source::Mic),
                &sample_devices(),
                Arc::new(TestRecordingEventSink::default()),
            )
            .unwrap();

        manager
            .record_mixed_samples(
                &silence_ms(300),
                &tracker,
                &TestRealtimeJobEnqueuer::default(),
            )
            .unwrap();
        manager
            .record_mixed_samples(&tone_ms(500), &tracker, &TestRealtimeJobEnqueuer::default())
            .unwrap();

        let error = manager
            .stop(&db, &tracker, &FailingRealtimeJobEnqueuer)
            .unwrap_err();

        assert_eq!(error.message, "rt worker unavailable");
        assert!(!manager.recording_active());
        assert_eq!(tracker.pending_count(&session_id), 0);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn recording_limit_state_warns_once_then_reports_max_once() {
        let mut state = RecordingLimitState::default();

        assert_eq!(
            state.next_payload(MAX_RECORDING_MS - RECORDING_LIMIT_WARNING_BEFORE_MS - 1),
            None
        );
        assert_eq!(
            state.next_payload(MAX_RECORDING_MS - RECORDING_LIMIT_WARNING_BEFORE_MS),
            Some(RecordingLimitPayload {
                kind: RecordingLimitKind::Warning,
                elapsed_ms: MAX_RECORDING_MS - RECORDING_LIMIT_WARNING_BEFORE_MS,
                max_ms: MAX_RECORDING_MS,
            })
        );
        assert_eq!(
            state.next_payload(MAX_RECORDING_MS - RECORDING_LIMIT_WARNING_BEFORE_MS + 1),
            None
        );
        assert_eq!(
            state.next_payload(MAX_RECORDING_MS),
            Some(RecordingLimitPayload {
                kind: RecordingLimitKind::MaxReached,
                elapsed_ms: MAX_RECORDING_MS,
                max_ms: MAX_RECORDING_MS,
            })
        );
        assert_eq!(state.next_payload(MAX_RECORDING_MS + 1), None);
    }

    #[test]
    fn recording_limit_state_reports_max_without_prior_warning() {
        let mut state = RecordingLimitState::default();

        assert_eq!(
            state.next_payload(MAX_RECORDING_MS + 1),
            Some(RecordingLimitPayload {
                kind: RecordingLimitKind::MaxReached,
                elapsed_ms: MAX_RECORDING_MS + 1,
                max_ms: MAX_RECORDING_MS,
            })
        );
        assert_eq!(state.next_payload(MAX_RECORDING_MS + 2), None);
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

    fn tone_ms(ms: u64) -> Vec<f32> {
        vec![0.2; samples_for_ms(ms)]
    }

    fn silence_ms(ms: u64) -> Vec<f32> {
        vec![0.0; samples_for_ms(ms)]
    }

    fn samples_for_ms(ms: u64) -> usize {
        (crate::audio::wav::WAV_SAMPLE_RATE as u64 * ms / 1_000) as usize
    }

    fn stop_without_pending(manager: &RecordingManager, db: &Db) -> Session {
        manager
            .stop(db, &JobTracker::new(), &TestRealtimeJobEnqueuer::default())
            .unwrap()
    }

    fn temp_data_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("sokki-recording-{name}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[derive(Default)]
    struct TestRecordingEventSink {
        levels: StdMutex<Vec<RecordingLevelPayload>>,
        elapsed: StdMutex<Vec<RecordingElapsedPayload>>,
        drops: StdMutex<Vec<RecordingDropsPayload>>,
        limits: StdMutex<Vec<RecordingLimitPayload>>,
    }

    #[derive(Default)]
    struct TestRealtimeJobEnqueuer {
        jobs: StdMutex<Vec<TranscribeJob>>,
    }

    impl RealtimeJobEnqueuer for TestRealtimeJobEnqueuer {
        fn enqueue_rt_job(&self, job: TranscribeJob) -> Result<(), AppError> {
            self.jobs.lock().unwrap().push(job);
            Ok(())
        }
    }

    struct FailingRealtimeJobEnqueuer;

    impl RealtimeJobEnqueuer for FailingRealtimeJobEnqueuer {
        fn enqueue_rt_job(&self, _job: TranscribeJob) -> Result<(), AppError> {
            Err(AppError::new(DB_ERROR, "rt worker unavailable"))
        }
    }

    impl RecordingEventSink for TestRecordingEventSink {
        fn emit_level(&self, payload: RecordingLevelPayload) {
            self.levels.lock().unwrap().push(payload);
        }

        fn emit_elapsed(&self, payload: RecordingElapsedPayload) {
            self.elapsed.lock().unwrap().push(payload);
        }

        fn emit_drops(&self, payload: RecordingDropsPayload) {
            self.drops.lock().unwrap().push(payload);
        }

        fn emit_limit(&self, payload: RecordingLimitPayload) {
            self.limits.lock().unwrap().push(payload);
        }
    }

    fn wait_until(timeout: StdDuration, mut condition: impl FnMut() -> bool) {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if condition() {
                return;
            }
            std::thread::sleep(StdDuration::from_millis(20));
        }
    }
}
