use std::{error::Error, fmt, sync::Mutex};

use rusqlite::{params, types::Type, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: i64 = 1;

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub duration_ms: i64,
    pub audio_path: Option<String>,
    pub source: Source,
    pub language: Language,
    pub model: String,
    pub status: SessionStatus,
    pub error_message: Option<String>,
    pub drop_count: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Mic,
    System,
    Mix,
    Import,
}

impl Source {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Mic => "mic",
            Self::System => "system",
            Self::Mix => "mix",
            Self::Import => "import",
        }
    }

    fn from_db_str(value: String, column: usize) -> rusqlite::Result<Self> {
        match value.as_str() {
            "mic" => Ok(Self::Mic),
            "system" => Ok(Self::System),
            "mix" => Ok(Self::Mix),
            "import" => Ok(Self::Import),
            _ => Err(parse_enum_error(column, value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Ja,
    En,
    Auto,
}

impl Language {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Ja => "ja",
            Self::En => "en",
            Self::Auto => "auto",
        }
    }

    fn from_db_str(value: String, column: usize) -> rusqlite::Result<Self> {
        match value.as_str() {
            "ja" => Ok(Self::Ja),
            "en" => Ok(Self::En),
            "auto" => Ok(Self::Auto),
            _ => Err(parse_enum_error(column, value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Recording,
    Transcribing,
    Done,
    Error,
    Interrupted,
}

impl SessionStatus {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Transcribing => "transcribing",
            Self::Done => "done",
            Self::Error => "error",
            Self::Interrupted => "interrupted",
        }
    }

    fn from_db_str(value: String, column: usize) -> rusqlite::Result<Self> {
        match value.as_str() {
            "recording" => Ok(Self::Recording),
            "transcribing" => Ok(Self::Transcribing),
            "done" => Ok(Self::Done),
            "error" => Ok(Self::Error),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(parse_enum_error(column, value)),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: i64,
    pub session_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub lang: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewSegment {
    pub session_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub lang: Option<String>,
}

#[derive(Debug)]
struct ParseEnumError {
    value: String,
}

impl fmt::Display for ParseEnumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid enum value in database: {}", self.value)
    }
}

impl Error for ParseEnumError {}

fn parse_enum_error(column: usize, value: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        Type::Text,
        Box::new(ParseEnumError { value }),
    )
}

impl Db {
    pub fn open(path: impl AsRef<std::path::Path>) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn with_connection<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let conn = self
            .conn
            .lock()
            .expect("database mutex should not be poisoned");
        f(&conn)
    }

    pub fn insert_session(&self, session: &Session) -> rusqlite::Result<()> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO sessions (
                    id, title, created_at, duration_ms, audio_path, source, language, model,
                    status, error_message, drop_count
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    session.id,
                    session.title,
                    session.created_at,
                    session.duration_ms,
                    session.audio_path,
                    session.source.as_db_str(),
                    session.language.as_db_str(),
                    session.model,
                    session.status.as_db_str(),
                    session.error_message,
                    session.drop_count,
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_sessions(&self) -> rusqlite::Result<Vec<Session>> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, title, created_at, duration_ms, audio_path, source, language, model,
                        status, error_message, drop_count
                 FROM sessions
                 ORDER BY created_at DESC",
            )?;
            let sessions = stmt.query_map([], session_from_row)?.collect();
            sessions
        })
    }

    pub fn get_session(&self, id: &str) -> rusqlite::Result<Option<Session>> {
        self.with_connection(|conn| {
            conn.query_row(
                "SELECT id, title, created_at, duration_ms, audio_path, source, language, model,
                        status, error_message, drop_count
                 FROM sessions
                 WHERE id = ?1",
                params![id],
                session_from_row,
            )
            .optional()
        })
    }

    pub fn rename_session(&self, id: &str, title: &str) -> rusqlite::Result<bool> {
        self.with_connection(|conn| {
            let changed = conn.execute(
                "UPDATE sessions SET title = ?1 WHERE id = ?2",
                params![title, id],
            )?;
            Ok(changed > 0)
        })
    }

    pub fn update_session_status(
        &self,
        id: &str,
        status: SessionStatus,
        error_message: Option<&str>,
    ) -> rusqlite::Result<bool> {
        self.with_connection(|conn| {
            let changed = conn.execute(
                "UPDATE sessions SET status = ?1, error_message = ?2 WHERE id = ?3",
                params![status.as_db_str(), error_message, id],
            )?;
            Ok(changed > 0)
        })
    }

    pub fn update_session_duration(
        &self,
        id: &str,
        duration_ms: i64,
        drop_count: i64,
    ) -> rusqlite::Result<bool> {
        self.with_connection(|conn| {
            let changed = conn.execute(
                "UPDATE sessions SET duration_ms = ?1, drop_count = ?2 WHERE id = ?3",
                params![duration_ms, drop_count, id],
            )?;
            Ok(changed > 0)
        })
    }

    pub fn delete_session(&self, id: &str) -> rusqlite::Result<bool> {
        self.with_connection(|conn| {
            let changed = conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
            Ok(changed > 0)
        })
    }

    pub fn insert_segment(&self, segment: &NewSegment) -> rusqlite::Result<Segment> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO segments (session_id, start_ms, end_ms, text, lang)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    segment.session_id,
                    segment.start_ms,
                    segment.end_ms,
                    segment.text,
                    segment.lang,
                ],
            )?;

            Ok(Segment {
                id: conn.last_insert_rowid(),
                session_id: segment.session_id.clone(),
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                text: segment.text.clone(),
                lang: segment.lang.clone(),
            })
        })
    }

    pub fn list_segments(&self, session_id: &str) -> rusqlite::Result<Vec<Segment>> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, start_ms, end_ms, text, lang
                 FROM segments
                 WHERE session_id = ?1
                 ORDER BY start_ms ASC, id ASC",
            )?;
            let segments = stmt
                .query_map(params![session_id], segment_from_row)?
                .collect();
            segments
        })
    }

    pub fn delete_segments_for_session(&self, session_id: &str) -> rusqlite::Result<usize> {
        self.with_connection(|conn| {
            conn.execute(
                "DELETE FROM segments WHERE session_id = ?1",
                params![session_id],
            )
        })
    }
}

fn session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
        duration_ms: row.get(3)?,
        audio_path: row.get(4)?,
        source: Source::from_db_str(row.get(5)?, 5)?,
        language: Language::from_db_str(row.get(6)?, 6)?,
        model: row.get(7)?,
        status: SessionStatus::from_db_str(row.get(8)?, 8)?,
        error_message: row.get(9)?,
        drop_count: row.get(10)?,
    })
}

fn segment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Segment> {
    Ok(Segment {
        id: row.get(0)?,
        session_id: row.get(1)?,
        start_ms: row.get(2)?,
        end_ms: row.get(3)?,
        text: row.get(4)?,
        lang: row.get(5)?,
    })
}

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS sessions (
          id            TEXT PRIMARY KEY,
          title         TEXT NOT NULL,
          created_at    INTEGER NOT NULL,
          duration_ms   INTEGER NOT NULL DEFAULT 0,
          audio_path    TEXT,
          source        TEXT NOT NULL,
          language      TEXT NOT NULL,
          model         TEXT NOT NULL,
          status        TEXT NOT NULL,
          error_message TEXT,
          drop_count    INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS segments (
          id         INTEGER PRIMARY KEY AUTOINCREMENT,
          session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
          start_ms   INTEGER NOT NULL,
          end_ms     INTEGER NOT NULL,
          text       TEXT NOT NULL,
          lang       TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_segments_session ON segments(session_id, start_ms);

        CREATE TABLE IF NOT EXISTS schema_meta (
          key   TEXT PRIMARY KEY,
          value TEXT NOT NULL
        );
        ",
    )?;

    conn.execute(
        "INSERT INTO schema_meta(key, value) VALUES('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![SCHEMA_VERSION.to_string()],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_creates_schema_and_version_meta() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");

        db.with_connection(|conn| {
            let schema_version: String = conn.query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(schema_version, SCHEMA_VERSION.to_string());

            let session_columns: i64 =
                conn.query_row("SELECT COUNT(*) FROM pragma_table_info('sessions')", [], |row| {
                    row.get(0)
                })?;
            assert_eq!(session_columns, 11);

            let segment_columns: i64 =
                conn.query_row("SELECT COUNT(*) FROM pragma_table_info('segments')", [], |row| {
                    row.get(0)
                })?;
            assert_eq!(segment_columns, 6);

            let index_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_segments_session'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(index_count, 1);

            Ok(())
        })
        .expect("schema assertions should query successfully");
    }

    #[test]
    fn session_crud_updates_and_cascades_segments() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        let mut session = sample_session("session-a", 2_000);

        db.insert_session(&session).expect("session should insert");
        assert_eq!(db.get_session("session-a").unwrap(), Some(session.clone()));

        db.insert_session(&sample_session("session-b", 3_000))
            .expect("second session should insert");
        let sessions = db.list_sessions().expect("sessions should list");
        assert_eq!(
            sessions
                .iter()
                .map(|session| session.id.as_str())
                .collect::<Vec<_>>(),
            vec!["session-b", "session-a"]
        );

        assert!(db
            .rename_session("session-a", "renamed")
            .expect("rename should succeed"));
        assert!(db
            .update_session_duration("session-a", 12_345, 7)
            .expect("duration should update"));
        assert!(db
            .update_session_status("session-a", SessionStatus::Error, Some("failed"))
            .expect("status should update"));

        session.title = "renamed".to_string();
        session.duration_ms = 12_345;
        session.drop_count = 7;
        session.status = SessionStatus::Error;
        session.error_message = Some("failed".to_string());
        assert_eq!(db.get_session("session-a").unwrap(), Some(session));

        db.insert_segment(&NewSegment {
            session_id: "session-a".to_string(),
            start_ms: 0,
            end_ms: 1_000,
            text: "hello".to_string(),
            lang: Some("en".to_string()),
        })
        .expect("segment should insert");
        assert!(db
            .delete_session("session-a")
            .expect("session should delete"));
        assert_eq!(db.get_session("session-a").unwrap(), None);
        assert!(db
            .list_segments("session-a")
            .expect("segments should list")
            .is_empty());
        assert!(!db
            .delete_session("missing")
            .expect("missing delete should be ok"));
    }

    #[test]
    fn segment_crud_lists_in_timeline_order() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        db.insert_session(&sample_session("session-a", 1_000))
            .expect("session should insert");

        let late = db
            .insert_segment(&NewSegment {
                session_id: "session-a".to_string(),
                start_ms: 1_000,
                end_ms: 1_500,
                text: "second".to_string(),
                lang: None,
            })
            .expect("late segment should insert");
        let early = db
            .insert_segment(&NewSegment {
                session_id: "session-a".to_string(),
                start_ms: 100,
                end_ms: 600,
                text: "first".to_string(),
                lang: Some("ja".to_string()),
            })
            .expect("early segment should insert");

        let segments = db.list_segments("session-a").expect("segments should list");
        assert_eq!(
            segments
                .iter()
                .map(|segment| segment.id)
                .collect::<Vec<_>>(),
            vec![early.id, late.id]
        );
        assert_eq!(segments[0].text, "first");

        assert_eq!(
            db.delete_segments_for_session("session-a")
                .expect("segments should delete"),
            2
        );
        assert!(db
            .list_segments("session-a")
            .expect("segments should list after delete")
            .is_empty());
    }

    fn sample_session(id: &str, created_at: i64) -> Session {
        Session {
            id: id.to_string(),
            title: format!("Session {id}"),
            created_at,
            duration_ms: 0,
            audio_path: Some(format!("recordings/{id}.wav")),
            source: Source::Mic,
            language: Language::Ja,
            model: "medium-q5_0".to_string(),
            status: SessionStatus::Recording,
            error_message: None,
            drop_count: 0,
        }
    }
}
