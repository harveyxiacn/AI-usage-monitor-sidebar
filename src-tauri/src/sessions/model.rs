use crate::model::{SessionRow, TokenTotals};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionListQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub provider: Option<String>,
    pub project: Option<String>,
    pub search: Option<String>,
    pub sort: Option<String>,
    pub offset: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    #[serde(flatten)]
    pub usage: SessionRow,
    pub title: String,
    pub title_source: String,
    pub parent_session_id: Option<String>,
    pub user_turns: i64,
    pub tool_calls: i64,
    pub tool_failures: i64,
    pub repeated_tool_calls: i64,
    pub active_duration_ms: Option<i64>,
    pub transcript_available: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionListResult {
    pub rows: Vec<SessionSummary>,
    pub total: i64,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub id: String,
    pub turn_id: Option<String>,
    pub role: String,
    pub text: String,
    pub timestamp: Option<i64>,
    pub tool_name: Option<String>,
    pub is_error: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTurn {
    pub id: String,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub user_message_id: Option<String>,
    pub totals: Option<TokenTotals>,
    pub tool_calls: i64,
    pub tool_failures: i64,
    pub duration_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub source_updated_at: Option<i64>,
    pub session: SessionSummary,
    pub messages: Vec<SessionMessage>,
    pub total_messages: i64,
    pub next_offset: Option<u32>,
    pub turns: Vec<SessionTurn>,
    pub children: Vec<SessionSummary>,
    pub warnings: Vec<String>,
}

/// Internal paging result for evaluation preparation, without expensive
/// summary, children, or per-turn aggregation on every content page.
pub struct SessionContentPage {
    pub messages: Vec<SessionMessage>,
    pub total_messages: i64,
    pub next_offset: Option<u32>,
    pub warnings: Vec<String>,
    pub source_updated_at: i64,
}
