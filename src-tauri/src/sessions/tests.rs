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
