//! Claude Code session-log parser (`~/.claude/projects/**/*.jsonl`). [BACKEND]
//!
//! Every interesting line is self-contained (model, ids, cwd, timestamp), so
//! Claude files can be parsed **truly incrementally**: we seek to the stored
//! byte offset and only look at the bytes that were appended since.
//!
//! Streaming writes several `assistant` lines for one `message.id`; the last
//! one carries the complete usage, so within a chunk we dedupe on
//! `<message.id>:<requestId>` keeping the **last** occurrence, and across
//! chunks `store::insert_usage_events` upgrades a row when a later line has
//! more tokens.

use crate::commands::providers::CLAUDE_ID;
use crate::commands::store::UsageEvent;
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Model name Claude Code uses for internal (unbilled) requests.
pub const SYNTHETIC_MODEL: &str = "<synthetic>";

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
struct Line {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    uuid: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
struct Message {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
struct Usage {
    input_tokens: i64,
    cache_creation_input_tokens: i64,
    cache_read_input_tokens: i64,
    output_tokens: i64,
    output_tokens_details: Option<OutputDetails>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
struct OutputDetails {
    thinking_tokens: i64,
}

/// Parse a whole chunk of JSONL text. `source` is stored on every event.
pub fn parse_chunk(text: &str, source: &str) -> Vec<UsageEvent> {
    let mut order: Vec<String> = Vec::new();
    let mut by_key: HashMap<String, UsageEvent> = HashMap::new();

    for line in text.lines() {
        // Cheap pre-filter — JSON-parsing every line of a large transcript is
        // an order of magnitude more expensive than these substring checks.
        if !line.contains("\"usage\"") || !line.contains("\"assistant\"") {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<Line>(line) else {
            continue;
        };
        if parsed.kind != "assistant" {
            continue;
        }
        let Some(message) = parsed.message else {
            continue;
        };
        let Some(usage) = message.usage else { continue };
        let model = message.model.unwrap_or_default();
        if model.is_empty() || model == SYNTHETIC_MODEL {
            continue;
        }
        // The dedupe key must stay stable across re-ingestion, so fall back to
        // the line uuid only when the API ids are missing.
        let id = message
            .id
            .filter(|s| !s.is_empty())
            .or_else(|| parsed.uuid.clone())
            .unwrap_or_default();
        let request_id = parsed.request_id.clone().unwrap_or_default();
        if id.is_empty() && request_id.is_empty() {
            continue;
        }
        let key = format!("{id}:{request_id}");
        let ts = parsed
            .timestamp
            .as_deref()
            .and_then(parse_ts_ms)
            .unwrap_or(0);
        let event = UsageEvent {
            provider: CLAUDE_ID.to_string(),
            model,
            ts,
            input_tokens: usage.input_tokens.max(0),
            cache_write_tokens: usage.cache_creation_input_tokens.max(0),
            cache_read_tokens: usage.cache_read_input_tokens.max(0),
            output_tokens: usage.output_tokens.max(0),
            reasoning_tokens: usage
                .output_tokens_details
                .map(|d| d.thinking_tokens.max(0))
                .unwrap_or(0),
            total_tokens: usage.input_tokens.max(0)
                + usage.cache_creation_input_tokens.max(0)
                + usage.cache_read_input_tokens.max(0)
                + usage.output_tokens.max(0),
            session_id: parsed.session_id.clone(),
            request_id: key.clone(),
            cwd: parsed.cwd.clone(),
            source_file: Some(source.to_string()),
        };
        if by_key.insert(key.clone(), event).is_none() {
            order.push(key);
        }
    }

    order
        .into_iter()
        .filter_map(|k| by_key.remove(&k))
        .collect()
}

/// Parse the bytes of `path` starting at `from_offset`.
///
/// Returns the events and the new offset, which never points into the middle
/// of a line (a partially written last line is picked up next time).
pub fn parse_file(path: &Path, from_offset: u64) -> Result<(Vec<UsageEvent>, u64)> {
    let (text, next_offset) = super::read_from_offset(path, from_offset)?;
    let source = path.display().to_string();
    Ok((parse_chunk(&text, &source), next_offset))
}

/// ISO-8601 / RFC 3339 timestamp → unix milliseconds.
pub fn parse_ts_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three streaming lines for one message id (the last is complete), one
    /// synthetic line, one unrelated line and one broken line.
    const FIXTURE: &str = r#"
{"parentUuid":null,"isSidechain":false,"type":"user","message":{"role":"user"},"timestamp":"2026-09-15T02:20:40.000Z"}
{"requestId":"req_A","type":"assistant","uuid":"u1","timestamp":"2026-09-15T02:20:47.002Z","cwd":"/home/me","sessionId":"sess-1","message":{"id":"msg_A","model":"claude-opus-5","role":"assistant","usage":{"input_tokens":2,"cache_creation_input_tokens":5748,"cache_read_input_tokens":22410,"output_tokens":1,"service_tier":"standard"}}}
{"requestId":"req_A","type":"assistant","uuid":"u2","timestamp":"2026-09-15T02:20:47.604Z","cwd":"/home/me","sessionId":"sess-1","message":{"id":"msg_A","model":"claude-opus-5","role":"assistant","usage":{"input_tokens":2,"cache_creation_input_tokens":5748,"cache_read_input_tokens":22410,"output_tokens":120}}}
{"requestId":"req_A","type":"assistant","uuid":"u3","timestamp":"2026-09-15T02:20:49.579Z","cwd":"/home/me","sessionId":"sess-1","message":{"id":"msg_A","model":"claude-opus-5","role":"assistant","usage":{"input_tokens":2,"cache_creation_input_tokens":5748,"cache_read_input_tokens":22410,"output_tokens":281,"output_tokens_details":{"thinking_tokens":64}}}}
{"requestId":"req_B","type":"assistant","uuid":"u4","timestamp":"2026-09-15T02:21:00.000Z","cwd":"/home/me","sessionId":"sess-1","message":{"id":"msg_B","model":"<synthetic>","role":"assistant","usage":{"input_tokens":5,"output_tokens":5}}}
{"requestId":"req_C","type":"assistant","uuid":"u5","timestamp":"2026-09-15T02:21:10.000Z","cwd":"/home/me","sessionId":"sess-1","message":{"id":"msg_C","model":"claude-haiku-4-5","role":"assistant","usage":{"input_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":7}}}
{"type":"assistant","message":{"usage":{ broken json
"#;

    #[test]
    fn streaming_duplicates_collapse_to_the_last_line() {
        let events = parse_chunk(FIXTURE, "fixture.jsonl");
        assert_eq!(
            events.len(),
            2,
            "synthetic + non-assistant lines are skipped"
        );

        let a = &events[0];
        assert_eq!(a.request_id, "msg_A:req_A");
        assert_eq!(a.model, "claude-opus-5");
        assert_eq!(a.input_tokens, 2);
        assert_eq!(a.cache_write_tokens, 5748);
        assert_eq!(a.cache_read_tokens, 22410);
        assert_eq!(a.output_tokens, 281, "the last streaming line wins");
        assert_eq!(a.reasoning_tokens, 64);
        assert_eq!(a.total_tokens, 2 + 5748 + 22410 + 281);
        assert_eq!(a.session_id.as_deref(), Some("sess-1"));
        assert_eq!(a.cwd.as_deref(), Some("/home/me"));
        assert_eq!(a.ts, parse_ts_ms("2026-09-15T02:20:49.579Z").unwrap());

        assert_eq!(events[1].request_id, "msg_C:req_C");
        assert_eq!(events[1].total_tokens, 17);
    }

    #[test]
    fn incremental_offsets_only_return_new_lines() {
        let dir = crate::commands::test_support::tempdir();
        let path = dir.join("session.jsonl");
        let first = FIXTURE.trim();
        std::fs::write(&path, first).unwrap();
        let (events, offset) = parse_file(&path, 0).unwrap();
        assert_eq!(events.len(), 2);
        // the broken last line has no trailing newline yet → not consumed
        assert!(offset < first.len() as u64);

        let mut appended = std::fs::read_to_string(&path).unwrap();
        appended.push_str("\n{\"requestId\":\"req_D\",\"type\":\"assistant\",\"timestamp\":\"2026-09-15T02:22:00.000Z\",\"message\":{\"id\":\"msg_D\",\"model\":\"claude-sonnet-5\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n");
        std::fs::write(&path, &appended).unwrap();
        let (events2, offset2) = parse_file(&path, offset).unwrap();
        assert_eq!(events2.len(), 1);
        assert_eq!(events2[0].request_id, "msg_D:req_D");
        assert_eq!(offset2, appended.len() as u64);

        // nothing new → no events, same offset
        let (events3, offset3) = parse_file(&path, offset2).unwrap();
        assert!(events3.is_empty());
        assert_eq!(offset3, offset2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn garbage_never_panics() {
        assert!(parse_chunk("", "x").is_empty());
        assert!(parse_chunk("not json\n{\"usage\":\"assistant\"}\n", "x").is_empty());
    }
}
