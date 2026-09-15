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
pub use usage::{insert_usage_events, query_history, UsageEvent};

/// Bumped whenever the schema changes; migrations live in [`migrate`].
pub const SCHEMA_VERSION: i64 = 1;

/// One row of the `ingest_files` bookkeeping table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IngestFile {
    pub path: String,
    pub provider: String,
    pub size: i64,
    /// file mtime in unix milliseconds
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
        let conn = self.lock();
        conn.execute_batch(
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
        let current: i64 = conn
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if current != SCHEMA_VERSION {
            // Future migrations go here, keyed on `current`.
            conn.execute(
                "INSERT INTO meta(key, value) VALUES('schema_version', ?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [SCHEMA_VERSION.to_string()],
            )?;
            log::info!("usage.db schema {} -> {}", current, SCHEMA_VERSION);
        }
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
