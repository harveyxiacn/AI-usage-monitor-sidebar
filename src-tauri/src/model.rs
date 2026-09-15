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
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStatus {
    Ok,
    NotLoggedIn,
    TokenExpired,
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
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub providers: Vec<ProviderQuota>,
    pub generated_at: String,
}

// ---------- settings ----------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    Left,
    Right,
}

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
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettings {
    pub enabled: bool,
    pub order: i32,
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
    pub edge: Edge,
    pub vertical_align: VerticalAlign,
    pub vertical_offset: i32,
    pub monitor: Option<String>,
    pub auto_hide: bool,
    pub auto_hide_delay_ms: u64,
    pub collapsed_width: u32,
    pub ring_mode: RingMode,
    pub show_scoped_ring: bool,
    pub percent_mode: PercentMode,
    pub show_percent_label: bool,
    pub refresh_interval_sec: u64,
    pub providers: BTreeMap<String, ProviderSettings>,
    pub ingest_enabled: bool,
    pub autostart: bool,
    pub opacity: f64,
    pub scale: f64,
    pub thresholds: Thresholds,
    pub colors: ColorSettings,
    pub sizes: SizeSettings,
    pub notifications: bool,
    pub always_on_top: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let mut providers = BTreeMap::new();
        providers.insert(
            "claude".to_string(),
            ProviderSettings {
                enabled: true,
                order: 0,
            },
        );
        providers.insert(
            "codex".to_string(),
            ProviderSettings {
                enabled: true,
                order: 1,
            },
        );
        Settings {
            version: 1,
            language: "auto".into(),
            theme: Theme::Dark,
            surface_style: SurfaceStyle::Glass,
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
            refresh_interval_sec: 60,
            providers,
            ingest_enabled: true,
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
    #[serde(flatten)]
    pub totals: TokenTotals,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryResult {
    pub rows: Vec<HistoryRow>,
    pub totals: TokenTotals,
    pub by_provider: BTreeMap<String, TokenTotals>,
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
    pub anchor_y: f64,
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
