//! Incremental ingestion of the providers' session logs. [BACKEND owns this]
//!
//! One pass = walk every `*.jsonl` under the known log roots, skip the files
//! whose `(size, mtime)` still match the `ingest_files` bookkeeping row, parse
//! the rest from their stored byte offset and `INSERT OR IGNORE` the resulting
//! events. A file that shrank (or whose size no longer covers the stored
//! offset) is re-parsed from 0.

pub mod claude;
pub mod codex;

use crate::commands::providers;
use crate::commands::store::{now_ms, Db, IngestFile, UsageEvent};
use crate::model::IngestStats;
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Events are inserted in batches of this size to keep transactions short.
const BATCH: usize = 2_000;

/// A log root together with the provider it belongs to.
pub struct Root {
    pub provider: &'static str,
    pub path: PathBuf,
}

/// Every log directory that exists on this machine.
pub fn roots() -> Vec<Root> {
    let mut out = Vec::new();
    let mut push = |provider: &'static str, path: Option<PathBuf>| {
        if let Some(p) = path {
            if p.is_dir() {
                out.push(Root { provider, path: p });
            }
        }
    };
    push(providers::CLAUDE_ID, providers::claude::log_root());
    push(providers::CODEX_ID, providers::codex::log_root());
    push(providers::CODEX_ID, providers::codex::archived_log_root());
    out
}

/// Read `path` from `from_offset`, returning the text of the **complete**
/// lines it contains and the offset just past the last of them.
pub fn read_from_offset(path: &Path, from_offset: u64) -> Result<(String, u64)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if from_offset >= len {
        return Ok((String::new(), len.max(from_offset)));
    }
    file.seek(SeekFrom::Start(from_offset))?;
    let mut buf = Vec::with_capacity((len - from_offset) as usize);
    file.read_to_end(&mut buf)?;
    let end = buf
        .iter()
        .rposition(|b| *b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let text = String::from_utf8_lossy(&buf[..end]).into_owned();
    Ok((text, from_offset + end as u64))
}

/// Parse one file for `provider` starting at `from_offset`.
pub fn parse_file(provider: &str, path: &Path, from_offset: u64) -> Result<(Vec<UsageEvent>, u64)> {
    match provider {
        providers::CODEX_ID => codex::parse_file(path, from_offset),
        _ => claude::parse_file(path, from_offset),
    }
}

/// All `*.jsonl` files under `root` (recursively).
pub fn session_files(root: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .map(|e| e.into_path())
        .collect()
}

/// Run one ingestion pass over every known log root.
///
/// `full` forgets all byte offsets first, so everything is re-parsed (the
/// `UNIQUE(provider, request_id)` constraint keeps that idempotent).
pub fn run(db: &Db, full: bool) -> IngestStats {
    let started = Instant::now();
    let mut stats = IngestStats::default();
    if full {
        if let Err(e) = db.reset_ingest_offsets() {
            stats.errors.push(format!("reset ingest offsets: {e:#}"));
        }
    }
    for root in roots() {
        ingest_root(db, root.provider, &root.path, &mut stats);
    }
    stats.duration_ms = started.elapsed().as_millis() as u64;
    stats.running = false;
    stats
}

/// Ingest one directory tree, accumulating into `stats`.
pub fn ingest_root(db: &Db, provider: &str, root: &Path, stats: &mut IngestStats) {
    for path in session_files(root) {
        stats.files_scanned += 1;
        match ingest_one(db, provider, &path) {
            Ok(0) => {}
            Ok(n) => {
                stats.files_updated += 1;
                stats.events_added += n;
            }
            Err(e) => {
                let msg = format!("{}: {e:#}", path.display());
                log::debug!("ingest error {msg}");
                if stats.errors.len() < 20 {
                    stats.errors.push(msg);
                }
            }
        }
    }
}

/// Ingest a single file; returns the number of newly stored events.
///
/// A file whose size *and* mtime are unchanged since the last pass is skipped
/// without opening it.
pub fn ingest_one(db: &Db, provider: &str, path: &Path) -> Result<u64> {
    let meta = std::fs::metadata(path)?;
    let size = meta.len() as i64;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0);
    let key = path.display().to_string();
    let previous = db.get_ingest_file(&key)?;

    let mut offset = 0u64;
    if let Some(prev) = &previous {
        if prev.size == size && prev.mtime == mtime {
            return Ok(0);
        }
        // Only a growing file can be an append. A same-size rewrite or a
        // shrink can still be longer than the last complete line's offset.
        if size > prev.size && size >= prev.byte_offset && prev.byte_offset >= 0 {
            offset = prev.byte_offset as u64;
        }
    }

    let (events, next_offset) = parse_file(provider, path, offset)?;
    let mut added = 0u64;
    for chunk in events.chunks(BATCH) {
        added += crate::commands::store::insert_usage_events(db, chunk)?;
    }
    db.upsert_ingest_file(&IngestFile {
        path: key,
        provider: provider.to_string(),
        size,
        mtime,
        byte_offset: next_offset as i64,
        last_ingested_at: now_ms(),
    })?;
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::tempdir;

    #[test]
    fn incremental_pass_skips_unchanged_files_and_picks_up_appends() {
        let dir = tempdir();
        let db = Db::open_in_memory().unwrap();
        let path = dir.join("session.jsonl");
        let line = |id: &str, out: i64| {
            format!(
                r#"{{"requestId":"req_{id}","type":"assistant","timestamp":"2026-09-15T02:20:47.002Z","sessionId":"s","cwd":"/tmp","message":{{"id":"msg_{id}","model":"claude-opus-5","usage":{{"input_tokens":1,"cache_creation_input_tokens":2,"cache_read_input_tokens":3,"output_tokens":{out}}}}}}}"#
            )
        };
        std::fs::write(&path, format!("{}\n", line("A", 10))).unwrap();

        let mut stats = IngestStats::default();
        ingest_root(&db, providers::CLAUDE_ID, &dir, &mut stats);
        assert_eq!((stats.files_scanned, stats.events_added), (1, 1));

        // second pass, nothing changed
        let mut stats = IngestStats::default();
        ingest_root(&db, providers::CLAUDE_ID, &dir, &mut stats);
        assert_eq!(
            (stats.files_scanned, stats.files_updated, stats.events_added),
            (1, 0, 0)
        );

        // append a second request
        std::fs::write(&path, format!("{}\n{}\n", line("A", 10), line("B", 20))).unwrap();
        let mut stats = IngestStats::default();
        ingest_root(&db, providers::CLAUDE_ID, &dir, &mut stats);
        assert_eq!(stats.events_added, 1);
        assert_eq!(crate::commands::store::usage::count_events(&db).unwrap(), 2);

        // a full re-scan re-parses everything but stores no duplicates
        db.reset_ingest_offsets().unwrap();
        let mut stats = IngestStats::default();
        ingest_root(&db, providers::CLAUDE_ID, &dir, &mut stats);
        assert_eq!(
            stats.events_added, 0,
            "INSERT OR IGNORE keeps it idempotent"
        );
        assert_eq!(crate::commands::store::usage::count_events(&db).unwrap(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_shrinking_file_is_reparsed_from_zero() {
        let dir = tempdir();
        let db = Db::open_in_memory().unwrap();
        let path = dir.join("rollout-shrink.jsonl");
        let rec = |id: &str| {
            format!(
                r#"{{"timestamp":"2026-09-14T04:43:14.740Z","type":"token_usage_record","payload":{{"thread_id":"t","response_id":"{id}","usage":{{"input_tokens":10,"cached_input_tokens":4,"output_tokens":2,"total_tokens":12}}}}}}"#
            )
        };
        std::fs::write(&path, format!("{}\n{}\n", rec("r1"), rec("r2"))).unwrap();
        let mut stats = IngestStats::default();
        ingest_root(&db, providers::CODEX_ID, &dir, &mut stats);
        assert_eq!(stats.events_added, 2);

        std::fs::write(&path, format!("{}\n", rec("r3"))).unwrap();
        let mut stats = IngestStats::default();
        ingest_root(&db, providers::CODEX_ID, &dir, &mut stats);
        assert_eq!(stats.events_added, 1);
        assert_eq!(crate::commands::store::usage::count_events(&db).unwrap(), 3);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn same_size_rewrite_and_shrink_above_partial_offset_are_reparsed() {
        let dir = tempdir();
        let db = Db::open_in_memory().unwrap();
        let path = dir.join("rewrite.jsonl");
        let line = |id: &str| {
            format!(
                r#"{{"type":"token_usage_record","payload":{{"response_id":"{id}","usage":{{"input_tokens":5,"output_tokens":1}}}}}}"#
            )
        };
        let first = format!("{}\n", line("r1"));
        std::fs::write(&path, &first).unwrap();
        assert_eq!(ingest_one(&db, providers::CODEX_ID, &path).unwrap(), 1);
        // Force a distinct mtime in bookkeeping; no timing-sensitive sleep.
        let mut previous = db
            .get_ingest_file(&path.display().to_string())
            .unwrap()
            .unwrap();
        previous.mtime -= 1;
        db.upsert_ingest_file(&previous).unwrap();
        std::fs::write(&path, format!("{}\n", line("r2"))).unwrap();
        assert_eq!(ingest_one(&db, providers::CODEX_ID, &path).unwrap(), 1);

        std::fs::write(&path, format!("{first}{}", "x".repeat(500))).unwrap();
        ingest_one(&db, providers::CODEX_ID, &path).unwrap();
        std::fs::write(&path, format!("{}\n{}", line("r3"), "x".repeat(250))).unwrap();
        assert_eq!(ingest_one(&db, providers::CODEX_ID, &path).unwrap(), 1);
        assert_eq!(crate::commands::store::usage::count_events(&db).unwrap(), 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bookkeeping_preserves_submillisecond_modification_times() {
        let dir = tempdir();
        let db = Db::open_in_memory().unwrap();
        let path = dir.join("fast-rewrite.jsonl");
        let write = |id: &str, nanos| {
            std::fs::write(&path, format!(r#"{{"type":"token_usage_record","payload":{{"response_id":"{id}","usage":{{"input_tokens":1}}}}}}
"#)).unwrap();
            let modified = std::time::UNIX_EPOCH + std::time::Duration::new(1_789_430_400, nanos);
            std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(modified))
                .unwrap();
        };
        write("first", 100_000);
        assert_eq!(ingest_one(&db, providers::CODEX_ID, &path).unwrap(), 1);
        write("other", 900_000);
        assert_eq!(ingest_one(&db, providers::CODEX_ID, &path).unwrap(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
