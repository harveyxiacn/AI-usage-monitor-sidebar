//! Claude Code session-log parser (`~/.claude/projects/**/*.jsonl`). [BACKEND]
//!
//! Every interesting line is self-contained (model, ids, cwd, timestamp), so
//! Claude files can be parsed **truly incrementally**: we seek to the stored
//! byte offset and only look at the bytes that were appended since.
//!
//! Streaming writes several `assistant` lines for one `message.id`; the last
//! one carries the complete usage, so within a chunk we dedupe on
//! `<message.id>:<requestId>` keeping the most complete occurrence, and across
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
    /// Explicit request metadata emitted by Claude Code, outside `message`.
    effort: Option<serde_json::Value>,
    #[serde(rename = "perTurnEffort")]
    per_turn_effort: Option<serde_json::Value>,
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
        let mut event = UsageEvent {
            provider: CLAUDE_ID.to_string(),
            model,
            reasoning_effort: explicit_effort(parsed.per_turn_effort.as_ref())
                .or_else(|| explicit_effort(parsed.effort.as_ref())),
            ts,
            input_tokens: usage.input_tokens.max(0),
            cache_write_tokens: usage.cache_creation_input_tokens.max(0),
            cache_read_tokens: usage.cache_read_input_tokens.max(0),
            output_tokens: usage.output_tokens.max(0),
            reasoning_tokens: usage
                .output_tokens_details
                .map(|d| d.thinking_tokens.max(0))
                .unwrap_or(0),
            total_tokens: usage
                .input_tokens
                .max(0)
                .saturating_add(usage.cache_creation_input_tokens.max(0))
                .saturating_add(usage.cache_read_input_tokens.max(0))
                .saturating_add(usage.output_tokens.max(0)),
            session_id: parsed.session_id.clone(),
            request_id: key.clone(),
            cwd: parsed.cwd.clone(),
            source_file: Some(source.to_string()),
        };
        if let Some(previous) = by_key.get_mut(&key) {
            if previous.model == event.model {
                // Streaming fragments can omit metadata. Keep explicit effort
                // independently from whichever fragment has complete counters.
                event.reasoning_effort = event
                    .reasoning_effort
                    .or_else(|| previous.reasoning_effort.clone());
                previous.reasoning_effort = event.reasoning_effort.clone();
            }
            if previous.total_tokens > event.total_tokens {
                continue;
            }
        } else {
            order.push(key);
        }
        by_key.insert(event.request_id.clone(), event);
    }

    order
        .into_iter()
        .filter_map(|k| by_key.remove(&k))
        .collect()
}

pub(super) fn explicit_effort(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
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
    fn exact_models_and_explicit_per_turn_effort_are_preserved() {
        let cases = [
            (
                "claude-opus-5",
                serde_json::json!("high"),
                serde_json::json!("xhigh"),
                Some("xhigh"),
            ),
            (
                "claude-fable-5-1",
                serde_json::Value::Null,
                serde_json::json!("high"),
                Some("high"),
            ),
            (
                "claude-fabel-5-1",
                serde_json::json!("ultra"),
                serde_json::Value::Null,
                Some("ultra"),
            ),
            (
                "claude-opus-5",
                serde_json::json!("medium"),
                serde_json::json!(""),
                Some("medium"),
            ),
            (
                "claude-opus-5",
                serde_json::Value::Null,
                serde_json::Value::Null,
                None,
            ),
            (
                "claude-opus-5",
                serde_json::json!(42),
                serde_json::json!(false),
                None,
            ),
        ];
        let text = cases
            .iter()
            .enumerate()
            .map(|(i, (model, effort, per_turn, _))| {
                serde_json::json!({"type":"assistant", "requestId":format!("r{i}"),
                "effort":effort,"perTurnEffort":per_turn,
                "message":{"id":format!("m{i}"),"model":model,
                    "usage":{"input_tokens":10,"output_tokens":20,
                        "output_tokens_details":{"thinking_tokens":15}}}})
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let events = parse_chunk(&text, "fixture");
        assert_eq!(events.len(), cases.len());
        for (event, (model, _, _, expected)) in events.iter().zip(cases) {
            assert_eq!(event.model, model, "model ids are not aliases");
            assert_eq!(event.reasoning_effort.as_deref(), expected);
            assert_eq!(
                event.reasoning_tokens, 15,
                "thinking counters never imply effort"
            );
            assert_eq!(event.total_tokens, 30);
        }
    }

    #[test]
    fn streaming_fragments_keep_effort_and_complete_usage_independently() {
        let fragment = |output, effort: serde_json::Value| {
            serde_json::json!({
                "type":"assistant", "requestId":"r", "effort":effort,
                "message":{"id":"m", "model":"claude-fable-5-1",
                    "usage":{"input_tokens":5,"output_tokens":output}}
            })
            .to_string()
        };
        for text in [
            [
                fragment(1, serde_json::json!("xhigh")),
                fragment(100, serde_json::Value::Null),
            ]
            .join("\n"),
            [
                fragment(100, serde_json::Value::Null),
                fragment(1, serde_json::json!("xhigh")),
            ]
            .join("\n"),
            [
                fragment(100, serde_json::json!("xhigh")),
                fragment(100, serde_json::Value::Null),
            ]
            .join("\n"),
        ] {
            let events = parse_chunk(&text, "fixture");
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].total_tokens, 105);
            assert_eq!(events[0].reasoning_effort.as_deref(), Some("xhigh"));
        }
    }

    #[test]
    fn garbage_never_panics() {
        assert!(parse_chunk("", "x").is_empty());
        assert!(parse_chunk("not json\n{\"usage\":\"assistant\"}\n", "x").is_empty());
    }

    #[test]
    fn less_complete_duplicates_do_not_replace_complete_usage() {
        let line = |output: i64| {
            format!(
                r#"{{"type":"assistant","requestId":"req","message":{{"id":"msg","model":"claude-opus-5","usage":{{"input_tokens":2,"output_tokens":{output}}}}}}}"#
            )
        };
        let events = parse_chunk(&format!("{}\n{}", line(100), line(1)), "x");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].output_tokens, 100);
    }
}
