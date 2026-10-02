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
    /// Id of the extra account this quota belongs to (`settings.accounts`);
    /// absent for the primary account, so a primary entry serializes exactly
    /// as it did before multi-account support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// User label of that extra account ("Work"); absent for the primary one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
}

impl ProviderQuota {
    /// Registry key: `"claude"` for the primary account, `"claude@work"` for
    /// an extra one. Everything keyed per provider (poll clocks, backoff,
    /// alert dedupe, ring keys, the CLI filter) uses this.
    pub fn key(&self) -> String {
        provider_key(&self.provider, self.account_id.as_deref())
    }
}

/// `provider` or `provider@account`.
pub fn provider_key(provider: &str, account_id: Option<&str>) -> String {
    match account_id.filter(|a| !a.is_empty()) {
        Some(a) => format!("{provider}@{a}"),
        None => provider.to_string(),
    }
}

/// Inverse of [`provider_key`]; the account part is `""` for the primary one.
pub fn split_key(key: &str) -> (&str, &str) {
    key.split_once('@').unwrap_or((key, ""))
}

/// One extra account of a provider (`settings.accounts`). The primary account
/// of each provider is implicit: the default config dir.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettings {
    /// Slug `[a-z0-9-]{1,24}`, unique across all accounts.
    pub id: String,
    /// `"claude"` | `"codex"`.
    pub provider: String,
    /// Shown as "Claude Code · <label>"; 1..=40 characters.
    pub label: String,
    /// Absolute path of the account's CLI config dir (`CLAUDE_CONFIG_DIR` /
    /// `CODEX_HOME` of that login).
    pub config_dir: String,
    /// Off = not polled; the entry stays in the list.
    #[serde(default = "default_true")]
    pub enabled: bool,
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

/// Where the percentage label is rendered for each ring in the sidebar.
/// `Below` preserves the original layout; `Center` uses the ring's center.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PercentPosition {
    #[default]
    Below,
    Center,
}

/// What the tray icon itself shows: only the glyph, or the busiest window's
/// percentage as well (menu-bar title on macOS, rendered into the icon on
/// Windows and Linux).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrayDisplay {
    #[default]
    Icon,
    Percent,
}

/// What the label next to / under each ring says. `Percent` is the original
/// behaviour; `Reset` is the countdown ("1h12"); `Both` shows both.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LabelContent {
    #[default]
    Percent,
    Reset,
    Both,
}

/// How a provider is drawn on the bar: concentric rings, or slim mini-bars.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RingStyle {
    #[default]
    Ring,
    Bar,
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

/// Wire format of the webhook channel.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WebhookKind {
    /// JSON `{title, body, provider, window, level, ts}`.
    #[default]
    Generic,
    /// Plain-text body, `Title` header (ntfy.sh and compatible servers).
    Ntfy,
    /// Slack-compatible incoming webhook, `{"text": …}`.
    Slack,
}

/// Second notification channel next to the native one. The URL may carry a
/// secret (a Slack token, an ntfy topic) and is therefore never logged.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct WebhookSettings {
    pub enabled: bool,
    /// `https://` only; empty = not configured.
    pub url: String,
    pub kind: WebhookKind,
}

/// Last week's usage, for the weekly-summary notification and Overview card.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklySummary {
    /// Local `YYYY-MM-DD` of the Monday the summarised week started on.
    pub week_start: String,
    /// Local `YYYY-MM-DD` of the Sunday it ended on.
    pub week_end: String,
    pub total_tokens: i64,
    pub requests: i64,
    /// Sum of priced requests (an estimate, never billing).
    pub estimated_cost_usd: Option<f64>,
    /// Local `YYYY-MM-DD` of the day with the most tokens, if any activity.
    pub busiest_day: Option<String>,
    pub busiest_day_tokens: i64,
    /// Quota windows (per provider/kind/scope/cycle) that reached 100 %.
    pub limits_hit: u32,
}

/// Existence check of a prospective extra account's folder (Accounts card).
/// No credential is read.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountCheck {
    /// The path is absolute (a relative one is never accepted).
    pub absolute: bool,
    pub dir_found: bool,
    /// The provider's credentials file exists in that folder.
    pub credentials_found: bool,
    /// Which file was looked for.
    pub credentials_file: String,
    /// macOS Claude: no credentials file, so this account cannot be read
    /// (its login would be in the Keychain, which is not consulted).
    pub keychain_only: bool,
}

/// Where a provider's CLI stands on this machine; no credential is read.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSetup {
    pub provider: String,
    /// The CLI's config directory (honouring `CLAUDE_CONFIG_DIR` / `CODEX_HOME`).
    pub config_dir: String,
    pub config_dir_found: bool,
    /// A credentials file exists (existence only, contents are never read).
    pub credentials_found: bool,
    /// Shell commands that sign in, in order.
    pub login_steps: Vec<String>,
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
    pub copilot: String,
    pub openrouter: String,
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
            copilot: "#8250df".into(),
            openrouter: "#6467f2".into(),
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
    /// The popover closes after this long without any pointer activity on the
    /// bar or the popover, whatever the hover flags say. 0 = never.
    pub popover_timeout_sec: u64,
    pub collapsed_width: u32,
    pub ring_mode: RingMode,
    /// Deprecated, mirrors `sidebar_items.scoped` (kept so older builds and
    /// hand-written settings files keep working).
    pub show_scoped_ring: bool,
    pub percent_mode: PercentMode,
    /// Render the percentage below the ring (legacy layout) or in its center.
    #[serde(default)]
    pub percent_position: PercentPosition,
    /// Percent, reset countdown or both in the sidebar label.
    #[serde(default)]
    pub label_content: LabelContent,
    /// Concentric rings (default) or compact mini-bars.
    #[serde(default)]
    pub ring_style: RingStyle,
    /// One-shot pulse / flash on threshold crossings and resets.
    #[serde(default = "default_true")]
    pub sidebar_animations: bool,
    /// Deprecated, mirrors `sidebar_items.percent_label`.
    pub show_percent_label: bool,
    pub sidebar_items: SidebarItems,
    pub refresh_interval_sec: u64,
    /// Stretch the polling period for providers whose session logs have been
    /// quiet for a while (see `scheduler::poll_interval_secs`).
    pub adaptive_refresh: bool,
    pub providers: BTreeMap<String, ProviderSettings>,
    /// Extra accounts (at most `MAX_ACCOUNTS`) of Claude Code / Codex, each
    /// with its own CLI config dir. The primary account stays implicit.
    /// Quota only: their local session logs are not ingested.
    #[serde(default)]
    pub accounts: Vec<AccountSettings>,
    pub ingest_enabled: bool,
    /// Optional https URL of a pricing table. Empty uses the project's
    /// published table, so people receive pricing revisions independently of
    /// app releases.
    pub pricing_url: String,
    /// Monthly *estimated* cost budget in USD; 0 turns the budget line off.
    pub monthly_budget_usd: f64,
    /// Name of the environment variable that holds the OpenRouter API key
    /// (experimental provider). Never the key itself.
    pub openrouter_key_env: String,
    /// What the user pays per month for each provider's subscription, in USD;
    /// 0 = unknown. Only used to compare the API-equivalent estimate with it.
    pub subscription_usd: BTreeMap<String, f64>,
    /// Delete quota samples older than this many days (0 = keep forever).
    /// Token usage events are never deleted.
    pub quota_retention_days: u32,
    pub autostart: bool,
    /// Ask GitHub once a day whether a newer release exists. Never installs
    /// anything on its own — the user always confirms (docs/RELEASING.md).
    pub auto_update_check: bool,
    /// Check the pricing-table source once a day. Checking only downloads and
    /// compares; applying remains an explicit action.
    pub auto_pricing_check: bool,
    /// Global shortcut that shows/hides the bar, e.g. `"Ctrl+Alt+U"`.
    /// Empty (the default) registers nothing, so no key is hijacked.
    pub shortcut_toggle_sidebar: String,
    /// Global shortcut that opens the dashboard; empty = disabled.
    pub shortcut_open_dashboard: String,
    pub opacity: f64,
    pub scale: f64,
    pub thresholds: Thresholds,
    pub colors: ColorSettings,
    pub sizes: SizeSettings,
    pub notifications: bool,
    /// Warn when a window is on pace to run out before it resets.
    pub forecast_notifications: bool,
    /// Warn when a window crosses `thresholds.warn` / `thresholds.critical`.
    pub threshold_notifications: bool,
    /// Warn when the month-to-date estimated cost reaches 80 % / 100 % of
    /// `monthly_budget_usd` (needs a budget > 0).
    pub budget_notifications: bool,
    /// Monday ~09:00 local: one notification summarising the previous week.
    pub weekly_summary: bool,
    /// Optional second notification channel (see `alerts::notifier`).
    pub webhook: WebhookSettings,
    /// Update version the user chose to skip; the banner stays quiet for it.
    pub skipped_version: String,
    /// App version whose release notes were last shown ("What's new").
    pub last_seen_version: String,
    /// The first-run wizard was completed or skipped. Settings files that
    /// already exist when this key is introduced count as onboarded.
    pub onboarded: bool,
    /// Focus / do-not-disturb: native notifications are suppressed until this
    /// epoch-ms instant. `0` = off, `-1` = until the user turns it off.
    pub focus_until: i64,
    /// While focus mode is active, also hide the sidebar window.
    pub focus_hides_sidebar: bool,
    /// Mask account e-mails everywhere they render (screen sharing).
    pub hide_account_email: bool,
    /// After every snapshot, write `snapshot.json` into the app data dir for
    /// scripts, status bars and `--print` (see `export_snapshot.rs`).
    pub export_snapshot: bool,
    /// Skip the *automatic* provider polling (ingestion of local logs keeps
    /// running). An explicit refresh still polls. Persisted on purpose.
    pub polling_paused: bool,
    /// `icon` (default) keeps the plain tray glyph; `percent` adds the busiest
    /// visible window's percentage (see `window/tray_status.rs`).
    #[serde(default)]
    pub tray_display: TrayDisplay,
    pub always_on_top: bool,
    /// The user's own presets, name → partial settings patch (at most 10).
    /// A patch goes through the normal merge when applied, so it is only
    /// stored shape-checked here (see `settings::clamp`).
    pub custom_presets: BTreeMap<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        // one entry per registered provider (providers::DEFAULT_PROVIDER_ORDER)
        let mut providers = BTreeMap::new();
        for (order, id) in crate::commands::providers::DEFAULT_PROVIDER_ORDER
            .iter()
            .enumerate()
        {
            providers.insert(
                (*id).to_string(),
                ProviderSettings {
                    enabled: crate::commands::providers::enabled_by_default(id),
                    show_in_sidebar: true,
                    order: order as i32,
                },
            );
        }
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
            popover_timeout_sec: 10,
            collapsed_width: 6,
            ring_mode: RingMode::Concentric,
            show_scoped_ring: true,
            percent_mode: PercentMode::Used,
            percent_position: PercentPosition::default(),
            label_content: LabelContent::default(),
            ring_style: RingStyle::default(),
            sidebar_animations: true,
            show_percent_label: true,
            sidebar_items: SidebarItems::default(),
            refresh_interval_sec: 60,
            adaptive_refresh: true,
            providers,
            accounts: Vec::new(),
            ingest_enabled: true,
            pricing_url: String::new(),
            monthly_budget_usd: 0.0,
            openrouter_key_env: "OPENROUTER_API_KEY".into(),
            subscription_usd: [("claude", 0.0), ("codex", 0.0)]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            quota_retention_days: 365,
            autostart: false,
            auto_update_check: true,
            auto_pricing_check: true,
            shortcut_toggle_sidebar: String::new(),
            shortcut_open_dashboard: String::new(),
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
            threshold_notifications: true,
            budget_notifications: true,
            weekly_summary: false,
            webhook: WebhookSettings::default(),
            skipped_version: String::new(),
            last_seen_version: String::new(),
            onboarded: false,
            focus_until: 0,
            focus_hides_sidebar: false,
            hide_account_email: false,
            export_snapshot: false,
            polling_paused: false,
            tray_display: TrayDisplay::default(),
            always_on_top: true,
            custom_presets: BTreeMap::new(),
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

/// Token usage of one provider inside each of several time windows (quota
/// cycles). Windows are `[from, to)` RFC 3339 instants.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowUsageQuery {
    pub provider: String,
    pub windows: Vec<TimeWindow>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimeWindow {
    pub from: String,
    pub to: String,
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
    /// Sum of priced records; None if no contributing record was priced.
    #[serde(default)]
    pub known_cost_usd: Option<f64>,
    /// Requests excluded from the known subtotal because no price matched.
    #[serde(default)]
    pub unpriced_requests: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRow {
    pub bucket_start: String,
    pub provider: String,
    pub model: Option<String>,
    /// Explicit effort recorded by the CLI; None for unknown or ungrouped rows.
    #[serde(default)]
    pub reasoning_effort: Option<String>,
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
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct ModelVariant {
    pub model: String,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
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
    /// Distinct model/effort pairs, sorted without changing the raw model ids.
    #[serde(default)]
    pub model_variants: Vec<ModelVariant>,
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
    /// `None` = every account; `Some("")` = the primary account only;
    /// `Some("work")` = that extra account.
    #[serde(default)]
    pub account: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSample {
    pub provider: String,
    /// Extra account id; absent for the primary account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
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
    /// Quota source implemented from the tool's published source but never
    /// verified against a live account; the UI badges it as such.
    #[serde(default)]
    pub experimental: bool,
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

/// State of the independently delivered pricing table. Unlike an application
/// update, a price-list revision never applies itself.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PriceUpdateStatus {
    pub available: bool,
    pub revision: Option<String>,
    pub checking: bool,
    pub applying: bool,
    pub checked_at: Option<String>,
    pub error: Option<String>,
    /// A complete `pricing.json` was explicitly saved by the user.
    pub custom_pricing: bool,
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

/// What the in-app updater currently knows. Never changes on its own without
/// the user asking — see `src-tauri/src/updater.rs` and docs/RELEASING.md.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// Version offered by the release feed; `None` when up to date.
    pub available: Option<String>,
    pub current_version: String,
    /// Release notes of the offered version, as published.
    pub notes: Option<String>,
    /// Release page, for builds we must not replace in place.
    pub release_url: String,
    /// False for `.deb`/`.rpm` and `scripts/install-linux.sh` installs: the
    /// package manager owns those files, so we only link to the release.
    pub can_install: bool,
    pub checking: bool,
    pub installing: bool,
    pub error: Option<String>,
    /// RFC 3339 UTC of the last completed check.
    pub checked_at: Option<String>,
    /// Bytes downloaded so far while `installing`.
    pub downloaded: u64,
    /// Total download size while `installing`, when the server says.
    pub total: Option<u64>,
}

/// Why a configured global shortcut is not active; `None` = registered (or
/// the setting is empty, which disables it).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    pub toggle_sidebar: Option<String>,
    pub open_dashboard: Option<String>,
}

/// What became of one configured global shortcut.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationState {
    /// Nothing configured.
    #[default]
    Off,
    Registered,
    /// Invalid, or the OS refused it (usually: another program owns the keys).
    Failed,
    /// This session cannot grab global keys at all (native Wayland).
    Unsupported,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutRegistration {
    pub state: RegistrationState,
    /// Why, for `failed` and `unsupported`.
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutRegistrations {
    pub toggle_sidebar: ShortcutRegistration,
    pub open_dashboard: ShortcutRegistration,
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
    pub const UPDATE_STATUS: &str = "update-status";
    pub const PRICE_UPDATE_STATUS: &str = "price-update-status";
}

/// Window labels.
pub mod windows {
    pub const SIDEBAR: &str = "sidebar";
    pub const POPOVER: &str = "popover";
    pub const DASHBOARD: &str = "dashboard";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_fills_every_missing_key_with_its_default() {
        let s: Settings = serde_json::from_str(r#"{"edge":"left"}"#).unwrap();
        assert_eq!(s.edge, Edge::Left);
        let d = Settings::default();
        assert_eq!(s.refresh_interval_sec, d.refresh_interval_sec);
        assert_eq!(s.quota_retention_days, 365);
        assert_eq!(s.providers, d.providers);
        let empty: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, d);
    }

    #[test]
    fn unknown_keys_from_a_newer_or_older_build_are_ignored() {
        let s: Settings = serde_json::from_str(
            r#"{"theme":"light","someFutureSetting":{"a":1},"removedLongAgo":true}"#,
        )
        .unwrap();
        assert_eq!(s.theme, Theme::Light);
        assert_eq!(s.edge, Settings::default().edge);
    }

    #[test]
    fn a_settings_value_survives_a_json_round_trip() {
        let s = Settings {
            quota_retention_days: 0,
            monthly_budget_usd: 12.5,
            ..Settings::default()
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let value = serde_json::to_value(&s).unwrap();
        assert_eq!(value["quotaRetentionDays"], 0, "keys are camelCase");
    }
}
