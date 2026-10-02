use super::*;
use crate::commands::{
    pricing,
    store::{insert_usage_events, Db, UsageEvent},
    test_support::tempdir,
};
use model::SessionListQuery;
use serde_json::json;
use std::io::Write;

fn fixture(lines: &[serde_json::Value]) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = tempdir();
    let path = dir.join("session.jsonl");
    let content = lines
        .iter()
        .map(|v| v.to_string() + "\n")
        .collect::<String>();
    std::fs::write(&path, content).unwrap();
    (dir, path)
}
fn usage(id: &str, session: &str, ts: i64, total: i64) -> UsageEvent {
    UsageEvent {
        provider: "codex".into(),
        model: "gpt-5.3-codex".into(),
        ts,
        session_id: Some(session.into()),
        request_id: id.into(),
        input_tokens: total,
        total_tokens: total,
        cwd: Some("/project".into()),
        ..UsageEvent::default()
    }
}

#[test]
fn titles_content_gate_tools_and_parent_links_are_distinct() {
    let db = Db::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    let (dir, path) = fixture(&[
        json!({"type":"session_meta","payload":{"id":"child","cwd":"/project","parent_thread_id":"parent","title":"Native title"}}),
        json!({"type":"turn_context","payload":{"turn_id":"turn-1","model":"gpt-5.3-codex"}}),
        json!({"type":"response_item","timestamp":"2026-09-28T01:00:00Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Build the private widget"}]}}),
        json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call-1","arguments":"{\"cmd\":\"test\"}"}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"Process exited with code 1\nfailed"}}),
        json!({"type":"token_usage_record","payload":{"thread_id":"child","turn_id":"turn-1","response_id":"r1","usage":{"input_tokens":12}}}),
    ]);
    insert_usage_events(&db, &[usage("r1", "child", 1790557200000, 12)]).unwrap();
    index_file(&db, "codex", &path).unwrap();
    let closed = detail(
        &db,
        "codex",
        "child",
        0,
        50,
        &pricing::default_table(),
        false,
    )
    .unwrap();
    assert!(closed.messages.is_empty());
    assert_eq!(closed.session.title, "Native title");
    assert_eq!(closed.session.parent_session_id.as_deref(), Some("parent"));
    assert_eq!(closed.session.user_turns, 1);
    assert_eq!(closed.session.tool_calls, 1);
    assert_eq!(closed.session.tool_failures, 1);
    let open = detail(
        &db,
        "codex",
        "child",
        0,
        50,
        &pricing::default_table(),
        true,
    )
    .unwrap();
    assert_eq!(open.messages.iter().filter(|m| m.role == "user").count(), 1);
    assert!(open
        .messages
        .iter()
        .any(|m| m.text == "Build the private widget"));
    assert_eq!(open.turns[0].totals.as_ref().unwrap().total_tokens, 12);
    set_alias(&db, "codex", "child", "My alias").unwrap();
    assert_eq!(
        detail(
            &db,
            "codex",
            "child",
            0,
            50,
            &pricing::default_table(),
            false
        )
        .unwrap()
        .session
        .title_source,
        "alias"
    );
    clear_analysis(&db, "codex", "child").unwrap();
    assert_eq!(
        detail(
            &db,
            "codex",
            "child",
            0,
            50,
            &pricing::default_table(),
            false
        )
        .unwrap()
        .session
        .title,
        "Native title"
    );
    let dump: String = db
        .lock()
        .query_row(
            "SELECT group_concat(sql) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!dump.contains("prompt_text"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pagination_returns_recent_low_token_session_outside_legacy_top_200() {
    let db = Db::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    let mut rows = (0..250)
        .map(|i| usage(&format!("r{i}"), &format!("s{i:03}"), i, 1000))
        .collect::<Vec<_>>();
    rows.push(usage("new", "recent-low", 10000, 1));
    insert_usage_events(&db, &rows).unwrap();
    let q = SessionListQuery {
        sort: Some("recent".into()),
        limit: Some(20),
        ..Default::default()
    };
    let result = list(&db, &q, &pricing::default_table(), false).unwrap();
    assert_eq!(result.total, 251);
    assert_eq!(result.rows.len(), 20);
    assert_eq!(result.rows[0].usage.session_id, "recent-low");
    let next = list(
        &db,
        &SessionListQuery {
            offset: Some(240),
            ..q
        },
        &pricing::default_table(),
        false,
    )
    .unwrap();
    assert_eq!(next.rows.len(), 11);
}

#[test]
fn incremental_index_deduplicates_and_reports_deleted_sources() {
    let db = Db::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    let (dir, path) = fixture(&[
        json!({"type":"user","uuid":"u1","sessionId":"c1","message":{"role":"user","content":"Please implement export"}}),
        json!({"type":"user","uuid":"tool-result","sessionId":"c1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call1","content":"result"}]}}),
        json!({"type":"user","uuid":"context","sessionId":"c1","isMeta":true,"message":{"role":"user","content":"<environment_context>private</environment_context>"}}),
    ]);
    index_file(&db, "claude", &path).unwrap();
    index_file(&db, "claude", &path).unwrap();
    let first = detail(&db, "claude", "c1", 0, 1, &pricing::default_table(), true).unwrap();
    assert_eq!(first.session.user_turns, 1);
    assert_eq!(first.total_messages, 2);
    assert_eq!(first.next_offset, Some(1));
    assert_eq!(first.session.title_source, "prompt");
    assert_eq!(
        detail(&db, "claude", "c1", 0, 20, &pricing::default_table(), false)
            .unwrap()
            .session
            .title_source,
        "fallback"
    );
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(
        f,
        "{}",
        json!({"type":"custom-title","sessionId":"c1","customTitle":"Renamed"})
    )
    .unwrap();
    index_file(&db, "claude", &path).unwrap();
    assert_eq!(
        detail(&db, "claude", "c1", 0, 10, &pricing::default_table(), true)
            .unwrap()
            .session
            .title,
        "Renamed"
    );
    std::fs::remove_file(&path).unwrap();
    let removed = detail(&db, "claude", "c1", 0, 10, &pricing::default_table(), true).unwrap();
    assert!(!removed.warnings.is_empty());
    assert!(removed.messages.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn partial_lines_native_index_titles_and_replaced_sources_are_safe() {
    let db = Db::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    let (dir, path) = fixture(&[
        json!({"type":"session_meta","payload":{"id":"s","cwd":"/p"}}),
        json!({"type":"turn_context","payload":{"turn_id":"t"}}),
    ]);
    let line=json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Secret example only"}]}}).to_string();
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    write!(f, "{line}").unwrap();
    drop(f);
    index_file(&db, "codex", &path).unwrap();
    assert_eq!(
        detail(&db, "codex", "s", 0, 10, &pricing::default_table(), true)
            .unwrap()
            .total_messages,
        0
    );
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(f).unwrap();
    drop(f);
    index_file(&db, "codex", &path).unwrap();
    let first = detail(&db, "codex", "s", 0, 10, &pricing::default_table(), true).unwrap();
    assert_eq!(first.total_messages, 1);
    let title_path = dir.join("session_index.jsonl");
    std::fs::write(
        &title_path,
        json!({"id":"s","thread_name":"From Codex index","updated_at":"2026-09-28T01:00:00Z"})
            .to_string()
            + "\n",
    )
    .unwrap();
    index_codex_titles(&db, &title_path).unwrap();
    assert_eq!(
        detail(&db, "codex", "s", 0, 10, &pricing::default_table(), false)
            .unwrap()
            .session
            .title,
        "From Codex index"
    );
    std::fs::write(&path, "changed\n").unwrap();
    let stale = detail(&db, "codex", "s", 0, 10, &pricing::default_table(), true).unwrap();
    assert!(stale.messages.is_empty());
    assert!(stale
        .warnings
        .iter()
        .any(|w| w.contains("replaced") || w.contains("changed")));
    index_file(&db, "codex", &path).unwrap();
    assert_eq!(
        detail(&db, "codex", "s", 0, 10, &pricing::default_table(), true)
            .unwrap()
            .total_messages,
        0
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn claude_summary_before_identity_and_subagents_preserve_separate_usage() {
    let db = Db::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    let (dir, path) = fixture(&[
        json!({"type":"summary","summary":"Prior native summary"}),
        json!({"type":"user","sessionId":"parent","uuid":"u","message":{"content":"Please build this"}}),
        json!({"type":"assistant","sessionId":"parent","requestId":"r","message":{"id":"a","content":[{"type":"text","text":"First"}],"usage":{"input_tokens":1}}}),
        json!({"type":"assistant","sessionId":"parent","requestId":"r","message":{"id":"a","content":[{"type":"text","text":"First complete answer"}],"usage":{"input_tokens":1,"output_tokens":5}}}),
    ]);
    index_file(&db, "claude", &path).unwrap();
    let parent = detail(
        &db,
        "claude",
        "parent",
        0,
        30,
        &pricing::default_table(),
        true,
    )
    .unwrap();
    assert_eq!(parent.session.title, "Prior native summary");
    assert_eq!(parent.total_messages, 2);
    assert_eq!(
        parent.messages.last().unwrap().text,
        "First complete answer"
    );
    let agents = dir.join("subagents");
    std::fs::create_dir(&agents).unwrap();
    let child = agents.join("agent-x.jsonl");
    std::fs::write(
        &child,
        json!({"type":"user","sessionId":"parent","uuid":"child-u","message":{"content":"Subtask"}})
            .to_string() + "\n",
    )
    .unwrap();
    index_file(&db, "claude", &child).unwrap();
    let parent = detail(
        &db,
        "claude",
        "parent",
        0,
        30,
        &pricing::default_table(),
        true,
    )
    .unwrap();
    assert_eq!(parent.children.len(), 1);
    assert_eq!(parent.total_messages, 2);
    assert_eq!(
        parent.children[0].parent_session_id.as_deref(),
        Some("parent")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn user_before_turn_context_is_relinked_and_unknown_turns_stay_distinct() {
    let db = Db::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    let (dir, path) = fixture(&[
        json!({"type":"session_meta","payload":{"id":"s"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>automatic</environment_context>"},{"type":"input_text","text":"Real first request"}]}}),
        json!({"type":"turn_context","payload":{"turn_id":"t1"}}),
        json!({"type":"token_usage_record","payload":{"thread_id":"s","turn_id":"t1","response_id":"r","usage":{"input_tokens":10}}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Second request with no explicit turn"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Third request with no explicit turn"}]}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"missing","output":"unknown call result"}}),
    ]);
    insert_usage_events(&db, &[usage("r", "s", 100, 10)]).unwrap();
    index_file(&db, "codex", &path).unwrap();
    let detail = detail(&db, "codex", "s", 0, 100, &pricing::default_table(), true).unwrap();
    assert_eq!(detail.session.user_turns, 3);
    assert_eq!(detail.messages[0].turn_id.as_deref(), Some("t1"));
    assert_eq!(detail.messages[0].text, "Real first request");
    assert_eq!(detail.messages.last().unwrap().turn_id, None);
    assert_eq!(
        detail.turns.iter().filter(|t| t.totals.is_some()).count(),
        1
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn large_message_pages_advance_only_over_delivered_records() {
    let db = Db::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    let mut lines = vec![json!({"type":"session_meta","payload":{"id":"long"}})];
    for i in 0..15 {
        lines.push(json!({"type":"response_item","payload":{"type":"message","id":format!("m{i}"),"role":"user","content":[{"type":"input_text","text":"x".repeat(12000)}]}}));
    }
    let (dir, path) = fixture(&lines);
    index_file(&db, "codex", &path).unwrap();
    let first = detail(
        &db,
        "codex",
        "long",
        0,
        100,
        &pricing::default_table(),
        true,
    )
    .unwrap();
    assert_eq!(first.messages.len(), 10);
    assert_eq!(first.next_offset, Some(10));
    let second = detail(
        &db,
        "codex",
        "long",
        10,
        100,
        &pricing::default_table(),
        true,
    )
    .unwrap();
    assert_eq!(second.messages.len(), 5);
    assert_eq!(second.next_offset, None);
    assert!(second.messages.iter().all(|m| !m.text.is_empty()));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn stored_metadata_contains_no_transcript_text_or_tool_arguments() {
    let db = Db::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    let (dir, path) = fixture(&[
        json!({"type":"session_meta","payload":{"id":"private"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"UNIQUE_PRIVATE_PROMPT"}]}}),
        json!({"type":"response_item","payload":{"type":"function_call","name":"exec","call_id":"c","arguments":"UNIQUE_PRIVATE_ARGS"}}),
    ]);
    index_file(&db, "codex", &path).unwrap();
    let conn = db.lock();
    for table in [
        "session_metadata",
        "session_aliases",
        "session_sources",
        "session_message_refs",
        "session_turns",
        "session_usage_links",
    ] {
        let mut stmt = conn.prepare(&format!("SELECT * FROM {table}")).unwrap();
        let columns = stmt.column_count();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for i in 0..columns {
                if let Ok(value) = row.get::<_, String>(i) {
                    assert!(!value.contains("UNIQUE_PRIVATE_PROMPT"));
                    assert!(!value.contains("UNIQUE_PRIVATE_ARGS"));
                }
            }
        }
    }
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recent_order_includes_new_user_activity_before_next_usage_record() {
    let db = Db::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    insert_usage_events(
        &db,
        &[
            usage("older", "active", 100, 5),
            usage("newer", "idle", 200, 50),
        ],
    )
    .unwrap();
    let (dir, path) = fixture(&[
        json!({"type":"session_meta","payload":{"id":"active"}}),
        json!({"type":"response_item","timestamp":"2026-09-28T01:00:00Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"A fresh user request"}]}}),
    ]);
    index_file(&db, "codex", &path).unwrap();
    let result = list(
        &db,
        &SessionListQuery::default(),
        &pricing::default_table(),
        false,
    )
    .unwrap();
    assert_eq!(result.rows[0].usage.session_id, "active");
    assert_eq!(result.rows[0].usage.totals.total_tokens, 5);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn delayed_streaming_completion_keeps_the_original_user_turn() {
    let db = Db::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    let (dir, path) = fixture(&[
        json!({"type":"user","sessionId":"s","uuid":"first","message":{"content":"First task"}}),
        json!({"type":"assistant","sessionId":"s","requestId":"r","message":{"id":"answer","content":[{"type":"text","text":"Partial"}],"usage":{"input_tokens":1}}}),
        json!({"type":"user","sessionId":"s","uuid":"second","message":{"content":"Second task"}}),
        json!({"type":"assistant","sessionId":"s","requestId":"r","message":{"id":"answer","content":[{"type":"text","text":"Completed first answer"},{"type":"tool_use","id":"late-call","name":"test","input":{}}],"usage":{"input_tokens":1,"output_tokens":10}}}),
    ]);
    let mut event = usage("answer:r", "s", 100, 11);
    event.provider = "claude".into();
    insert_usage_events(&db, &[event]).unwrap();
    index_file(&db, "claude", &path).unwrap();
    let result = detail(&db, "claude", "s", 0, 50, &pricing::default_table(), true).unwrap();
    assert_eq!(
        result
            .messages
            .iter()
            .find(|m| m.role == "assistant")
            .unwrap()
            .turn_id
            .as_deref(),
        Some("user:first")
    );
    assert_eq!(
        result
            .messages
            .iter()
            .find(|m| m.id == "call:late-call")
            .unwrap()
            .turn_id
            .as_deref(),
        Some("user:first")
    );
    assert_eq!(
        result
            .turns
            .iter()
            .find(|t| t.id == "user:first")
            .unwrap()
            .totals
            .as_ref()
            .unwrap()
            .total_tokens,
        11
    );
    assert_eq!(
        result
            .turns
            .iter()
            .find(|t| t.id == "user:second")
            .unwrap()
            .totals,
        None
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[allow(clippy::too_many_arguments)]
fn tool_ref(
    db: &Db,
    session: &str,
    n: usize,
    ts: i64,
    tool: &str,
    is_call: bool,
    is_error: bool,
    fingerprint: &str,
) {
    db.lock()
        .execute(
            "INSERT INTO session_message_refs(provider,session_id,message_id,path,byte_offset,byte_len,line_hash,message_index,
            role,turn_id,ts,tool_name,is_error,is_call,call_fingerprint,content_chars)
            VALUES('codex',?1,?2,'p',0,1,'h',?3,'tool',NULL,?4,?5,?6,?7,?8,0)",
            rusqlite::params![
                session,
                format!("m{n}"),
                n as i64,
                ts,
                tool,
                is_error as i64,
                is_call as i64,
                is_call.then_some(fingerprint)
            ],
        )
        .unwrap();
}

#[test]
fn insights_aggregate_metrics_tools_and_flags_without_content() {
    let db = Db::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    insert_usage_events(
        &db,
        &[
            usage("a", "cheap", 1_000, 1_000),
            usage("b", "pricey", 2_000, 2_000_000),
            usage("c", "", 3_000, 5),
        ],
    )
    .unwrap();
    // "cheap": 2 distinct calls. "pricey": 8 calls, 4 failures, 6 repeats of one signature.
    tool_ref(&db, "cheap", 1, 1_000, "shell", true, false, "x");
    tool_ref(&db, "cheap", 2, 2_000, "read", true, false, "y");
    for i in 0..8 {
        let ts = 2_000 + i as i64 * 60_000;
        tool_ref(
            &db,
            "pricey",
            10 + i,
            ts,
            "shell",
            true,
            false,
            if i < 3 { "same" } else { "f" },
        );
    }
    for i in 0..4 {
        tool_ref(&db, "pricey", 30 + i, 700_000, "shell", false, true, "");
    }
    let q = SessionListQuery::default();
    let result = insights(&db, &q, &pricing::default_table()).unwrap();
    assert_eq!(
        result.total_sessions, 2,
        "the unassigned bucket is not a session"
    );
    assert!(!result.truncated);
    assert_eq!(result.kpis.sessions, 2);
    assert_eq!(result.kpis.tool_calls, 10);
    assert_eq!(result.kpis.tool_failures, 4);
    assert_eq!(result.kpis.failure_rate, Some(0.4));
    assert_eq!(result.kpis.median_turns, Some(0.0));
    let pricey = result
        .points
        .iter()
        .find(|p| p.session_id == "pricey")
        .unwrap();
    assert_eq!(pricey.tool_calls, 8);
    assert_eq!(pricey.tool_failures, 4);
    assert!(pricey.flags.contains(&"failures".to_string()));
    assert!(
        pricey.cost_usd.unwrap()
            > result
                .points
                .iter()
                .find(|p| p.session_id == "cheap")
                .unwrap()
                .cost_usd
                .unwrap()
    );
    assert_eq!(result.top_cost[0].session_id, "pricey");
    assert_eq!(result.top_failures[0].session_id, "pricey");
    assert_eq!(result.tools[0].tool, "shell");
    assert_eq!(result.tools[0].calls, 9);
    assert_eq!(result.tools[0].failures, 4);
    assert_eq!(result.tools[0].sessions, 2);
    assert_eq!(result.tools[1].tool, "read");
    assert_eq!(result.cost_histogram.sample, 2);
    let filtered = insights(
        &db,
        &SessionListQuery {
            provider: Some("claude".into()),
            ..Default::default()
        },
        &pricing::default_table(),
    )
    .unwrap();
    assert_eq!(filtered.kpis.sessions, 0);
    assert!(filtered.tools.is_empty());
    assert_eq!(filtered.kpis.median_cost_usd, None);
}

#[test]
fn insights_cap_reports_truncation_and_keeps_newest() {
    let db = Db::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    let rows = (0..1005)
        .map(|i| usage(&format!("r{i}"), &format!("s{i:04}"), 1_000 + i, 10))
        .collect::<Vec<_>>();
    insert_usage_events(&db, &rows).unwrap();
    let result = insights(&db, &SessionListQuery::default(), &pricing::default_table()).unwrap();
    assert_eq!(result.total_sessions, 1005);
    assert!(result.truncated);
    assert_eq!(result.points.len(), 1000);
    assert!(result.points.iter().any(|p| p.session_id == "s1004"));
    assert!(!result.points.iter().any(|p| p.session_id == "s0000"));
}

#[test]
fn percentiles_and_log_histogram_bins_are_correct() {
    use super::insights::{log_histogram, percentile};
    assert_eq!(percentile(&[], 0.5), None);
    assert_eq!(percentile(&[4.0], 0.9), Some(4.0));
    assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 0.5), Some(2.5));
    assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0, 5.0], 0.9), Some(4.6));
    let empty = log_histogram(&[]);
    assert!(empty.bins.is_empty() && empty.median.is_none());
    let h = log_histogram(&[0.0, 0.011, 0.5, 0.9, 12.0]);
    assert_eq!(h.zero_count, 1);
    assert_eq!(h.sample, 5);
    assert_eq!(h.bins.iter().map(|b| b.count).sum::<i64>(), 4);
    let first = h.bins.first().unwrap();
    let last = h.bins.last().unwrap();
    assert!(first.from <= 0.011 && 0.011 < first.to);
    assert!(last.from <= 12.0 && 12.0 < last.to);
    assert!(h
        .bins
        .windows(2)
        .all(|w| (w[0].to - w[1].from).abs() < 1e-9));
    assert_eq!(h.median, Some(0.5));
}
