//! Provider adapters. Text is transient: the store persists offsets and counters only.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Context {
    #[serde(default)]
    pub version: u32,
    pub session: String,
    pub project: String,
    pub parent: Option<String>,
    pub turn: Option<String>,
    pub model: Option<String>,
    pub tools: BTreeMap<String, (Option<String>, String)>,
    #[serde(default)]
    pub subagent: Option<String>,
    #[serde(default)]
    pub pending_title: Option<(String, i64)>,
    #[serde(default)]
    pub pending_user_turn: Option<String>,
    #[serde(default)]
    pub turn_claimed: bool,
    #[serde(default)]
    pub file_stem: String,
    #[serde(default)]
    pub line_no: u64,
    #[serde(default)]
    pub tail_hash: String,
    #[serde(default)]
    pub tail_len: usize,
    #[serde(default)]
    pub assistant_turns: BTreeMap<String, Option<String>>,
}

pub(crate) struct ParsedMessage {
    pub id: String,
    pub role: String,
    pub text: String,
    pub turn: Option<String>,
    pub tool: Option<String>,
    pub is_error: bool,
    pub is_call: bool,
    pub call_fingerprint: Option<String>,
}

#[derive(Default)]
pub(crate) struct ParsedLine {
    pub timestamp: Option<i64>,
    pub title: Option<(String, i64)>,
    pub messages: Vec<ParsedMessage>,
    pub usage: Option<(String, String)>,
    pub turn_started: Option<String>,
    pub turn_finished: Option<String>,
    pub relink_turn: Option<(String, String)>,
}

pub(crate) fn hash(bytes: &[u8]) -> String {
    // Stable, non-cryptographic identity check; never used for authentication.
    let mut value = 0xcbf29ce484222325u64;
    for byte in bytes {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100000001b3);
    }
    format!("{value:016x}")
}

fn string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
}
fn timestamp(value: &Value) -> Option<i64> {
    let value = value.get("timestamp").or_else(|| value.get("updated_at"))?;
    value.as_i64().or_else(|| {
        value
            .as_str()
            .and_then(crate::commands::ingest::claude::parse_ts_ms)
    })
}
fn text(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        return s.to_string();
    }
    if let Some(items) = value.as_array() {
        return items
            .iter()
            .filter_map(|v| {
                let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
                if matches!(kind, "text" | "input_text" | "output_text") {
                    string(v, "text")
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}
fn genuine_user(s: &str) -> bool {
    let trimmed = s.trim();
    !trimmed.is_empty()
        && ![
            "<environment_context>",
            "<permissions instructions>",
            "<INSTRUCTIONS>",
            "# AGENTS.md instructions",
            "<system-reminder>",
            "<local-command-caveat>",
            "<command-name>",
            "<local-command-stdout>",
        ]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

fn user_text(content: &Value) -> String {
    if let Some(items) = content.as_array() {
        items
            .iter()
            .filter_map(|item| string(item, "text"))
            .filter(|body| genuine_user(body))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        let body = text(content);
        if genuine_user(&body) {
            body
        } else {
            String::new()
        }
    }
}

fn error_output(value: &Value, output: &str) -> bool {
    value
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || value
            .get("exit_code")
            .and_then(Value::as_i64)
            .is_some_and(|n| n != 0)
        || value.get("status").and_then(Value::as_str) == Some("failed")
        || output.lines().any(|line| {
            line.trim()
                .strip_prefix("Process exited with code ")
                .and_then(|code| code.trim().parse::<i64>().ok())
                .is_some_and(|n| n != 0)
        })
}

fn message(id: String, role: &str, body: String, ctx: &Context) -> ParsedMessage {
    ParsedMessage {
        id,
        role: role.into(),
        text: body,
        turn: ctx.turn.clone(),
        tool: None,
        is_error: false,
        is_call: false,
        call_fingerprint: None,
    }
}

pub(crate) fn parse(provider: &str, value: &Value, ctx: &mut Context, offset: u64) -> ParsedLine {
    let mut out = ParsedLine {
        timestamp: timestamp(value),
        ..Default::default()
    };
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
    if provider == "codex" {
        codex(value, kind, ctx, offset, &mut out);
    } else {
        claude(value, kind, ctx, offset, &mut out);
    }
    if let Some((title, _)) = &mut out.title {
        *title = title.trim().chars().take(300).collect();
    }
    if ctx.session.is_empty() {
        if out.title.is_some() {
            ctx.pending_title = out.title.take();
        }
    } else if out.title.is_none() {
        out.title = ctx.pending_title.take();
    }
    out
}

fn codex(value: &Value, kind: &str, ctx: &mut Context, offset: u64, out: &mut ParsedLine) {
    let payload = value.get("payload").unwrap_or(value);
    if kind == "session_meta" || (kind.is_empty() && value.get("thread_name").is_some()) {
        if let Some(id) = string(payload, "session_id")
            .or_else(|| string(payload, "id"))
            .or_else(|| string(payload, "thread_id"))
        {
            ctx.session = id;
        }
        if let Some(cwd) = string(payload, "cwd") {
            ctx.project = cwd;
        }
        ctx.parent = string(payload, "parent_thread_id")
            .or_else(|| string(payload, "parent_session_id"))
            .or_else(|| {
                payload
                    .pointer("/source/subagent/thread_spawn/parent_thread_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(|| ctx.parent.clone());
        out.title = string(payload, "thread_name")
            .map(|title| (title, 4))
            .or_else(|| {
                string(payload, "name")
                    .or_else(|| string(payload, "title"))
                    .map(|title| (title, 2))
            });
        return;
    }
    if kind == "turn_context" {
        if let Some(next) = string(payload, "turn_id") {
            if let Some(pending) = ctx.pending_user_turn.take() {
                out.relink_turn = Some((pending.clone(), next.clone()));
                for (turn, _) in ctx.tools.values_mut() {
                    if turn.as_ref() == Some(&pending) {
                        *turn = Some(next.clone());
                    }
                }
                ctx.turn_claimed = true;
            } else if ctx.turn.as_ref() != Some(&next) {
                ctx.turn_claimed = false;
            }
            ctx.turn = Some(next);
        }
        ctx.model = string(payload, "model").or_else(|| ctx.model.clone());
        if let Some(cwd) = string(payload, "cwd") {
            ctx.project = cwd;
        }
        return;
    }
    if kind == "token_usage_record" {
        if let Some(session) =
            string(payload, "thread_id").or_else(|| string(payload, "session_id"))
        {
            ctx.session = session;
        }
        if let Some(turn) = string(payload, "turn_id") {
            let request = string(payload, "response_id")
                .unwrap_or_else(|| format!("{}:{turn}", ctx.file_stem));
            out.usage = Some((request, turn));
        }
        return;
    }
    if kind == "event_msg" {
        let event = payload.get("type").and_then(Value::as_str).unwrap_or("");
        if event == "task_started" {
            if let Some(next) = string(payload, "turn_id") {
                if ctx.turn.as_ref() != Some(&next) {
                    ctx.turn_claimed = false;
                }
                ctx.turn = Some(next);
            }
            out.turn_started = ctx.turn.clone();
        } else if event == "task_complete" {
            out.turn_finished = string(payload, "turn_id").or_else(|| ctx.turn.clone());
            ctx.turn_claimed = true;
        } else if event == "token_count" {
            let turn = string(payload, "turn_id")
                .or_else(|| ctx.turn.clone().filter(|t| !t.starts_with("message:")));
            if let Some(turn) = turn {
                out.usage = Some((
                    format!("{}:{}", ctx.file_stem, ctx.line_no.saturating_sub(1)),
                    turn,
                ));
            }
        }
        // response_item is the canonical message stream; the UI event copies
        // (user_message/agent_message) must not double-count the same turn.
        return;
    }
    if kind != "response_item" {
        return;
    }
    let explicit_turn = string(payload, "turn_id").or_else(|| string(value, "turn_id"));
    if let Some(turn) = &explicit_turn {
        ctx.turn = Some(turn.clone());
    }
    let item = payload.get("type").and_then(Value::as_str).unwrap_or("");
    let id = string(payload, "id")
        .or_else(|| string(value, "id"))
        .unwrap_or_else(|| format!("{}:{offset}", ctx.session));
    match item {
        "message" => {
            let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
            if !matches!(role, "user" | "assistant") {
                return;
            }
            let body = if role == "user" {
                user_text(&payload["content"])
            } else {
                text(&payload["content"])
            };
            if body.is_empty() || (role == "user" && !genuine_user(&body)) {
                return;
            }
            if role == "user" && explicit_turn.is_none() && (ctx.turn.is_none() || ctx.turn_claimed)
            {
                // This synthetic boundary is local only. Usage remains unknown
                // unless an explicit provider turn link exists.
                ctx.turn = Some(format!("message:{id}"));
                ctx.pending_user_turn = ctx.turn.clone();
            }
            if role == "user" {
                ctx.turn_claimed = true;
                out.turn_started = ctx.turn.clone();
            }
            out.messages.push(message(id, role, body, ctx));
        }
        "function_call" | "custom_tool_call" => {
            let call = string(payload, "call_id").unwrap_or(id);
            let name = string(payload, "name").unwrap_or_else(|| "unknown".into());
            let args = string(payload, "arguments")
                .or_else(|| string(payload, "input"))
                .unwrap_or_default();
            let mut msg = message(format!("call:{call}"), "tool", args.clone(), ctx);
            msg.is_call = true;
            msg.tool = Some(name.clone());
            msg.call_fingerprint = Some(hash(format!("{name}\0{args}").as_bytes()));
            if ctx.tools.len() < 4096 {
                ctx.tools.insert(call, (ctx.turn.clone(), name));
            }
            out.messages.push(msg);
        }
        "function_call_output" | "custom_tool_call_output" => {
            let call = string(payload, "call_id").unwrap_or(id);
            let output = text(&payload["output"]);
            let mut msg = message(format!("result:{call}"), "tool", output.clone(), ctx);
            msg.turn = None;
            if let Some((turn, name)) = ctx.tools.get(&call) {
                msg.turn = turn.clone();
                msg.tool = Some(name.clone());
            }
            msg.is_error = error_output(payload, &output);
            out.messages.push(msg);
        }
        _ => {}
    }
}

fn claude(value: &Value, kind: &str, ctx: &mut Context, offset: u64, out: &mut ParsedLine) {
    if let Some(id) = string(value, "sessionId") {
        let agent = string(value, "agentId")
            .or_else(|| ctx.subagent.clone())
            .map(|s| s.strip_prefix("agent-").unwrap_or(&s).to_string());
        if let Some(agent) = agent {
            ctx.session = format!("{id}:agent:{agent}");
            ctx.parent = Some(id);
            ctx.subagent = Some(agent);
        } else {
            ctx.session = id;
        }
    }
    if let Some(cwd) = string(value, "cwd") {
        ctx.project = cwd;
    }
    ctx.parent = string(value, "parentSessionId")
        .or_else(|| string(value, "parent_session_id"))
        .or_else(|| ctx.parent.clone());
    if kind == "custom-title" {
        out.title = string(value, "customTitle").map(|t| (t, 3));
        return;
    }
    if kind == "summary" {
        out.title = string(value, "summary").map(|t| (t, 1));
        return;
    }
    if !matches!(kind, "user" | "assistant")
        || value.get("isMeta").and_then(Value::as_bool) == Some(true)
    {
        return;
    }
    let msg = &value["message"];
    let id = if kind == "assistant" {
        string(msg, "id").or_else(|| string(value, "uuid"))
    } else {
        string(value, "uuid")
    }
    .unwrap_or_else(|| format!("{}:{offset}", ctx.session));
    // Streaming fragments can arrive after the next user has already spoken.
    // Bind the stable assistant message id to its first observed turn, including
    // tool blocks that only appear in a later complete fragment.
    let current_turn = ctx.turn.clone();
    if kind == "assistant" {
        if let Some(turn) = ctx.assistant_turns.get(&id) {
            ctx.turn = turn.clone();
        } else if ctx.assistant_turns.len() < 4096 {
            ctx.assistant_turns.insert(id.clone(), ctx.turn.clone());
        }
    }
    let content = &msg["content"];
    let has_user_text = kind == "user" && genuine_user(&text(content));
    if has_user_text {
        ctx.turn = Some(format!("user:{id}"));
        out.turn_started = ctx.turn.clone();
    }
    if let Some(items) = content.as_array() {
        for (index, block) in items.iter().enumerate() {
            match block.get("type").and_then(Value::as_str).unwrap_or("") {
                "text" => {
                    let body = string(block, "text").unwrap_or_default();
                    if body.is_empty() || (kind == "user" && !genuine_user(&body)) {
                        continue;
                    }
                    out.messages
                        .push(message(format!("{id}:{index}"), kind, body, ctx));
                }
                "tool_use" => {
                    let call = string(block, "id").unwrap_or_else(|| format!("{id}:{index}"));
                    let name = string(block, "name").unwrap_or_else(|| "unknown".into());
                    let args = block.get("input").map(Value::to_string).unwrap_or_default();
                    let mut m = message(format!("call:{call}"), "tool", args.clone(), ctx);
                    m.is_call = true;
                    m.tool = Some(name.clone());
                    m.call_fingerprint = Some(hash(format!("{name}\0{args}").as_bytes()));
                    if ctx.tools.len() < 4096 {
                        ctx.tools.insert(call, (ctx.turn.clone(), name));
                    }
                    out.messages.push(m);
                }
                "tool_result" => {
                    let call =
                        string(block, "tool_use_id").unwrap_or_else(|| format!("{id}:{index}"));
                    let body = text(&block["content"]);
                    let mut m = message(format!("result:{call}"), "tool", body.clone(), ctx);
                    m.turn = None;
                    if let Some((turn, name)) = ctx.tools.get(&call) {
                        m.turn = turn.clone();
                        m.tool = Some(name.clone());
                    }
                    m.is_error = error_output(block, &body);
                    out.messages.push(m);
                }
                _ => {}
            }
        }
    } else {
        let body = text(content);
        if !body.is_empty() && (kind != "user" || genuine_user(&body)) {
            out.messages.push(message(id.clone(), kind, body, ctx));
        }
    }
    if kind == "assistant" && msg.get("usage").is_some() {
        if let Some(turn) = &ctx.turn {
            let request = string(value, "requestId").unwrap_or_default();
            out.usage = Some((format!("{id}:{request}"), turn.clone()));
        }
    }
    if kind == "assistant" {
        ctx.turn = current_turn;
    }
}
