//! Codex CLI session-log parser (`~/.codex/sessions/**/*.jsonl`). [BACKEND]
//!
//! Unlike Claude's transcripts a Codex rollout is **not** line-independent: the
//! model name comes from the newest `turn_context` before a record and the cwd
//! from the `session_meta` at the top of the file. The parser therefore always
//! walks the file from the start to rebuild that context, but only *emits*
//! records that begin at or after the stored byte offset — so re-ingesting an
//! append-only file stays cheap (a substring pre-filter keeps the walk to
//! memchr speed) and never produces duplicates.
//!
//! Newer CLIs write one `token_usage_record` per response (deduped on
//! `response_id`). Older ones only have `event_msg`/`token_count` with a
//! *cumulative* `info.total_token_usage`, so for those files we emit the
//! per-line delta under the synthetic id `<file-stem>:<line-no>`.

use crate::commands::providers::CODEX_ID;
use crate::commands::store::UsageEvent;
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
struct RawLine {
    #[serde(rename = "type")]
    kind: String,
    timestamp: Option<String>,
    payload: serde_json::Value,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(default)]
struct SessionMeta {
    session_id: Option<String>,
    id: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
struct TurnContext {
    model: Option<String>,
    cwd: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
struct UsageRecord {
    thread_id: Option<String>,
    session_id: Option<String>,
    response_id: Option<String>,
    turn_id: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize, Debug, Default, Clone, Copy)]
#[serde(default)]
pub struct Usage {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub cache_write_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
struct TokenCountPayload {
    #[serde(rename = "type")]
    kind: String,
    info: Option<TokenCountInfo>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
struct TokenCountInfo {
    total_token_usage: Option<Usage>,
    last_token_usage: Option<Usage>,
}

impl Usage {
    /// Codex `input_tokens` **includes** the cached ones; the DB stores
    /// non-cached input for both providers (ARCHITECTURE §2).
    fn to_event_fields(self) -> (i64, i64, i64, i64, i64, i64) {
        let cache_read = self.cached_input_tokens.clamp(0, self.input_tokens.max(0));
        let input = self.input_tokens.max(0) - cache_read;
        let total = if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.input_tokens
                .max(0)
                .saturating_add(self.output_tokens.max(0))
        };
        (
            input,
            self.cache_write_input_tokens.max(0),
            cache_read,
            self.output_tokens.max(0),
            self.reasoning_output_tokens.max(0),
            total.max(0),
        )
    }

    fn delta(self, prev: Usage) -> Usage {
        // A shrinking cumulative counter means the thread was reset/compacted;
        // the current value is then itself the delta.
        if self.total_tokens < prev.total_tokens
            || self.input_tokens < prev.input_tokens
            || self.output_tokens < prev.output_tokens
        {
            return self;
        }
        Usage {
            input_tokens: self.input_tokens.saturating_sub(prev.input_tokens),
            cached_input_tokens: self
                .cached_input_tokens
                .saturating_sub(prev.cached_input_tokens),
            cache_write_input_tokens: self
                .cache_write_input_tokens
                .saturating_sub(prev.cache_write_input_tokens),
            output_tokens: self.output_tokens.saturating_sub(prev.output_tokens),
            reasoning_output_tokens: self
                .reasoning_output_tokens
                .saturating_sub(prev.reasoning_output_tokens),
            total_tokens: self.total_tokens.saturating_sub(prev.total_tokens),
        }
    }

    fn is_empty(&self) -> bool {
        self.total_tokens <= 0
            && self.input_tokens <= 0
            && self.output_tokens <= 0
            && self.cached_input_tokens <= 0
    }
}

/// Parse a whole rollout file, emitting only records at/after `from_offset`.
///
/// `stem` is the file stem used to build the synthetic request ids of the
/// `token_count` fallback.
pub fn parse_text(text: &str, stem: &str, source: &str, from_offset: u64) -> Vec<UsageEvent> {
    // The fallback is only used for files that have no real usage records.
    let has_usage_record = text
        .lines()
        .filter(|line| line.contains("token_usage_record"))
        .any(|line| {
            serde_json::from_str::<RawLine>(line)
                .ok()
                .is_some_and(|raw| {
                    raw.kind == "token_usage_record"
                        && serde_json::from_value::<UsageRecord>(raw.payload)
                            .ok()
                            .is_some_and(|record| record.usage.is_some())
                })
        });
    let mut events = Vec::new();

    let mut meta = SessionMeta::default();
    let mut model: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut cumulative = Usage::default();

    let mut pos: u64 = 0;
    for (line_no, line) in text.split_inclusive('\n').enumerate() {
        let start = pos;
        pos += line.len() as u64;
        let line = line.trim_end_matches(['\n', '\r']);

        // Cheap pre-filter: only four line kinds are interesting.
        let interesting = line.contains("session_meta")
            || line.contains("turn_context")
            || line.contains("token_usage_record")
            || line.contains("token_count");
        if !interesting {
            continue;
        }
        let Ok(raw) = serde_json::from_str::<RawLine>(line) else {
            continue;
        };
        let ts = raw
            .timestamp
            .as_deref()
            .and_then(super::claude::parse_ts_ms)
            .unwrap_or(0);

        match raw.kind.as_str() {
            "session_meta" => {
                if let Ok(m) = serde_json::from_value::<SessionMeta>(raw.payload) {
                    cwd = m.cwd.clone().or(cwd);
                    if model.is_none() {
                        model = m.model.clone();
                    }
                    meta = m;
                }
            }
            "turn_context" => {
                if let Ok(t) = serde_json::from_value::<TurnContext>(raw.payload) {
                    if let Some(m) = t.model.filter(|s| !s.is_empty()) {
                        model = Some(m);
                    }
                    if let Some(c) = t.cwd.filter(|s| !s.is_empty()) {
                        cwd = Some(c);
                    }
                }
            }
            "token_usage_record" => {
                let Ok(rec) = serde_json::from_value::<UsageRecord>(raw.payload) else {
                    continue;
                };
                let Some(usage) = rec.usage else { continue };
                let request_id = rec
                    .response_id
                    .clone()
                    .filter(|s| !s.is_empty())
                    .or_else(|| rec.turn_id.clone().map(|t| format!("{stem}:{t}")))
                    .unwrap_or_else(|| format!("{stem}:{line_no}"));
                if start < from_offset {
                    continue;
                }
                events.push(make_event(
                    usage,
                    request_id,
                    ts,
                    rec.thread_id
                        .or(rec.session_id)
                        .or_else(|| session_id(&meta)),
                    model.clone(),
                    cwd.clone(),
                    source,
                ));
            }
            "event_msg" => {
                let Ok(payload) = serde_json::from_value::<TokenCountPayload>(raw.payload) else {
                    continue;
                };
                if payload.kind != "token_count" {
                    continue;
                }
                let Some(info) = payload.info else {
                    continue;
                };
                let delta = if let Some(total) = info.total_token_usage {
                    let delta = total.delta(cumulative);
                    cumulative = total;
                    delta
                } else if let Some(last) = info.last_token_usage {
                    last
                } else {
                    continue;
                };
                if has_usage_record {
                    continue;
                }
                if delta.is_empty() || start < from_offset {
                    continue;
                }
                events.push(make_event(
                    delta,
                    format!("{stem}:{line_no}"),
                    ts,
                    session_id(&meta),
                    model.clone(),
                    cwd.clone(),
                    source,
                ));
            }
            _ => {}
        }
    }
    events
}

fn session_id(meta: &SessionMeta) -> Option<String> {
    meta.session_id.clone().or_else(|| meta.id.clone())
}

#[allow(clippy::too_many_arguments)]
fn make_event(
    usage: Usage,
    request_id: String,
    ts: i64,
    session_id: Option<String>,
    model: Option<String>,
    cwd: Option<String>,
    source: &str,
) -> UsageEvent {
    let (input, cache_write, cache_read, output, reasoning, total) = usage.to_event_fields();
    UsageEvent {
        provider: CODEX_ID.to_string(),
        model: model.unwrap_or_else(|| "unknown".to_string()),
        ts,
        input_tokens: input,
        cache_write_tokens: cache_write,
        cache_read_tokens: cache_read,
        output_tokens: output,
        reasoning_tokens: reasoning,
        total_tokens: total,
        session_id,
        request_id,
        cwd,
        source_file: Some(source.to_string()),
    }
}

/// Parse `path`, emitting only the records appended since `from_offset`.
pub fn parse_file(path: &Path, from_offset: u64) -> Result<(Vec<UsageEvent>, u64)> {
    let (text, next) = super::read_from_offset(path, 0)?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("rollout")
        .to_string();
    let source = path.display().to_string();
    let events = parse_text(&text, &stem, &source, from_offset);
    Ok((events, next))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROLLOUT: &str = concat!(
        r#"{"timestamp":"2026-09-14T04:43:04.796Z","ordinal":0,"type":"session_meta","payload":{"session_id":"thread-1","id":"thread-1","cwd":"/home/me/proj","originator":"codex-tui","cli_version":"0.154.0","source":"cli"}}"#,
        "\n",
        r#"{"timestamp":"2026-09-14T04:43:05.723Z","ordinal":7,"type":"turn_context","payload":{"turn_id":"t1","cwd":"/home/me/proj","model":"gpt-6-astra","effort":"medium"}}"#,
        "\n",
        r#"{"timestamp":"2026-09-14T04:43:14.740Z","ordinal":13,"type":"token_usage_record","payload":{"thread_id":"thread-1","turn_id":"t1","response_id":"resp_A","usage":{"input_tokens":14149,"cached_input_tokens":11904,"cache_write_input_tokens":0,"output_tokens":193,"reasoning_output_tokens":12,"total_tokens":14342}}}"#,
        "\n",
        r#"{"timestamp":"2026-09-14T04:43:15.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":14149,"cached_input_tokens":11904,"output_tokens":193,"total_tokens":14342}},"rate_limits":{"primary":{"used_percent":75.0,"window_minutes":10080,"resets_at":1789807722}}}}"#,
        "\n",
        r#"{"timestamp":"2026-09-14T04:44:00.000Z","type":"turn_context","payload":{"turn_id":"t2","model":"gpt-5.3-codex"}}"#,
        "\n",
        r#"{"timestamp":"2026-09-14T04:44:10.000Z","type":"token_usage_record","payload":{"thread_id":"thread-1","turn_id":"t2","response_id":"resp_B","usage":{"input_tokens":200,"cached_input_tokens":50,"cache_write_input_tokens":10,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":220}}}"#,
        "\n",
    );

    /// Old CLI: no `token_usage_record`, only cumulative `token_count` lines.
    const LEGACY: &str = concat!(
        r#"{"timestamp":"2026-09-10T10:00:00.000Z","type":"session_meta","payload":{"session_id":"thread-9","cwd":"/tmp/x","model":"gpt-5.1-codex"}}"#,
        "\n",
        r#"{"timestamp":"2026-09-10T10:00:10.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":200,"output_tokens":100,"reasoning_output_tokens":10,"total_tokens":1100}}}}"#,
        "\n",
        r#"{"timestamp":"2026-09-10T10:00:20.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":2500,"cached_input_tokens":900,"output_tokens":250,"reasoning_output_tokens":30,"total_tokens":2750}}}}"#,
        "\n",
        r#"{"timestamp":"2026-09-10T10:00:30.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":2500,"cached_input_tokens":900,"output_tokens":250,"reasoning_output_tokens":30,"total_tokens":2750}}}}"#,
        "\n",
    );

    #[test]
    fn token_usage_records_win_and_track_the_latest_model() {
        let e = parse_text(ROLLOUT, "rollout-x", "rollout-x.jsonl", 0);
        assert_eq!(e.len(), 2, "the token_count line is ignored for new files");

        assert_eq!(e[0].request_id, "resp_A");
        assert_eq!(e[0].model, "gpt-6-astra");
        assert_eq!(e[0].input_tokens, 14149 - 11904);
        assert_eq!(e[0].cache_read_tokens, 11904);
        assert_eq!(e[0].cache_write_tokens, 0);
        assert_eq!(e[0].output_tokens, 193);
        assert_eq!(e[0].reasoning_tokens, 12);
        assert_eq!(e[0].total_tokens, 14342);
        assert_eq!(e[0].session_id.as_deref(), Some("thread-1"));
        assert_eq!(e[0].cwd.as_deref(), Some("/home/me/proj"));

        assert_eq!(e[1].request_id, "resp_B");
        assert_eq!(e[1].model, "gpt-5.3-codex", "latest turn_context wins");
        assert_eq!(e[1].input_tokens, 150);
        assert_eq!(e[1].cache_write_tokens, 10);
    }

    #[test]
    fn token_count_deltas_are_the_fallback_for_old_files() {
        let e = parse_text(LEGACY, "rollout-legacy", "rollout-legacy.jsonl", 0);
        assert_eq!(e.len(), 2, "the unchanged third line yields an empty delta");

        assert_eq!(e[0].request_id, "rollout-legacy:1");
        assert_eq!(
            e[0].model, "gpt-5.1-codex",
            "session_meta model is the fallback"
        );
        assert_eq!(e[0].input_tokens, 800);
        assert_eq!(e[0].cache_read_tokens, 200);
        assert_eq!(e[0].total_tokens, 1100);

        assert_eq!(e[1].request_id, "rollout-legacy:2");
        assert_eq!(e[1].input_tokens, (2500 - 1000) - (900 - 200));
        assert_eq!(e[1].cache_read_tokens, 700);
        assert_eq!(e[1].output_tokens, 150);
        assert_eq!(e[1].reasoning_tokens, 20);
        assert_eq!(e[1].total_tokens, 1650);
    }

    #[test]
    fn offsets_skip_already_ingested_records_but_keep_the_context() {
        let second_record_start = ROLLOUT
            .find(r#"{"timestamp":"2026-09-14T04:44:10"#)
            .unwrap() as u64;
        let e = parse_text(ROLLOUT, "rollout-x", "rollout-x.jsonl", second_record_start);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].request_id, "resp_B");
        assert_eq!(
            e[0].model, "gpt-5.3-codex",
            "context is rebuilt from the head"
        );
        assert_eq!(e[0].cwd.as_deref(), Some("/home/me/proj"));
    }

    #[test]
    fn missing_model_falls_back_to_unknown_and_garbage_is_skipped() {
        let text = concat!(
            "not json\n",
            r#"{"type":"token_usage_record","payload":{"response_id":"r1","usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}}}"#,
            "\n",
        );
        let e = parse_text(text, "stem", "stem.jsonl", 0);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].model, "unknown");
        assert_eq!(e[0].total_tokens, 12);
        assert!(parse_text("", "s", "s", 0).is_empty());
    }

    #[test]
    fn parse_file_reports_a_line_aligned_offset() {
        let dir = crate::commands::test_support::tempdir();
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, ROLLOUT).unwrap();
        let (e, off) = parse_file(&path, 0).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(off, ROLLOUT.len() as u64);
        let (e2, off2) = parse_file(&path, off).unwrap();
        assert!(e2.is_empty());
        assert_eq!(off2, off);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn incomplete_record_and_partial_utf8_are_retried_after_append() {
        let dir = crate::commands::test_support::tempdir();
        let path = dir.join("partial.jsonl");
        let record = r#"{"type":"token_usage_record","payload":{"response_id":"partial","usage":{"input_tokens":3,"output_tokens":2}}}"#;
        std::fs::write(&path, format!("{LEGACY}{record}")).unwrap();
        let (events, offset) = parse_file(&path, 0).unwrap();
        assert_eq!(
            events.len(),
            2,
            "unfinished new-format record must not suppress legacy usage"
        );
        assert_eq!(offset, LEGACY.len() as u64);
        std::fs::write(&path, format!("{LEGACY}{record}\n")).unwrap();
        let (events, offset) = parse_file(&path, offset).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].request_id, "partial");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(&[0xe4, 0xb8]);
        std::fs::write(&path, bytes).unwrap();
        assert!(parse_file(&path, offset).unwrap().0.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn record_names_in_message_text_do_not_suppress_legacy_counts() {
        let text = format!("{LEGACY}{{\"type\":\"response_item\",\"payload\":{{\"text\":\"token_usage_record\"}}}}\n");
        assert_eq!(parse_text(&text, "legacy", "legacy.jsonl", 0).len(), 2);
        let last_only = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"cached_input_tokens":2,"output_tokens":3}}}}"#;
        let events = parse_text(last_only, "old", "old.jsonl", 0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].total_tokens, 13);
        assert_eq!(events[0].input_tokens, 8);
    }
}
