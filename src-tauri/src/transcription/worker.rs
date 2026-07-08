use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{select, unbounded, Receiver, RecvTimeoutError, Sender};
use serde::Serialize;
use tauri::Emitter;

use crate::db::{Db, NewSegment, Segment, SessionStatus};
use crate::error::{AppError, DB_ERROR};
use crate::transcription::jobs::{JobCompletion, JobKind, JobTracker, TranscribeJob};

const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(50);
pub const IMPORT_PROGRESS_EVENT: &str = "import://progress";
pub const TRANSCRIPT_SEGMENT_EVENT: &str = "transcript://segment";
pub const SESSION_STATUS_EVENT: &str = "session://status";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgressPayload {
    pub session_id: String,
    pub progress: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatusPayload {
    pub session_id: String,
    pub status: SessionStatus,
    pub message: Option<String>,
}

pub trait TranscriptionEventSink: Send + Sync + 'static {
    fn emit_import_progress(&self, payload: ImportProgressPayload);
    fn emit_transcript_segment(&self, segment: Segment);
    fn emit_session_status(&self, _payload: SessionStatusPayload) {}
}

pub struct NoopTranscriptionEventSink;

impl TranscriptionEventSink for NoopTranscriptionEventSink {
    fn emit_import_progress(&self, _payload: ImportProgressPayload) {}

    fn emit_transcript_segment(&self, _segment: Segment) {}
}

pub struct TauriTranscriptionEventSink {
    app: tauri::AppHandle,
}

impl TauriTranscriptionEventSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl TranscriptionEventSink for TauriTranscriptionEventSink {
    fn emit_import_progress(&self, payload: ImportProgressPayload) {
        let _ = self.app.emit(IMPORT_PROGRESS_EVENT, payload);
    }

    fn emit_transcript_segment(&self, segment: Segment) {
        let _ = self.app.emit(TRANSCRIPT_SEGMENT_EVENT, segment);
    }

    fn emit_session_status(&self, payload: SessionStatusPayload) {
        let _ = self.app.emit(SESSION_STATUS_EVENT, payload);
    }
}

#[derive(Debug)]
pub enum JobProcessResult {
    Completed { segments: Vec<NewSegment> },
    Aborted,
}

pub trait JobProcessor: Send + Sync + 'static {
    fn process(
        &self,
        job: &TranscribeJob,
        should_abort: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<JobProcessResult, AppError>;
}

pub struct NoopJobProcessor;

impl JobProcessor for NoopJobProcessor {
    fn process(
        &self,
        _job: &TranscribeJob,
        should_abort: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<JobProcessResult, AppError> {
        if should_abort() {
            Ok(JobProcessResult::Aborted)
        } else {
            Ok(JobProcessResult::Completed {
                segments: Vec::new(),
            })
        }
    }
}

pub struct TranscribeWorkerHandle {
    rt_tx: Sender<TranscribeJob>,
    batch_tx: Sender<TranscribeJob>,
    shutdown_tx: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

pub struct TranscribeWorkerState {
    handle: Mutex<TranscribeWorkerHandle>,
}

impl TranscribeWorkerState {
    pub fn new(handle: TranscribeWorkerHandle) -> Self {
        Self {
            handle: Mutex::new(handle),
        }
    }

    pub fn enqueue_batch(&self, job: TranscribeJob) -> Result<(), AppError> {
        self.handle
            .lock()
            .expect("transcription worker mutex should not be poisoned")
            .enqueue_batch(job)
    }

    pub fn enqueue_rt(&self, job: TranscribeJob) -> Result<(), AppError> {
        self.handle
            .lock()
            .expect("transcription worker mutex should not be poisoned")
            .enqueue_rt(job)
    }
}

impl TranscribeWorkerHandle {
    pub fn start(
        db: Arc<Db>,
        tracker: Arc<JobTracker>,
        recording_active: Arc<AtomicBool>,
        processor: Arc<dyn JobProcessor>,
        events: Arc<dyn TranscriptionEventSink>,
    ) -> Self {
        let (rt_tx, rt_rx) = unbounded();
        let (batch_tx, batch_rx) = unbounded();
        let (shutdown_tx, shutdown_rx) = unbounded();
        let thread = spawn_worker_loop(WorkerRuntime {
            rt_rx,
            batch_rx,
            shutdown_rx,
            db,
            tracker,
            recording_active,
            processor,
            events,
        });

        Self {
            rt_tx,
            batch_tx,
            shutdown_tx,
            thread: Some(thread),
        }
    }

    pub fn enqueue_rt(&self, job: TranscribeJob) -> Result<(), AppError> {
        self.rt_tx
            .send(job)
            .map_err(|_| AppError::new(DB_ERROR, "transcription worker is stopped"))
    }

    pub fn enqueue_batch(&self, job: TranscribeJob) -> Result<(), AppError> {
        self.batch_tx
            .send(job)
            .map_err(|_| AppError::new(DB_ERROR, "transcription worker is stopped"))
    }

    pub fn shutdown(mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for TranscribeWorkerHandle {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct WorkerRuntime {
    rt_rx: Receiver<TranscribeJob>,
    batch_rx: Receiver<TranscribeJob>,
    shutdown_rx: Receiver<()>,
    db: Arc<Db>,
    tracker: Arc<JobTracker>,
    recording_active: Arc<AtomicBool>,
    processor: Arc<dyn JobProcessor>,
    events: Arc<dyn TranscriptionEventSink>,
}

fn spawn_worker_loop(runtime: WorkerRuntime) -> JoinHandle<()> {
    thread::spawn(move || run_worker_loop(runtime))
}

fn run_worker_loop(runtime: WorkerRuntime) {
    let mut deferred: Option<TranscribeJob> = None;

    loop {
        if runtime.shutdown_rx.try_recv().is_ok() {
            break;
        }

        if let Ok(job) = runtime.rt_rx.try_recv() {
            process_or_finish(&runtime, job, &mut deferred);
            continue;
        }

        if runtime.recording_active.load(Ordering::SeqCst) {
            if wait_for_shutdown(&runtime.shutdown_rx) {
                break;
            }
            continue;
        }

        if let Some(job) = deferred.take() {
            if job.canceled() {
                finish_job(&runtime, &job.session_id);
            } else {
                process_or_finish(&runtime, job, &mut deferred);
            }
            continue;
        }

        select! {
            recv(runtime.shutdown_rx) -> _ => break,
            recv(runtime.rt_rx) -> job => {
                if let Ok(job) = job {
                    process_or_finish(&runtime, job, &mut deferred);
                }
            }
            recv(runtime.batch_rx) -> job => {
                if let Ok(job) = job {
                    if job.canceled() {
                        finish_job(&runtime, &job.session_id);
                    } else {
                        process_or_finish(&runtime, job, &mut deferred);
                    }
                }
            }
            default(WORKER_POLL_INTERVAL) => {}
        }
    }
}

fn wait_for_shutdown(shutdown_rx: &Receiver<()>) -> bool {
    match shutdown_rx.recv_timeout(WORKER_POLL_INTERVAL) {
        Ok(()) | Err(RecvTimeoutError::Disconnected) => true,
        Err(RecvTimeoutError::Timeout) => false,
    }
}

fn process_or_finish(
    runtime: &WorkerRuntime,
    job: TranscribeJob,
    deferred: &mut Option<TranscribeJob>,
) {
    if job.canceled() {
        finish_job(runtime, &job.session_id);
        return;
    }

    let should_abort =
        || job.kind == JobKind::Batch && runtime.recording_active.load(Ordering::SeqCst);

    let result = runtime.processor.process(&job, &should_abort);
    match result {
        Ok(JobProcessResult::Completed { segments }) => {
            match insert_segments(&runtime.db, segments) {
                Ok(inserted_segments) => {
                    for segment in inserted_segments {
                        runtime.events.emit_transcript_segment(segment);
                    }
                    finish_job(runtime, &job.session_id);
                    emit_import_progress(&runtime.events, &job);
                }
                Err(_) => {
                    let _ = runtime.db.update_session_status(
                        &job.session_id,
                        SessionStatus::Error,
                        Some("failed to persist transcription segments"),
                    );
                    finish_job(runtime, &job.session_id);
                }
            }
        }
        Ok(JobProcessResult::Aborted) => {
            *deferred = Some(job);
        }
        Err(error) => {
            let _ = runtime.db.update_session_status(
                &job.session_id,
                SessionStatus::Error,
                Some(&error.message),
            );
            finish_job(runtime, &job.session_id);
        }
    }
}

fn finish_job(runtime: &WorkerRuntime, session_id: &str) {
    let Ok(completion) = runtime.tracker.finish(
        &runtime.db,
        session_id,
        runtime.recording_active.load(Ordering::SeqCst),
    ) else {
        return;
    };

    if let JobCompletion::Done { session_id } = completion {
        if let Ok(Some(session)) = runtime.db.get_session(&session_id) {
            runtime.events.emit_session_status(SessionStatusPayload {
                session_id,
                status: session.status,
                message: session.error_message,
            });
        }
    }
}

fn emit_import_progress(events: &Arc<dyn TranscriptionEventSink>, job: &TranscribeJob) {
    if job.kind != JobKind::Batch {
        return;
    }
    if job.session_duration_ms == 0 {
        return;
    }

    let progress = (job.valid_end_ms as f64 / job.session_duration_ms as f64).clamp(0.0, 1.0);
    events.emit_import_progress(ImportProgressPayload {
        session_id: job.session_id.clone(),
        progress,
    });
}

fn insert_segments(db: &Db, segments: Vec<NewSegment>) -> Result<Vec<Segment>, AppError> {
    let mut inserted = Vec::with_capacity(segments.len());
    for segment in segments {
        inserted.push(
            db.insert_segment(&segment)
                .map_err(|error| AppError::new(DB_ERROR, format!("database error: {error}")))?,
        );
    }
    Ok(inserted)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Language, Session, Source};
    use crate::transcription::jobs::JobKind;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    type ProcessedMessage = (String, JobKind);
    type ProcessedSender = Sender<ProcessedMessage>;
    type ProcessedReceiver = Receiver<ProcessedMessage>;

    #[test]
    fn rt_jobs_are_processed_before_queued_batch_jobs() {
        let fixture = WorkerFixture::new();
        fixture.insert_session("rt-session");
        fixture.insert_session("batch-session");
        let processor = Arc::new(RecordingProcessor::default());
        let (rt_tx, rt_rx) = unbounded();
        let (batch_tx, batch_rx) = unbounded();
        let (shutdown_tx, shutdown_rx) = unbounded();

        let batch_job = fixture.job("batch-session", JobKind::Batch);
        let rt_job = fixture.job("rt-session", JobKind::Rt);
        batch_tx.send(batch_job).unwrap();
        rt_tx.send(rt_job).unwrap();

        let thread = spawn_worker_loop(WorkerRuntime {
            rt_rx,
            batch_rx,
            shutdown_rx,
            db: Arc::clone(&fixture.db),
            tracker: Arc::clone(&fixture.tracker),
            recording_active: Arc::clone(&fixture.recording_active),
            processor: processor.clone(),
            events: noop_events(),
        });

        assert_eq!(
            processor.recv_processed(),
            ("rt-session".to_string(), JobKind::Rt)
        );
        assert_eq!(
            processor.recv_processed(),
            ("batch-session".to_string(), JobKind::Batch)
        );
        shutdown_tx.send(()).unwrap();
        thread.join().unwrap();
    }

    #[test]
    fn batch_jobs_wait_while_recording_is_active() {
        let fixture = WorkerFixture::new();
        fixture.insert_session("batch-session");
        fixture.recording_active.store(true, Ordering::SeqCst);
        let processor = Arc::new(RecordingProcessor::default());
        let worker = TranscribeWorkerHandle::start(
            Arc::clone(&fixture.db),
            Arc::clone(&fixture.tracker),
            Arc::clone(&fixture.recording_active),
            processor.clone(),
            noop_events(),
        );

        worker
            .enqueue_batch(fixture.job("batch-session", JobKind::Batch))
            .unwrap();
        assert!(
            processor
                .try_recv_processed(WORKER_POLL_INTERVAL * 2)
                .is_none(),
            "batch job should not start while recording is active"
        );

        fixture.recording_active.store(false, Ordering::SeqCst);
        assert_eq!(
            processor.recv_processed(),
            ("batch-session".to_string(), JobKind::Batch)
        );
        worker.shutdown();
    }

    #[test]
    fn aborted_batch_job_is_deferred_and_reprocessed_after_recording_stops() {
        let fixture = WorkerFixture::new();
        fixture.insert_session("batch-session");
        let (processed_tx, processed_rx) = processed_channel();
        let processor = Arc::new(AbortFirstBatchProcessor {
            processed_tx,
            attempt: AtomicUsize::new(0),
            recording_active: Arc::clone(&fixture.recording_active),
        });
        let worker = TranscribeWorkerHandle::start(
            Arc::clone(&fixture.db),
            Arc::clone(&fixture.tracker),
            Arc::clone(&fixture.recording_active),
            processor,
            noop_events(),
        );

        worker
            .enqueue_batch(fixture.job("batch-session", JobKind::Batch))
            .unwrap();

        assert_eq!(
            processed_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("first attempt should run"),
            ("batch-session".to_string(), JobKind::Batch)
        );
        assert_eq!(fixture.tracker.pending_count("batch-session"), 1);
        fixture.recording_active.store(false, Ordering::SeqCst);
        assert_eq!(
            processed_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("deferred attempt should run"),
            ("batch-session".to_string(), JobKind::Batch)
        );
        eventually_done(&fixture, "batch-session");
        worker.shutdown();
    }

    #[test]
    fn canceled_deferred_job_is_skipped_and_finished() {
        let fixture = WorkerFixture::new();
        fixture.insert_session("batch-session");
        let (processed_tx, processed_rx) = processed_channel();
        let processor = Arc::new(AbortFirstBatchProcessor {
            processed_tx,
            attempt: AtomicUsize::new(0),
            recording_active: Arc::clone(&fixture.recording_active),
        });
        let worker = TranscribeWorkerHandle::start(
            Arc::clone(&fixture.db),
            Arc::clone(&fixture.tracker),
            Arc::clone(&fixture.recording_active),
            processor,
            noop_events(),
        );
        let job = fixture.job("batch-session", JobKind::Batch);
        let canceled = Arc::clone(&job.canceled);

        worker.enqueue_batch(job).unwrap();
        processed_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first attempt should run");
        canceled.store(true, Ordering::SeqCst);
        fixture.recording_active.store(false, Ordering::SeqCst);

        assert!(
            processed_rx.recv_timeout(WORKER_POLL_INTERVAL * 2).is_err(),
            "canceled deferred job should not be processed again"
        );
        eventually_done(&fixture, "batch-session");
        worker.shutdown();
    }

    #[test]
    fn canceled_rt_job_is_skipped_and_finished() {
        let fixture = WorkerFixture::new();
        fixture.insert_session("rt-session");
        let processor = Arc::new(RecordingProcessor::default());
        let worker = TranscribeWorkerHandle::start(
            Arc::clone(&fixture.db),
            Arc::clone(&fixture.tracker),
            Arc::clone(&fixture.recording_active),
            processor.clone(),
            noop_events(),
        );
        let job = fixture.job("rt-session", JobKind::Rt);
        job.canceled.store(true, Ordering::SeqCst);

        worker.enqueue_rt(job).unwrap();

        assert!(
            processor
                .try_recv_processed(WORKER_POLL_INTERVAL * 2)
                .is_none(),
            "canceled rt job should not be processed"
        );
        eventually_done(&fixture, "rt-session");
        worker.shutdown();
    }

    #[test]
    fn completed_batch_job_emits_import_progress_from_valid_range() {
        let fixture = WorkerFixture::new();
        fixture.insert_session("batch-session");
        let processor = Arc::new(RecordingProcessor::default());
        let events = Arc::new(RecordingEventSink::default());
        let worker = TranscribeWorkerHandle::start(
            Arc::clone(&fixture.db),
            Arc::clone(&fixture.tracker),
            Arc::clone(&fixture.recording_active),
            processor.clone(),
            events.clone(),
        );
        let mut job = fixture.job("batch-session", JobKind::Batch);
        job.valid_end_ms = 10;
        job.session_duration_ms = 20;

        worker.enqueue_batch(job).unwrap();

        assert_eq!(
            processor.recv_processed(),
            ("batch-session".to_string(), JobKind::Batch)
        );
        eventually_done(&fixture, "batch-session");
        assert_eq!(
            events.payloads.lock().unwrap().as_slice(),
            &[ImportProgressPayload {
                session_id: "batch-session".to_string(),
                progress: 0.5,
            }]
        );
        worker.shutdown();
    }

    #[test]
    fn completed_job_emits_inserted_transcript_segments() {
        let fixture = WorkerFixture::new();
        fixture.insert_session("session-a");
        let processor = Arc::new(SegmentProcessor);
        let events = Arc::new(RecordingEventSink::default());
        let worker = TranscribeWorkerHandle::start(
            Arc::clone(&fixture.db),
            Arc::clone(&fixture.tracker),
            Arc::clone(&fixture.recording_active),
            processor,
            events.clone(),
        );

        worker
            .enqueue_batch(fixture.job("session-a", JobKind::Batch))
            .unwrap();

        eventually_done(&fixture, "session-a");
        let emitted = events.segments.lock().unwrap().clone();
        assert_eq!(emitted.len(), 1);
        assert!(emitted[0].id > 0);
        assert_eq!(emitted[0].session_id, "session-a");
        assert_eq!(emitted[0].start_ms, 100);
        assert_eq!(emitted[0].text, "hello");
        worker.shutdown();
    }

    #[test]
    fn completed_job_emits_done_status() {
        let fixture = WorkerFixture::new();
        fixture.insert_session("session-a");
        let processor = Arc::new(RecordingProcessor::default());
        let events = Arc::new(RecordingEventSink::default());
        let worker = TranscribeWorkerHandle::start(
            Arc::clone(&fixture.db),
            Arc::clone(&fixture.tracker),
            Arc::clone(&fixture.recording_active),
            processor,
            events.clone(),
        );

        worker
            .enqueue_rt(fixture.job("session-a", JobKind::Rt))
            .unwrap();

        eventually_done(&fixture, "session-a");
        assert_eq!(
            events.statuses.lock().unwrap().as_slice(),
            &[SessionStatusPayload {
                session_id: "session-a".to_string(),
                status: SessionStatus::Done,
                message: None,
            }]
        );
        worker.shutdown();
    }

    #[test]
    fn failed_job_emits_error_status() {
        let fixture = WorkerFixture::new();
        fixture.insert_session("session-a");
        let processor = Arc::new(FailingProcessor);
        let events = Arc::new(RecordingEventSink::default());
        let worker = TranscribeWorkerHandle::start(
            Arc::clone(&fixture.db),
            Arc::clone(&fixture.tracker),
            Arc::clone(&fixture.recording_active),
            processor,
            events.clone(),
        );

        worker
            .enqueue_rt(fixture.job("session-a", JobKind::Rt))
            .unwrap();

        eventually_done(&fixture, "session-a");
        assert_eq!(
            events.statuses.lock().unwrap().as_slice(),
            &[SessionStatusPayload {
                session_id: "session-a".to_string(),
                status: SessionStatus::Error,
                message: Some("transcribe failed".to_string()),
            }]
        );
        worker.shutdown();
    }

    struct RecordingProcessor {
        processed_tx: ProcessedSender,
        processed_rx: ProcessedReceiver,
    }

    impl Default for RecordingProcessor {
        fn default() -> Self {
            let (processed_tx, processed_rx) = processed_channel();
            Self {
                processed_tx,
                processed_rx,
            }
        }
    }

    impl RecordingProcessor {
        fn recv_processed(&self) -> ProcessedMessage {
            self.processed_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("job should be processed")
        }

        fn try_recv_processed(&self, timeout: Duration) -> Option<ProcessedMessage> {
            self.processed_rx.recv_timeout(timeout).ok()
        }
    }

    impl JobProcessor for RecordingProcessor {
        fn process(
            &self,
            job: &TranscribeJob,
            _should_abort: &(dyn Fn() -> bool + Send + Sync),
        ) -> Result<JobProcessResult, AppError> {
            self.processed_tx
                .send((job.session_id.clone(), job.kind))
                .expect("processed receiver should be open");
            Ok(JobProcessResult::Completed {
                segments: Vec::new(),
            })
        }
    }

    struct SegmentProcessor;

    impl JobProcessor for SegmentProcessor {
        fn process(
            &self,
            job: &TranscribeJob,
            _should_abort: &(dyn Fn() -> bool + Send + Sync),
        ) -> Result<JobProcessResult, AppError> {
            Ok(JobProcessResult::Completed {
                segments: vec![NewSegment {
                    session_id: job.session_id.clone(),
                    start_ms: 100,
                    end_ms: 500,
                    text: "hello".to_string(),
                    lang: Some("en".to_string()),
                }],
            })
        }
    }

    struct FailingProcessor;

    impl JobProcessor for FailingProcessor {
        fn process(
            &self,
            _job: &TranscribeJob,
            _should_abort: &(dyn Fn() -> bool + Send + Sync),
        ) -> Result<JobProcessResult, AppError> {
            Err(AppError::new(DB_ERROR, "transcribe failed"))
        }
    }

    struct AbortFirstBatchProcessor {
        processed_tx: ProcessedSender,
        attempt: AtomicUsize,
        recording_active: Arc<AtomicBool>,
    }

    impl JobProcessor for AbortFirstBatchProcessor {
        fn process(
            &self,
            job: &TranscribeJob,
            should_abort: &(dyn Fn() -> bool + Send + Sync),
        ) -> Result<JobProcessResult, AppError> {
            self.processed_tx
                .send((job.session_id.clone(), job.kind))
                .expect("processed receiver should be open");
            if job.kind == JobKind::Batch && self.attempt.fetch_add(1, Ordering::SeqCst) == 0 {
                self.recording_active.store(true, Ordering::SeqCst);
                assert!(should_abort());
                Ok(JobProcessResult::Aborted)
            } else {
                Ok(JobProcessResult::Completed {
                    segments: Vec::new(),
                })
            }
        }
    }

    fn processed_channel() -> (ProcessedSender, ProcessedReceiver) {
        unbounded()
    }

    #[derive(Default)]
    struct RecordingEventSink {
        payloads: Mutex<Vec<ImportProgressPayload>>,
        segments: Mutex<Vec<Segment>>,
        statuses: Mutex<Vec<SessionStatusPayload>>,
    }

    impl TranscriptionEventSink for RecordingEventSink {
        fn emit_import_progress(&self, payload: ImportProgressPayload) {
            self.payloads.lock().unwrap().push(payload);
        }

        fn emit_transcript_segment(&self, segment: Segment) {
            self.segments.lock().unwrap().push(segment);
        }

        fn emit_session_status(&self, payload: SessionStatusPayload) {
            self.statuses.lock().unwrap().push(payload);
        }
    }

    fn noop_events() -> Arc<dyn TranscriptionEventSink> {
        Arc::new(NoopTranscriptionEventSink)
    }

    struct WorkerFixture {
        db: Arc<Db>,
        tracker: Arc<JobTracker>,
        recording_active: Arc<AtomicBool>,
    }

    impl WorkerFixture {
        fn new() -> Self {
            Self {
                db: Arc::new(Db::open_in_memory().unwrap()),
                tracker: Arc::new(JobTracker::new()),
                recording_active: Arc::new(AtomicBool::new(false)),
            }
        }

        fn insert_session(&self, id: &str) {
            self.db
                .insert_session(&Session {
                    id: id.to_string(),
                    title: id.to_string(),
                    created_at: 1_000,
                    duration_ms: 0,
                    audio_path: None,
                    source: Source::Import,
                    language: Language::Ja,
                    model: "medium-q5_0".to_string(),
                    status: SessionStatus::Transcribing,
                    error_message: None,
                    drop_count: 0,
                })
                .unwrap();
        }

        fn job(&self, session_id: &str, kind: JobKind) -> TranscribeJob {
            let canceled = self.tracker.enqueue(session_id);
            TranscribeJob {
                session_id: session_id.to_string(),
                kind,
                audio: vec![0.0; 160],
                chunk_start_ms: 0,
                valid_start_ms: 0,
                valid_end_ms: 10,
                session_duration_ms: 10,
                language: Language::Ja,
                model: "medium-q5_0".to_string(),
                canceled,
            }
        }
    }

    fn eventually_done(fixture: &WorkerFixture, session_id: &str) {
        for _ in 0..20 {
            if fixture.tracker.pending_count(session_id) == 0 {
                assert_eq!(
                    fixture.db.get_session(session_id).unwrap().unwrap().status,
                    SessionStatus::Done
                );
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("session did not reach done");
    }
}
