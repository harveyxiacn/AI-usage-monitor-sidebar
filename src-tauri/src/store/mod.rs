//! SQLite storage. [BACKEND owns this directory]
//!
//! Schema is the one documented in `docs/ARCHITECTURE.md` §8. All timestamps
//! stored in the DB are unix **milliseconds**.
//!
//! The connection lives behind a `parking_lot::Mutex`; every caller is expected
//! to run the (blocking) queries inside `tokio::task::spawn_blocking`.

pub mod quota;
pub mod usage;

use anyhow::{Context, Result};
use parking_lot::{Mutex, MutexGuard};
use rusqlite::Connection;
use std::path::Path;

pub use quota::{insert_quota_sample, query_quota_history};
pub use usage::{insert_usage_events, query_calendar, query_history, query_sessions, UsageEvent};

/// Bumped whenever the schema changes; migrations live in [`migrate`].
pub const SCHEMA_VERSION: i64 = 2;

/// One row of the `ingest_files` bookkeeping table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IngestFile {
    pub path: String,
    pub provider: String,
    pub size: i64,
    /// File mtime in unix nanoseconds to detect fast same-size rewrites.
    /// Older millisecond rows naturally trigger one safe re-scan.
    pub mtime: i64,
    pub byte_offset: i64,
    pub last_ingested_at: i64,
}

/// Owning handle to the SQLite database.
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Open (creating if needed) the database at `path` and run migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)
            .with_context(|| format!("open sqlite database {}", path.display()))?;
        Self::init(conn)
    }

    /// In-memory database — used by tests and by the `probe` example.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        // WAL keeps readers from blocking the ingestion writer.
        let _: String = conn
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
            .unwrap_or_else(|_| "unknown".to_string());
        conn.execute_batch("PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;")?;
        let db = Db {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Lock the connection. Never hold the guard across an `.await`.
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock()
    }

    /// Create tables / apply migrations based on `meta.schema_version`.
    fn migrate(&self) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT);
            CREATE TABLE IF NOT EXISTS usage_events (
              id INTEGER PRIMARY KEY,
              provider TEXT NOT NULL,
              model TEXT NOT NULL,
              ts INTEGER NOT NULL,
              input_tokens INTEGER NOT NULL DEFAULT 0,
              cache_write_tokens INTEGER NOT NULL DEFAULT 0,
              cache_read_tokens INTEGER NOT NULL DEFAULT 0,
              output_tokens INTEGER NOT NULL DEFAULT 0,
              reasoning_tokens INTEGER NOT NULL DEFAULT 0,
              total_tokens INTEGER NOT NULL DEFAULT 0,
              session_id TEXT,
              request_id TEXT NOT NULL,
              cwd TEXT,
              source_file TEXT,
              UNIQUE(provider, request_id));
            CREATE INDEX IF NOT EXISTS idx_usage_ts ON usage_events(ts);
            CREATE TABLE IF NOT EXISTS quota_samples (
              id INTEGER PRIMARY KEY,
              provider TEXT NOT NULL,
              kind TEXT NOT NULL,
              scope TEXT,
              used_percent REAL NOT NULL,
              resets_at INTEGER,
              plan TEXT,
              ts INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS idx_quota_ts ON quota_samples(provider, ts);
            CREATE TABLE IF NOT EXISTS ingest_files (
              path TEXT PRIMARY KEY,
              provider TEXT NOT NULL,
              size INTEGER NOT NULL,
              mtime INTEGER NOT NULL,
              byte_offset INTEGER NOT NULL,
              last_ingested_at INTEGER NOT NULL);
            "#,
        )?;
        let current: i64 = tx
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        anyhow::ensure!(
            current <= SCHEMA_VERSION,
            "unsupported database schema {current}"
        );
        if current < 2 {
            // An older binary can rewrite the version marker while leaving
            // this additive column intact. Reapplying v2 is still safe.
            let has_effort: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('usage_events') WHERE name='reasoning_effort')",
                [], |row| row.get(0),
            )?;
            if !has_effort {
                tx.execute_batch("ALTER TABLE usage_events ADD COLUMN reasoning_effort TEXT;")?;
            }
            // Previously parsed logs still exist on disk. A single replay
            // enriches their metadata through the normal request-id upsert.
            // Existing usage and quota rows remain intact, even if logs expired.
            tx.execute("DELETE FROM ingest_files", [])?;
        }
        if current < SCHEMA_VERSION {
            tx.execute(
                "INSERT INTO meta(key, value) VALUES('schema_version', ?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [SCHEMA_VERSION.to_string()],
            )?;
            log::info!("usage.db schema {} -> {}", current, SCHEMA_VERSION);
        }
        tx.commit()?;
        Ok(())
    }

    // ---------- ingest_files bookkeeping ----------

    /// Fetch the bookkeeping row for `path`, if any.
    pub fn get_ingest_file(&self, path: &str) -> Result<Option<IngestFile>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT path, provider, size, mtime, byte_offset, last_ingested_at
             FROM ingest_files WHERE path = ?1",
        )?;
        let mut rows = stmt.query([path])?;
        if let Some(row) = rows.next()? {
            Ok(Some(IngestFile {
                path: row.get(0)?,
                provider: row.get(1)?,
                size: row.get(2)?,
                mtime: row.get(3)?,
                byte_offset: row.get(4)?,
                last_ingested_at: row.get(5)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Insert or replace the bookkeeping row for a file.
    pub fn upsert_ingest_file(&self, f: &IngestFile) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO ingest_files(path, provider, size, mtime, byte_offset, last_ingested_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET
               provider=excluded.provider, size=excluded.size, mtime=excluded.mtime,
               byte_offset=excluded.byte_offset, last_ingested_at=excluded.last_ingested_at",
            rusqlite::params![
                f.path,
                f.provider,
                f.size,
                f.mtime,
                f.byte_offset,
                f.last_ingested_at
            ],
        )?;
        Ok(())
    }

    /// Forget every byte offset so the next scan re-reads all files.
    pub fn reset_ingest_offsets(&self) -> Result<()> {
        let conn = self.lock();
        conn.execute("DELETE FROM ingest_files", [])?;
        Ok(())
    }
}

/// Current wall clock in unix milliseconds.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn old_schema(path: &Path) {
        let db = Db::open(path).unwrap();
        db.lock()
            .execute_batch(
                "ALTER TABLE usage_events DROP COLUMN reasoning_effort;
             UPDATE meta SET value='1' WHERE key='schema_version';
             INSERT INTO usage_events(provider,model,ts,total_tokens,request_id)
               VALUES('codex','gpt-6-astra',1000,123,'existing');
             INSERT INTO quota_samples(provider,kind,used_percent,ts)
               VALUES('codex','weekly',37.5,1000);
             INSERT INTO ingest_files VALUES('retained.jsonl','codex',100,123456,100,1000);",
            )
            .unwrap();
    }

    #[test]
    fn v2_migration_preserves_history_and_invalidates_offsets_exactly_once() {
        let dir = crate::commands::test_support::tempdir();
        let path = dir.join("usage.db");
        old_schema(&path);
        let db = Db::open(&path).unwrap();
        assert!(db.get_ingest_file("retained.jsonl").unwrap().is_none());
        let old: (i64, Option<String>) = db
            .lock()
            .query_row(
                "SELECT total_tokens,reasoning_effort FROM usage_events",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(old, (123, None));
        assert_eq!(
            db.lock()
                .query_row("SELECT used_percent FROM quota_samples", [], |r| r
                    .get::<_, f64>(0))
                .unwrap(),
            37.5
        );
        assert_eq!(
            db.lock()
                .query_row(
                    "SELECT value FROM meta WHERE key='schema_version'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "2"
        );
        let offset = IngestFile {
            path: "retained.jsonl".into(),
            provider: "codex".into(),
            size: 100,
            mtime: 123456,
            byte_offset: 100,
            last_ingested_at: 2000,
        };
        db.upsert_ingest_file(&offset).unwrap();
        drop(db);
        let reopened = Db::open(&path).unwrap();
        assert_eq!(
            reopened.get_ingest_file("retained.jsonl").unwrap(),
            Some(offset)
        );
        assert_eq!(usage::count_events(&reopened).unwrap(), 1);
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reapplying_v2_keeps_an_existing_effort_column_and_values() {
        let db = Db::open_in_memory().unwrap();
        db.lock()
            .execute_batch(
                "UPDATE meta SET value='1' WHERE key='schema_version';
             INSERT INTO usage_events(provider,model,ts,total_tokens,request_id,reasoning_effort)
               VALUES('codex','gpt-6-astra',1000,123,'existing','ultra');",
            )
            .unwrap();
        db.migrate().unwrap();
        db.migrate().unwrap();
        assert_eq!(
            db.lock()
                .query_row("SELECT reasoning_effort FROM usage_events", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "ultra"
        );
        assert_eq!(usage::count_events(&db).unwrap(), 1);
    }

    #[test]
    fn failed_migration_rolls_back_column_offsets_and_version_together() {
        let dir = crate::commands::test_support::tempdir();
        let path = dir.join("usage.db");
        old_schema(&path);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TRIGGER stop_offset_reset BEFORE DELETE ON ingest_files BEGIN SELECT RAISE(ABORT, 'test migration failure'); END;").unwrap();
        assert!(Db::open(&path).is_err());
        assert_eq!(
            conn.query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "1"
        );
        assert_eq!(
            conn.query_row("SELECT byte_offset FROM ingest_files", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            100
        );
        assert!(conn
            .prepare("SELECT reasoning_effort FROM usage_events")
            .is_err());
        assert_eq!(
            conn.query_row("SELECT total_tokens FROM usage_events", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            123
        );
        conn.execute_batch("DROP TRIGGER stop_offset_reset;")
            .unwrap();
        drop(conn);
        assert!(Db::open(&path).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
