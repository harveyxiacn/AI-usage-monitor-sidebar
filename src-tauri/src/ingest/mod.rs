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
const READ_CHUNK_BYTES: usize = 4 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

#[cfg(test)]
thread_local! { static USAGE_BYTES_READ: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }

#[derive(serde::Serialize, serde::Deserialize)]
struct FileCheckpoint {
    version: u8,
    offset: u64,
    head: u64,
    tail: u64,
    created: Option<u128>,
    codex: Option<codex::Checkpoint>,
}

/// Read bounded complete-line chunks. Oversized transcript bodies are skipped
/// without allocating their full content; a newline preserves parser line IDs.
/// The last partial line always remains pending, including partial UTF-8.
fn read_chunk(path: &Path, offset: u64) -> Result<(String, u64)> {
    use std::io::{BufRead, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
    let mut text = Vec::new();
    let mut line = Vec::new();
    let mut consumed = offset;
    let mut complete = offset;
    let mut oversized = false;
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            break;
        }
        let newline = bytes.iter().position(|b| *b == b'\n');
        let take = newline.map_or(bytes.len(), |p| p + 1);
        if !oversized {
            if line.len() + take <= MAX_LINE_BYTES {
                line.extend_from_slice(&bytes[..take]);
            } else {
                line.clear();
                oversized = true;
            }
        }
        reader.consume(take);
        #[cfg(test)]
        USAGE_BYTES_READ.with(|n| n.set(n.get() + take as u64));
        consumed += take as u64;
        if newline.is_some() {
            if oversized {
                text.push(b'\n');
            } else {
                text.extend_from_slice(&line);
            }
            line.clear();
            oversized = false;
            complete = consumed;
            if complete - offset >= READ_CHUNK_BYTES as u64 {
                break;
            }
        }
    }
    Ok((String::from_utf8_lossy(&text).into_owned(), complete))
}

/// Constant-size anchors catch common replacements that also happen to grow.
/// They never read more than 512 bytes of the already consumed prefix.
fn anchors(path: &Path, offset: u64) -> Result<(u64, u64)> {
    use std::hash::{Hash, Hasher};
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let count = offset.min(256) as usize;
    let mut sample = vec![0; count];
    file.read_exact(&mut sample)?;
    let mut head = std::collections::hash_map::DefaultHasher::new();
    sample.hash(&mut head);
    file.seek(SeekFrom::Start(offset - count as u64))?;
    file.read_exact(&mut sample)?;
    let mut tail = std::collections::hash_map::DefaultHasher::new();
    sample.hash(&mut tail);
    Ok((head.finish(), tail.finish()))
}

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
        if root.provider == providers::CODEX_ID {
            if let Some(parent) = root.path.parent() {
                let index = parent.join("session_index.jsonl");
                if index.is_file() {
                    if let Err(e) = crate::sessions::index_codex_titles(db, &index) {
                        log::debug!("session title index: {e:#}");
                    }
                }
            }
        }
        ingest_root(db, root.provider, &root.path, &mut stats);
    }
    stats.duration_ms = started.elapsed().as_millis() as u64;
    stats.running = false;
    stats
}

/// A watcher batch never walks unrelated session directories. The periodic
/// reconciliation remains responsible for missed notifications and new roots.
pub fn run_paths(db: &Db, paths: &[(String, PathBuf)]) -> IngestStats {
    let started = Instant::now();
    let mut stats = IngestStats::default();
    for (provider, path) in paths {
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        stats.files_scanned += 1;
        match ingest_one(db, provider, path) {
            Ok(n) => {
                stats.events_added += n;
                stats.files_updated += u64::from(n > 0);
            }
            Err(e) if stats.errors.len() < 20 => {
                stats.errors.push(format!("{}: {e:#}", path.display()))
            }
            Err(_) => {}
        }
    }
    stats.duration_ms = started.elapsed().as_millis() as u64;
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
    // The metadata index has its own checkpoint, so an existing usage-only
    // database gets session backfill even when all usage files are unchanged.
    if let Err(error) = crate::sessions::index_file(db, provider, path) {
        // Analysis indexing must not make the existing quota history unavailable.
        log::warn!(
            "session metadata index failed for {}: {error:#}",
            path.display()
        );
    }
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
    let saved = db
        .ingest_checkpoint(&key)?
        .and_then(|json| serde_json::from_str::<FileCheckpoint>(&json).ok())
        .filter(|c| {
            c.version == 1
                && (provider != providers::CODEX_ID
                    || c.codex.as_ref().is_some_and(|p| p.offset == c.offset))
        });
    let created = meta
        .created()
        .ok()
        .and_then(|v| v.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|v| v.as_nanos());
    let mut offset = 0u64;
    let mut parser = codex::Checkpoint::default();
    if let Some(prev) = &previous {
        if prev.size == size && prev.mtime == mtime && saved.is_some() {
            return Ok(0);
        }
        // Only a growing file can be an append. A same-size rewrite or a
        // shrink can still be longer than the last complete line's offset.
        if size > prev.size && size >= prev.byte_offset && prev.byte_offset >= 0 {
            if let Some(checkpoint) =
                saved.filter(|c| c.offset == prev.byte_offset as u64 && c.created == created)
            {
                if anchors(path, checkpoint.offset)? == (checkpoint.head, checkpoint.tail) {
                    offset = checkpoint.offset;
                    parser = checkpoint.codex.unwrap_or_default();
                }
            }
        }
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("rollout");
    let mut added = 0u64;
    loop {
        let (text, next) = read_chunk(path, offset)?;
        if next == offset {
            break;
        }
        let events = if provider == providers::CODEX_ID {
            let chunk = codex::parse_chunk(&text, stem, &key, offset, &mut parser);
            // Even old DBs with no saved mode converge during the one-time replay.
            db.reconcile_codex_legacy(&key, &chunk.legacy_ids, parser.modern)?;
            parser.offset = next;
            chunk.events
        } else {
            claude::parse_chunk(&text, &key)
        };
        for chunk in events.chunks(BATCH) {
            added += crate::commands::store::insert_usage_events(db, chunk)?;
        }
        offset = next;
    }
    let (head, tail) = anchors(path, offset)?;
    db.save_ingest_checkpoint(
        &key,
        &serde_json::to_string(&FileCheckpoint {
            version: 1,
            offset,
            head,
            tail,
            created,
            codex: (provider == providers::CODEX_ID).then_some(parser),
        })?,
    )?;
    db.upsert_ingest_file(&IngestFile {
        path: key,
        provider: provider.to_string(),
        size,
        mtime,
        byte_offset: offset as i64,
        last_ingested_at: now_ms(),
    })?;
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::tempdir;

    fn codex_record(id: &str) -> String {
        format!("{{\"timestamp\":\"2026-09-15T12:00:00Z\",\"type\":\"token_usage_record\",\"payload\":{{\"response_id\":\"{id}\",\"usage\":{{\"input_tokens\":10,\"output_tokens\":2}}}}}}\n")
    }

    #[test]
    fn checkpoint_survives_restart_and_reads_only_appended_bytes() {
        use std::io::Write;
        let dir = tempdir();
        let path = dir.join("incremental.jsonl");
        let database = dir.join("usage.db");
        let context = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session\",\"cwd\":\"/project\",\"model\":\"exact-model\"}}\n";
        let prefix = format!(
            "{context}{}{}",
            "{\"type\":\"irrelevant\"}\n".repeat(100_000),
            codex_record("first")
        );
        std::fs::write(&path, &prefix).unwrap();
        let db = Db::open(&database).unwrap();
        ingest_one(&db, providers::CODEX_ID, &path).unwrap();
        drop(db);
        let appended = codex_record("next");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(appended.as_bytes())
            .unwrap();
        let db = Db::open(&database).unwrap();
        USAGE_BYTES_READ.with(|v| v.set(0));
        assert_eq!(ingest_one(&db, providers::CODEX_ID, &path).unwrap(), 1);
        assert_eq!(USAGE_BYTES_READ.with(|v| v.get()), appended.len() as u64);
        let fields: (String, String) = db
            .lock()
            .query_row(
                "SELECT model,cwd FROM usage_events WHERE request_id='next'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(fields, ("exact-model".into(), "/project".into()));
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn growing_rewrite_resets_context_and_partial_line_waits_for_newline() {
        use std::io::Write;
        let dir = tempdir();
        let path = dir.join("rewrite-grow.jsonl");
        let db = Db::open_in_memory().unwrap();
        std::fs::write(&path, codex_record("first")).unwrap();
        ingest_one(&db, providers::CODEX_ID, &path).unwrap();
        std::fs::write(
            &path,
            format!(
                "{}{}",
                codex_record("other"),
                codex_record("partial").trim_end()
            ),
        )
        .unwrap();
        assert_eq!(ingest_one(&db, providers::CODEX_ID, &path).unwrap(), 1);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        assert_eq!(ingest_one(&db, providers::CODEX_ID, &path).unwrap(), 1);
        assert_eq!(crate::commands::store::usage::count_events(&db).unwrap(), 3);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_markers_cannot_delete_modern_synthetic_ids_after_rewrite() {
        use std::io::Write;
        let dir = tempdir();
        let path = dir.join("synthetic.jsonl");
        let db = Db::open_in_memory().unwrap();
        std::fs::write(&path, "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":1}}}}\n").unwrap();
        ingest_one(&db, providers::CODEX_ID, &path).unwrap();
        std::fs::write(
            &path,
            "{\"type\":\"token_usage_record\",\"payload\":{\"usage\":{\"input_tokens\":5}}}\n",
        )
        .unwrap();
        ingest_one(&db, providers::CODEX_ID, &path).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(codex_record("next").as_bytes())
            .unwrap();
        ingest_one(&db, providers::CODEX_ID, &path).unwrap();
        assert_eq!(crate::commands::store::usage::count_events(&db).unwrap(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bounded_chunks_skip_oversized_bodies_but_preserve_line_ids() {
        let dir = tempdir();
        let path = dir.join("large.jsonl");
        std::fs::write(
            &path,
            format!(
                "{}\n{}",
                "x".repeat(MAX_LINE_BYTES + 100),
                codex_record("valid")
            ),
        )
        .unwrap();
        let (first, next) = read_chunk(&path, 0).unwrap();
        assert_eq!(first, "\n");
        assert_eq!(next, (MAX_LINE_BYTES + 101) as u64);
        let (second, end) = read_chunk(&path, next).unwrap();
        assert_eq!(second, codex_record("valid"));
        assert_eq!(end, std::fs::metadata(&path).unwrap().len());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn watcher_batch_visits_only_selected_files() {
        let dir = tempdir();
        let db = Db::open_in_memory().unwrap();
        for i in 0..100 {
            std::fs::write(dir.join(format!("{i}.jsonl")), codex_record(&i.to_string())).unwrap();
        }
        let stats = run_paths(&db, &[(providers::CODEX_ID.into(), dir.join("42.jsonl"))]);
        assert_eq!((stats.files_scanned, stats.events_added), (1, 1));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn session_metadata_failure_does_not_block_existing_token_history() {
        let dir = tempdir();
        let path = dir.join("independent.jsonl");
        let db = Db::open_in_memory().unwrap();
        db.lock().execute_batch("CREATE TRIGGER refuse_metadata BEFORE INSERT ON session_metadata BEGIN SELECT RAISE(ABORT, 'synthetic metadata failure'); END;").unwrap();
        std::fs::write(
            &path,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"test\"}}}}\n{}",
                codex_record("valid")
            ),
        )
        .unwrap();
        assert_eq!(ingest_one(&db, providers::CODEX_ID, &path).unwrap(), 1);
        assert_eq!(crate::commands::store::usage::count_events(&db).unwrap(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Run with `cargo test synthetic_ingestion_benchmark --lib -- --ignored --nocapture`.
    #[test]
    #[ignore = "repeatable synthetic performance measurement"]
    fn synthetic_ingestion_benchmark() {
        use std::io::Write;
        let dir = tempdir();
        let path = dir.join("benchmark.jsonl");
        let db = Db::open_in_memory().unwrap();
        let filler = format!(
            "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"{}\"}}]}}}}\n",
            "x".repeat(2000)
        );
        let context = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"synthetic-benchmark\",\"cwd\":\"/synthetic\",\"model\":\"synthetic-model\"}}\n";
        std::fs::write(
            &path,
            format!(
                "{context}{}{}",
                filler.repeat(10_000),
                codex_record("first")
            ),
        )
        .unwrap();
        ingest_one(&db, providers::CODEX_ID, &path).unwrap();
        let rounds = 20;
        let mut old_bytes = 0;
        let mut old_duration = std::time::Duration::ZERO;
        let mut new_duration = std::time::Duration::ZERO;
        let mut prior = std::fs::metadata(&path).unwrap().len();
        USAGE_BYTES_READ.with(|v| v.set(0));
        for i in 0..rounds {
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap()
                .write_all(codex_record(&format!("append-{i}")).as_bytes())
                .unwrap();
            let before = Instant::now();
            let (events, next) = codex::parse_file(&path, prior).unwrap();
            old_duration += before.elapsed();
            old_bytes += next;
            prior = next;
            assert_eq!(events.len(), 1);
            let before = Instant::now();
            assert_eq!(ingest_one(&db, providers::CODEX_ID, &path).unwrap(), 1);
            new_duration += before.elapsed();
        }
        let new_bytes = USAGE_BYTES_READ.with(|v| v.get());
        assert!(new_bytes * 1000 < old_bytes);
        println!("INGEST_BENCH rounds={rounds} baseline_usage_bytes={old_bytes} checkpoint_usage_bytes={new_bytes} anchor_bytes_max={} baseline_parser_ms={:.3} incremental_with_metadata_and_sql_ms={:.3}", rounds * 1024, old_duration.as_secs_f64() * 1000., new_duration.as_secs_f64() * 1000.);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_late_modern_record_replaces_previously_ingested_legacy_counts() {
        use std::io::Write;
        let dir = tempdir();
        let path = dir.join("mixed.jsonl");
        let database = dir.join("usage.db");
        let legacy = "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":10,\"output_tokens\":2}}}}\n";
        std::fs::write(&path, legacy).unwrap();
        let db = Db::open(&database).unwrap();
        assert_eq!(ingest_one(&db, providers::CODEX_ID, &path).unwrap(), 1);
        drop(db);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"{\"type\":\"token_usage_record\",\"payload\":{\"response_id\":\"real\",\"usage\":{\"input_tokens\":10,\"output_tokens\":2}}}}\n").unwrap();
        drop(file);
        let db = Db::open(&database).unwrap();
        ingest_one(&db, providers::CODEX_ID, &path).unwrap();
        let totals: (i64, i64) = db
            .lock()
            .query_row(
                "SELECT COUNT(*), SUM(total_tokens) FROM usage_events",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            totals,
            (1, 12),
            "modern records must replace historical fallback rows, including after restart"
        );
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

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
