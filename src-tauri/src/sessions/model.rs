use crate::model::{SessionRow, TokenTotals};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionListQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub provider: Option<String>,
    pub project: Option<String>,
    /// `None` = every account; `Some("")` = the primary one; `Some("work")`.
    pub account: Option<String>,
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

/// One session reduced to the numbers the insights view plots. Titles are the
/// usual alias/native/fallback ones; no transcript content is included.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightSession {
    pub provider: String,
    pub session_id: String,
    pub title: String,
    pub project: String,
    pub last_ts: String,
    pub total_tokens: i64,
    /// `None` when any request of the session has no price.
    pub cost_usd: Option<f64>,
    pub active_duration_ms: Option<i64>,
    pub user_turns: i64,
    pub tool_calls: i64,
    pub tool_failures: i64,
    pub repeated_tool_calls: i64,
    pub failure_rate: Option<f64>,
    pub repeat_rate: Option<f64>,
    /// Subset of `failures`, `repeats`.
    pub flags: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightKpis {
    pub sessions: i64,
    pub priced_sessions: i64,
    pub median_cost_usd: Option<f64>,
    pub median_active_ms: Option<f64>,
    pub median_turns: Option<f64>,
    pub tool_calls: i64,
    pub tool_failures: i64,
    pub repeated_tool_calls: i64,
    pub failure_rate: Option<f64>,
    pub repeat_rate: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistogramBin {
    pub from: f64,
    pub to: f64,
    pub count: i64,
}

/// Log-scale histogram of positive values; zeros are counted separately.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Histogram {
    pub bins: Vec<HistogramBin>,
    pub zero_count: i64,
    pub sample: i64,
    pub median: Option<f64>,
    pub p90: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStat {
    pub tool: String,
    pub calls: i64,
    pub failures: i64,
    pub failure_rate: f64,
    pub sessions: i64,
}

/// Rules behind the warning chips, so the UI can state them exactly.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightThresholds {
    pub min_failures: i64,
    pub failure_rate: f64,
    pub min_repeats: i64,
    pub repeat_rate: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInsights {
    /// Matching sessions before the analysis cap.
    pub total_sessions: i64,
    pub truncated: bool,
    pub kpis: InsightKpis,
    pub cost_histogram: Histogram,
    pub duration_histogram: Histogram,
    pub points: Vec<InsightSession>,
    pub top_cost: Vec<InsightSession>,
    pub top_duration: Vec<InsightSession>,
    pub top_failures: Vec<InsightSession>,
    pub top_repeats: Vec<InsightSession>,
    pub tools: Vec<ToolStat>,
    pub thresholds: InsightThresholds,
}
