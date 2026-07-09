use std::path::Path;

use crate::audio::wav::repair_wav;
use crate::db::{Db, SessionStatus};
use crate::error::{AppError, DB_ERROR};

const RECORDING_INTERRUPTED_MESSAGE: &str = "recording interrupted by app shutdown";
const TRANSCRIPTION_INTERRUPTED_MESSAGE: &str = "transcription interrupted by app shutdown";

pub fn recover_interrupted_sessions(db: &Db, data_dir: &Path) -> Result<usize, AppError> {
    let sessions = db.list_sessions().map_err(db_error)?;
    let mut recovered = 0;

    for session in sessions {
        match session.status {
            SessionStatus::Recording => {
                if let Some(audio_path) = session.audio_path.as_deref() {
                    let path = audio_path_for_recovery(data_dir, audio_path);
                    match repair_wav(&path) {
                        Ok(duration_ms) => {
                            db.update_session_duration(
                                &session.id,
                                duration_ms as i64,
                                session.drop_count,
                            )
                            .map_err(db_error)?;
                        }
                        Err(error) => {
                            log::warn!(
                                "failed to repair interrupted wav for session {}: {}",
                                session.id,
                                error
                            );
                        }
                    }
                }
                db.update_session_status(
                    &session.id,
                    SessionStatus::Interrupted,
                    Some(RECORDING_INTERRUPTED_MESSAGE),
                )
                .map_err(db_error)?;
                recovered += 1;
            }
            SessionStatus::Transcribing => {
                db.update_session_status(
                    &session.id,
                    SessionStatus::Interrupted,
                    Some(TRANSCRIPTION_INTERRUPTED_MESSAGE),
                )
                .map_err(db_error)?;
                recovered += 1;
            }
            SessionStatus::Done | SessionStatus::Error | SessionStatus::Interrupted => {}
        }
    }

    Ok(recovered)
}

fn audio_path_for_recovery(data_dir: &Path, audio_path: &str) -> std::path::PathBuf {
    let path = Path::new(audio_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        data_dir.join(path)
    }
}

fn db_error(error: rusqlite::Error) -> AppError {
    AppError::new(DB_ERROR, format!("database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    use crate::audio::wav::WAV_SAMPLE_RATE;
    use crate::db::{Language, Session, SessionStatus, Source};

    #[test]
    fn recovers_recording_session_by_repairing_wav_and_marking_interrupted() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        let data_dir = temp_data_dir("recovers_recording_session");
        let wav_path = data_dir.join("recordings").join("recording.wav");
        write_interrupted_wav(&wav_path, &[0; 16_000]);
        db.insert_session(&sample_session(
            "recording-session",
            SessionStatus::Recording,
            Some(wav_path.display().to_string()),
        ))
        .expect("recording session should insert");

        let recovered = recover_interrupted_sessions(&db, &data_dir).unwrap();

        assert_eq!(recovered, 1);
        let session = db
            .get_session("recording-session")
            .unwrap()
            .expect("session should remain");
        assert_eq!(session.status, SessionStatus::Interrupted);
        assert_eq!(
            session.error_message.as_deref(),
            Some("recording interrupted by app shutdown")
        );
        assert_eq!(session.duration_ms, 1_000);
        let reader = hound::WavReader::open(&wav_path).unwrap();
        assert_eq!(reader.duration(), 16_000);
        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn recovers_transcribing_session_without_restarting_job_queue() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        let data_dir = temp_data_dir("recovers_transcribing_session");
        db.insert_session(&sample_session(
            "transcribing-session",
            SessionStatus::Transcribing,
            None,
        ))
        .expect("transcribing session should insert");

        let recovered = recover_interrupted_sessions(&db, &data_dir).unwrap();

        assert_eq!(recovered, 1);
        let session = db
            .get_session("transcribing-session")
            .unwrap()
            .expect("session should remain");
        assert_eq!(session.status, SessionStatus::Interrupted);
        assert_eq!(
            session.error_message.as_deref(),
            Some("transcription interrupted by app shutdown")
        );
        assert_eq!(session.duration_ms, 0);
        let _ = fs::remove_dir_all(data_dir);
    }

    fn sample_session(id: &str, status: SessionStatus, audio_path: Option<String>) -> Session {
        Session {
            id: id.to_string(),
            title: format!("Session {id}"),
            created_at: 1_000,
            duration_ms: 0,
            audio_path,
            source: Source::Mic,
            language: Language::Ja,
            model: "medium-q5_0".to_string(),
            status,
            error_message: None,
            drop_count: 0,
        }
    }

    fn temp_data_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("sokki-{name}"));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("recordings")).unwrap();
        path
    }

    fn write_interrupted_wav(path: &Path, samples: &[i16]) {
        let mut file = fs::File::create(path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&WAV_SAMPLE_RATE.to_le_bytes()).unwrap();
        file.write_all(&(WAV_SAMPLE_RATE * 2).to_le_bytes())
            .unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap();
        for sample in samples {
            file.write_all(&sample.to_le_bytes()).unwrap();
        }
    }
}
