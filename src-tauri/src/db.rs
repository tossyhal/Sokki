use std::sync::Mutex;

use rusqlite::{params, Connection};

pub const SCHEMA_VERSION: i64 = 1;

pub struct Db {
    conn: Mutex<Connection>,
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
}
