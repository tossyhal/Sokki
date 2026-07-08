use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::db::{Db, Language, SessionStatus};
use crate::error::{AppError, DB_ERROR};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Rt,
    Batch,
}

#[derive(Clone, Debug)]
pub struct TranscribeJob {
    pub session_id: String,
    pub kind: JobKind,
    pub audio: Vec<f32>,
    pub chunk_start_ms: u64,
    pub valid_start_ms: u64,
    pub valid_end_ms: u64,
    pub session_duration_ms: u64,
    pub language: Language,
    pub model: String,
    pub canceled: Arc<AtomicBool>,
}

impl TranscribeJob {
    pub fn canceled(&self) -> bool {
        self.canceled.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobCompletion {
    StillPending { pending: u32 },
    Done { session_id: String },
    WaitingForRecordingToStop,
}

#[derive(Default)]
pub struct JobTracker {
    state: Mutex<JobTrackerState>,
}

#[derive(Default)]
struct JobTrackerState {
    pending: HashMap<String, u32>,
    cancel_flags: HashMap<String, Arc<AtomicBool>>,
}

impl JobTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue(&self, session_id: &str) -> Arc<AtomicBool> {
        let mut state = self
            .state
            .lock()
            .expect("job tracker mutex should not be poisoned");
        *state.pending.entry(session_id.to_string()).or_insert(0) += 1;
        Arc::clone(
            state
                .cancel_flags
                .entry(session_id.to_string())
                .or_insert_with(|| Arc::new(AtomicBool::new(false))),
        )
    }

    pub fn pending_count(&self, session_id: &str) -> u32 {
        self.state
            .lock()
            .expect("job tracker mutex should not be poisoned")
            .pending
            .get(session_id)
            .copied()
            .unwrap_or(0)
    }

    pub fn cancel(&self, session_id: &str) -> bool {
        let state = self
            .state
            .lock()
            .expect("job tracker mutex should not be poisoned");
        if let Some(flag) = state.cancel_flags.get(session_id) {
            flag.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    pub fn discard_enqueued(&self, session_id: &str) {
        let mut state = self
            .state
            .lock()
            .expect("job tracker mutex should not be poisoned");
        let Some(pending) = state.pending.get_mut(session_id) else {
            return;
        };
        if *pending > 0 {
            *pending -= 1;
        }
        if *pending == 0 {
            state.pending.remove(session_id);
            state.cancel_flags.remove(session_id);
        }
    }

    pub fn finish(
        &self,
        db: &Db,
        session_id: &str,
        recording_active: bool,
    ) -> Result<JobCompletion, AppError> {
        let mut state = self
            .state
            .lock()
            .expect("job tracker mutex should not be poisoned");
        let Some(pending) = state.pending.get_mut(session_id) else {
            return Ok(JobCompletion::StillPending { pending: 0 });
        };
        if *pending > 0 {
            *pending -= 1;
        }

        if *pending > 0 {
            return Ok(JobCompletion::StillPending { pending: *pending });
        }

        if recording_active {
            return Ok(JobCompletion::WaitingForRecordingToStop);
        }

        let session = db
            .get_session(session_id)
            .map_err(db_error)?
            .ok_or_else(|| AppError::new(DB_ERROR, format!("session not found: {session_id}")))?;
        if session.status != SessionStatus::Error {
            db.update_session_status(session_id, SessionStatus::Done, None)
                .map_err(db_error)?;
        }
        state.pending.remove(session_id);
        state.cancel_flags.remove(session_id);

        Ok(JobCompletion::Done {
            session_id: session_id.to_string(),
        })
    }
}

fn db_error(error: rusqlite::Error) -> AppError {
    AppError::new(DB_ERROR, format!("database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Session, Source};

    #[test]
    fn enqueue_and_finish_marks_done_only_after_last_pending_job() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        db.insert_session(&sample_session("session-a", SessionStatus::Transcribing))
            .expect("session should insert");
        let tracker = JobTracker::new();

        tracker.enqueue("session-a");
        tracker.enqueue("session-a");

        assert_eq!(
            tracker.finish(&db, "session-a", false).unwrap(),
            JobCompletion::StillPending { pending: 1 }
        );
        assert_eq!(
            db.get_session("session-a").unwrap().unwrap().status,
            SessionStatus::Transcribing
        );

        assert_eq!(
            tracker.finish(&db, "session-a", false).unwrap(),
            JobCompletion::Done {
                session_id: "session-a".to_string()
            }
        );
        assert_eq!(
            db.get_session("session-a").unwrap().unwrap().status,
            SessionStatus::Done
        );
        assert_eq!(tracker.pending_count("session-a"), 0);
    }

    #[test]
    fn finish_does_not_overwrite_error_status_with_done() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        db.insert_session(&sample_session("session-a", SessionStatus::Error))
            .expect("session should insert");
        let tracker = JobTracker::new();
        tracker.enqueue("session-a");

        assert_eq!(
            tracker.finish(&db, "session-a", false).unwrap(),
            JobCompletion::Done {
                session_id: "session-a".to_string()
            }
        );
        assert_eq!(
            db.get_session("session-a").unwrap().unwrap().status,
            SessionStatus::Error
        );
    }

    #[test]
    fn finish_waits_for_recording_to_stop_before_marking_done() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        db.insert_session(&sample_session("session-a", SessionStatus::Transcribing))
            .expect("session should insert");
        let tracker = JobTracker::new();
        tracker.enqueue("session-a");

        assert_eq!(
            tracker.finish(&db, "session-a", true).unwrap(),
            JobCompletion::WaitingForRecordingToStop
        );
        assert_eq!(
            db.get_session("session-a").unwrap().unwrap().status,
            SessionStatus::Transcribing
        );

        assert_eq!(
            tracker.finish(&db, "session-a", false).unwrap(),
            JobCompletion::Done {
                session_id: "session-a".to_string()
            }
        );
        assert_eq!(
            db.get_session("session-a").unwrap().unwrap().status,
            SessionStatus::Done
        );
    }

    #[test]
    fn cancel_sets_shared_flag_for_enqueued_jobs() {
        let tracker = JobTracker::new();
        let flag = tracker.enqueue("session-a");
        let job = TranscribeJob {
            session_id: "session-a".to_string(),
            kind: JobKind::Batch,
            audio: vec![0.0; 160],
            chunk_start_ms: 0,
            valid_start_ms: 0,
            valid_end_ms: 10,
            session_duration_ms: 10,
            language: Language::Ja,
            model: "medium-q5_0".to_string(),
            canceled: flag,
        };

        assert!(!job.canceled());
        assert!(tracker.cancel("session-a"));
        assert!(job.canceled());
    }

    #[test]
    fn discard_enqueued_rolls_back_pending_without_status_change() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        db.insert_session(&sample_session("session-a", SessionStatus::Recording))
            .expect("session should insert");
        let tracker = JobTracker::new();
        let flag = tracker.enqueue("session-a");

        tracker.discard_enqueued("session-a");

        assert_eq!(tracker.pending_count("session-a"), 0);
        assert!(!flag.load(Ordering::SeqCst));
        assert_eq!(
            db.get_session("session-a").unwrap().unwrap().status,
            SessionStatus::Recording
        );
    }

    fn sample_session(id: &str, status: SessionStatus) -> Session {
        Session {
            id: id.to_string(),
            title: id.to_string(),
            created_at: 1_000,
            duration_ms: 0,
            audio_path: None,
            source: Source::Mic,
            language: Language::Ja,
            model: "medium-q5_0".to_string(),
            status,
            error_message: None,
            drop_count: 0,
        }
    }
}
