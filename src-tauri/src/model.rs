//! Canonical shared types. Mirror of `src/lib/types.ts` — keep in sync.
//! See docs/ARCHITECTURE.md §4.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    FiveHour,
    SevenDay,
    Other,
}

impl WindowKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            WindowKind::FiveHour => "five_hour",
            WindowKind::SevenDay => "seven_day",
            WindowKind::Other => "other",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "five_hour" => WindowKind::FiveHour,
            "seven_day" => WindowKind::SevenDay,
            _ => WindowKind::Other,
        }
    }
    /// Classify a window by its duration in seconds.
    pub fn from_seconds(secs: u64) -> Self {
        if secs <= 6 * 3600 {
            WindowKind::FiveHour
        } else if (6 * 86_400..=8 * 86_400).contains(&secs) {
            WindowKind::SevenDay
        } else {
            WindowKind::Other
        }
    }
}

/// How much the burn-rate estimate can be trusted (sample count + time span).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForecastConfidence {
    Low,
    Medium,
    High,
}

/// "At this pace" projection for one quota window — see `forecast.rs`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaForecast {
    /// used percent expected at the reset; never below the current value and
    /// deliberately *not* capped at 100 (the UI caps it where it must)
    pub projected_percent_at_reset: f64,
    /// RFC 3339 UTC, set only when 100 % is reached *before* the reset
    pub exhausts_at: Option<String>,
    /// current burn rate in percentage points per hour (always > 0)
    pub rate_percent_per_hour: f64,
    pub confidence: ForecastConfidence,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub kind: WindowKind,
    pub label: String,
    pub window_seconds: Option<u64>,
    /// used percentage 0..100
    pub used_percent: f64,
    /// RFC 3339 UTC
    pub resets_at: Option<String>,
    pub scope: Option<String>,
    pub is_primary: bool,
    /// Burn-rate projection; absent when there is not enough usable history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forecast: Option<QuotaForecast>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStatus {
    Ok,
    NotLoggedIn,
    TokenExpired,
    /// The provider answered `HTTP 429`. Not an error: the last known windows
    /// are simply getting stale until `ProviderQuota.next_attempt_at`.
    RateLimited,
    Error,
    Disabled,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    Api,
    LocalLog,
    Cache,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub email: Option<String>,
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreditsInfo {
    pub has_credits: bool,
    pub unlimited: bool,
    pub balance: Option<String>,
}

/// How loudly the UI should render a `QuotaExtra`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtraSeverity {
    #[default]
    Info,
    Warn,
    Critical,
}

/// One provider-neutral fact that is not a rate-limit window: a credit
/// balance, a spend limit that was hit, models the plan cannot use right now…
///
/// `kind` is a stable machine id; the frontend looks up `extras.<kind>` for the
/// label, so the backend never ships English prose. `value` / `detail` are
/// already-formatted numbers or names.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaExtra {
    pub kind: String,
    /// None = the label alone carries the meaning (a flag).
    pub value: Option<String>,
    pub detail: Option<String>,
    #[serde(default)]
    pub severity: ExtraSeverity,
}

impl QuotaExtra {
    /// Flag-style extra: label only.
    pub fn flag(kind: &str, severity: ExtraSeverity) -> Self {
        QuotaExtra {
            kind: kind.to_string(),
            value: None,
            detail: None,
            severity,
        }
    }

    /// Extra with a display value ("2", "gpt-5.3-codex", …).
    pub fn value(kind: &str, value: impl Into<String>, severity: ExtraSeverity) -> Self {
        QuotaExtra {
            kind: kind.to_string(),
            value: Some(value.into()),
            detail: None,
            severity,
        }
    }

    pub fn with_detail(mut self, detail: Option<String>) -> Self {
        self.detail = detail;
        self
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderQuota {
    /// "claude" | "codex"
    pub provider: String,
    pub display_name: String,
    pub plan: Option<String>,
    pub plan_label: Option<String>,
    pub account: Option<AccountInfo>,
    pub windows: Vec<QuotaWindow>,
    /// RFC 3339 UTC
    pub fetched_at: String,
    pub source: DataSource,
    pub status: ProviderStatus,
    pub error: Option<String>,
    pub credits: Option<CreditsInfo>,
    /// Provider-neutral extras (credits, spend limits, unavailable models …).
    /// `default` so a cache file written by an older build still deserializes.
    #[serde(default)]
    pub extras: Vec<QuotaExtra>,
    /// Only for `rate_limited`: when a new attempt is allowed (RFC 3339 UTC).
    /// The provider fills in what the server asked for (`Retry-After`); the
    /// scheduler replaces it with the time it will actually try again.
    #[serde(default)]
    pub next_attempt_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub providers: Vec<ProviderQuota>,
    pub generated_at: String,
}

// ---------- settings ----------

/// The screen edge the bar is docked to. `Left`/`Right` make it a vertical
/// pill, `Top`/`Bottom` a horizontal strip.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    /// True for `Top`/`Bottom`: the bar runs along the x axis, so the
    /// "position along the edge" settings apply horizontally.
    pub fn is_horizontal(self) -> bool {
        matches!(self, Edge::Top | Edge::Bottom)
    }
}

/// Position **along** the docked edge. The wire values are historical (the bar
/// used to be vertical only): `Top` means start (top of a left/right edge, left
/// of a top/bottom edge), `Bottom` means end. `vertical_offset` is added on top
/// of it, positive towards the end.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerticalAlign {
    Top,
    Center,
    Bottom,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RingMode {
    Concentric,
    Primary,
    All,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PercentMode {
    Used,
    Remaining,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Dark,
    Light,
    Auto,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceStyle {
    Glass,
    Solid,
    /// Dark sci-fi HUD painted entirely by the frontend; no native backdrop.
    Cyber,
}

/// Neon pair the `cyber` surface is painted with. Ignored by the other styles.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CyberAccent {
    /// cyan → magenta (the original HUD)
    Neon,
    /// phosphor green → lime
    Matrix,
    /// amber → orange, like an amber CRT
    Amber,
    /// ice blue → near-white
    Ice,
    /// purple → hot pink
    Synthwave,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettings {
    pub enabled: bool,
    /// Hide this provider from the *bar* only: it keeps being polled and stays
    /// in the dashboard, the history and the threshold warnings. `enabled`
    /// switches the provider off entirely.
    #[serde(default = "default_true")]
    pub show_in_sidebar: bool,
    pub order: i32,
}

fn default_true() -> bool {
    true
}

/// What the floating bar is allowed to draw. Everything switched off here is
/// still polled, still in the dashboard and still able to raise a warning —
/// see `src/lib/sidebar-items.ts` for the filtering rules.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct SidebarItems {
    /// account-wide 5-hour windows
    pub five_hour: bool,
    /// account-wide weekly windows
    pub weekly: bool,
    /// per-model / per-feature windows (any window with a `scope`); replaces
    /// the deprecated top-level `showScopedRing`
    pub scoped: bool,
    /// account-wide windows that are neither 5-hour nor weekly
    pub other: bool,
    /// provider mark in the middle of a ring group
    pub logo: bool,
    /// percent under a ring group; replaces the deprecated top-level
    /// `showPercentLabel`
    pub percent_label: bool,
    /// the "⋯" button (a grip is still drawn when nothing else is left)
    pub more_button: bool,
}

impl Default for SidebarItems {
    fn default() -> Self {
        SidebarItems {
            five_hour: true,
            weekly: true,
            scoped: true,
            other: true,
            logo: true,
            percent_label: true,
            more_button: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Thresholds {
    pub warn: f64,
    pub critical: f64,
}

/// User-tunable colours (CSS hex strings; empty = theme default).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ColorSettings {
    pub claude: String,
    pub codex: String,
    pub warn: String,
    pub critical: String,
    pub surface: String,
    pub text: String,
}

impl Default for ColorSettings {
    fn default() -> Self {
        ColorSettings {
            claude: "#ff5c1a".into(),
            codex: "#10a37f".into(),
            warn: "#f5c542".into(),
            critical: "#ff3b30".into(),
            surface: "".into(),
            text: "".into(),
        }
    }
}

/// User-tunable geometry (CSS px at scale 1).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SizeSettings {
    pub ring_size: f64,
    pub ring_stroke: f64,
    pub bar_gap: f64,
    pub bar_padding: f64,
    pub corner_radius: f64,
    pub label_size: f64,
}

impl Default for SizeSettings {
    fn default() -> Self {
        SizeSettings {
            ring_size: 56.0,
            ring_stroke: 4.5,
            bar_gap: 18.0,
            bar_padding: 10.0,
            corner_radius: 26.0,
            label_size: 13.0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    /// "auto" | "en" | "zh-CN"
    pub language: String,
    pub theme: Theme,
    pub surface_style: SurfaceStyle,
    /// Only meaningful while `surface_style` is `cyber`.
    pub cyber_accent: CyberAccent,
    pub edge: Edge,
    /// Position along the docked edge (see [`VerticalAlign`]).
    pub vertical_align: VerticalAlign,
    /// Offset along the docked edge in px, positive towards its end (down on a
    /// left/right edge, right on a top/bottom edge).
    pub vertical_offset: i32,
    pub monitor: Option<String>,
    pub auto_hide: bool,
    pub auto_hide_delay_ms: u64,
    pub collapsed_width: u32,
    pub ring_mode: RingMode,
    /// Deprecated, mirrors `sidebar_items.scoped` (kept so older builds and
    /// hand-written settings files keep working).
    pub show_scoped_ring: bool,
    pub percent_mode: PercentMode,
    /// Deprecated, mirrors `sidebar_items.percent_label`.
    pub show_percent_label: bool,
    pub sidebar_items: SidebarItems,
    pub refresh_interval_sec: u64,
    /// Stretch the polling period for providers whose session logs have been
    /// quiet for a while (see `scheduler::poll_interval_secs`).
    pub adaptive_refresh: bool,
    pub providers: BTreeMap<String, ProviderSettings>,
    pub ingest_enabled: bool,
    /// Opt-in https URL of a pricing table to refresh from (empty = off; the
    /// app makes no third-party request while it is empty).
    pub pricing_url: String,
    /// Monthly *estimated* cost budget in USD; 0 turns the budget line off.
    pub monthly_budget_usd: f64,
    pub autostart: bool,
    pub opacity: f64,
    pub scale: f64,
    pub thresholds: Thresholds,
    pub colors: ColorSettings,
    pub sizes: SizeSettings,
    pub notifications: bool,
    /// Warn when a window is on pace to run out before it resets.
    pub forecast_notifications: bool,
    /// Mask account e-mails everywhere they render (screen sharing).
    pub hide_account_email: bool,
    pub always_on_top: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let mut providers = BTreeMap::new();
        providers.insert(
            "claude".to_string(),
            ProviderSettings {
                enabled: true,
                show_in_sidebar: true,
                order: 0,
            },
        );
        providers.insert(
            "codex".to_string(),
            ProviderSettings {
                enabled: true,
                show_in_sidebar: true,
                order: 1,
            },
        );
        Settings {
            version: 1,
            language: "auto".into(),
            theme: Theme::Dark,
            surface_style: SurfaceStyle::Glass,
            cyber_accent: CyberAccent::Neon,
            edge: Edge::Right,
            vertical_align: VerticalAlign::Center,
            vertical_offset: 0,
            monitor: None,
            auto_hide: false,
            auto_hide_delay_ms: 800,
            collapsed_width: 6,
            ring_mode: RingMode::Concentric,
            show_scoped_ring: true,
            percent_mode: PercentMode::Used,
            show_percent_label: true,
            sidebar_items: SidebarItems::default(),
            refresh_interval_sec: 60,
            adaptive_refresh: true,
            providers,
            ingest_enabled: true,
            pricing_url: String::new(),
            monthly_budget_usd: 0.0,
            autostart: false,
            opacity: 1.0,
            scale: 1.0,
            thresholds: Thresholds {
                warn: 70.0,
                critical: 90.0,
            },
            colors: ColorSettings::default(),
            sizes: SizeSettings::default(),
            notifications: false,
            forecast_notifications: true,
            hide_account_email: false,
            always_on_top: true,
        }
    }
}

// ---------- history ----------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    Hour,
    Day,
    Week,
    Month,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    pub from: String,
    pub to: String,
    pub bucket: Bucket,
    #[serde(default)]
    pub group_by_model: bool,
    #[serde(default)]
    pub provider: Option<String>,
    /// Exact cwd, or an empty string for requests with no project. None = all.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub group_by_project: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenTotals {
    pub input_tokens: i64,
    pub cache_write_tokens: i64,
    pub cache_read_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub total_tokens: i64,
    pub requests: i64,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRow {
    pub bucket_start: String,
    pub provider: String,
    pub model: Option<String>,
    /// None only for cross-project aggregation; empty means unassigned.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(flatten)]
    pub totals: TokenTotals,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryResult {
    pub rows: Vec<HistoryRow>,
    pub totals: TokenTotals,
    pub by_provider: BTreeMap<String, TokenTotals>,
    /// Projects in the selected time/provider range, before project filtering.
    #[serde(default)]
    pub projects: Vec<String>,
    /// At least one cost came from an approximate family match, not from a
    /// price for that exact model (ARCHITECTURE §9).
    #[serde(default)]
    pub cost_approximate: bool,
}

/// Activity calendar / punch card over one time range. See §5.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarQuery {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub provider: Option<String>,
    /// Same semantics as `HistoryQuery::project`.
    #[serde(default)]
    pub project: Option<String>,
}

/// One local calendar day with activity. Days without events are omitted.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarDay {
    /// Local `YYYY-MM-DD`.
    pub date: String,
    #[serde(flatten)]
    pub totals: TokenTotals,
}

/// One weekday × hour-of-day cell of the punch card (local time).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSlot {
    /// 0 = Monday … 6 = Sunday.
    pub weekday: u8,
    /// 0..=23 local hour.
    pub hour: u8,
    #[serde(flatten)]
    pub totals: TokenTotals,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarResult {
    pub days: Vec<CalendarDay>,
    pub slots: Vec<CalendarSlot>,
    pub totals: TokenTotals,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionQuery {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub provider: Option<String>,
    /// Same semantics as `HistoryQuery::project`.
    #[serde(default)]
    pub project: Option<String>,
    /// Server-side cap on the returned rows (default 200, clamped to 1..=1000).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Counters and identifiers only — never prompt or response text.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    /// Provider session id; an empty string groups events that carry none.
    pub session_id: String,
    pub provider: String,
    /// Exact cwd of the session's last event in range; empty = unassigned.
    pub project: String,
    /// RFC 3339 with the local offset, first/last event **inside the range**.
    pub first_ts: String,
    pub last_ts: String,
    pub duration_ms: i64,
    /// Distinct model names used, sorted.
    pub models: Vec<String>,
    #[serde(flatten)]
    pub totals: TokenTotals,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionsResult {
    /// Top sessions by total tokens, at most `limit` of them.
    pub rows: Vec<SessionRow>,
    /// Sessions in range before the cap was applied.
    pub total_sessions: i64,
    /// Totals over every session in range, not only the returned ones.
    pub totals: TokenTotals,
    pub truncated: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaHistoryQuery {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub provider: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSample {
    pub provider: String,
    pub kind: WindowKind,
    pub scope: Option<String>,
    pub used_percent: f64,
    pub resets_at: Option<String>,
    pub plan: Option<String>,
    pub ts: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IngestStats {
    pub files_scanned: u64,
    pub files_updated: u64,
    pub events_added: u64,
    pub duration_ms: u64,
    pub errors: Vec<String>,
    pub running: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub display_name: String,
    pub logged_in: bool,
    pub credential_path: Option<String>,
    pub log_path: Option<String>,
    pub plan_label: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PricingEntry {
    pub model_pattern: String,
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub cache_write_per_m: f64,
    pub cache_read_per_m: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PricingTable {
    pub entries: Vec<PricingEntry>,
    pub updated_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub config_dir: String,
    /// "linux" | "macos" | "windows" | "unknown"
    pub platform: String,
    /// "x11" | "wayland" | "cocoa" | "win32"
    pub backend: String,
}

// ---------- platform / window ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PopoverRequest {
    pub provider: String,
    pub ring_index: usize,
    /// Ring centre **y** in CSS px relative to the sidebar window. Used when
    /// the bar is docked to a left/right edge.
    pub anchor_y: f64,
    /// Ring centre **x** in CSS px relative to the sidebar window, used when
    /// the bar is docked to a top/bottom edge. Absent in requests from older
    /// frontends, which only ever ran with a vertical bar.
    #[serde(default)]
    pub anchor_x: Option<f64>,
    #[serde(default)]
    pub window_kind: Option<WindowKind>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SidebarState {
    pub expanded: bool,
    pub pinned: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    pub is_primary: bool,
}

/// Event names (Rust → JS). Keep in sync with docs/ARCHITECTURE.md §5.
pub mod events {
    pub const SNAPSHOT_UPDATED: &str = "snapshot-updated";
    pub const SETTINGS_UPDATED: &str = "settings-updated";
    pub const INGEST_PROGRESS: &str = "ingest-progress";
    pub const POPOVER_TARGET: &str = "popover-target";
    pub const SIDEBAR_STATE: &str = "sidebar-state";
    pub const DASHBOARD_NAVIGATE: &str = "dashboard-navigate";
}

/// Window labels.
pub mod windows {
    pub const SIDEBAR: &str = "sidebar";
    pub const POPOVER: &str = "popover";
    pub const DASHBOARD: &str = "dashboard";
}
