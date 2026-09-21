//! GitHub Copilot quota provider. [BACKEND]
//!
//! **Experimental — never verified against a live account.** Everything here
//! was written from published source code (see docs/PROVIDERS.md §1); the
//! machine it was written on has no Copilot installation, so the only path
//! exercised by a test against reality is "no credentials → not logged in".
//! Every fixture in the tests below is *from source, not captured live*.
//!
//! The credential is the OAuth token the Copilot editor plugins write to
//! `<config>/github-copilot/apps.json` (older: `hosts.json`), and the quota
//! call is the same `GET /copilot_internal/user` those editors make. The
//! endpoint is undocumented — exactly like the two endpoints the Claude and
//! Codex providers already use — but GitHub documents the same data through
//! the Copilot SDK's `account.getQuota()`, which is the corroboration that
//! made it worth shipping at all.
//!
//! Deliberately *not* implemented: the GitHub CLI (`~/.config/gh/hosts.yml`)
//! fallback other clients have. That token is a general-purpose GitHub
//! credential, not one Copilot tooling left for Copilot's own API client.

use super::{
    clamp_percent, degraded, empty_quota, mark_primary, normalize_rfc3339, now_rfc3339,
    null_default, write_cache, Provider, ProviderCtx, COPILOT_ID,
};
use crate::model::{
    DataSource, ProviderInfo, ProviderQuota, ProviderStatus, QuotaWindow, WindowKind,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const DISPLAY_NAME: &str = "GitHub Copilot";
pub const USAGE_URL: &str = "https://api.github.com/copilot_internal/user";

/// Header values are the ones a real Copilot client sends; the endpoint has
/// been observed to reject requests that do not look like an editor.
pub const EDITOR_VERSION: &str = "vscode/1.96.2";
pub const EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.26.7";
pub const COPILOT_USER_AGENT: &str = "GitHubCopilotChat/0.26.7";
pub const GITHUB_API_VERSION: &str = "2025-04-01";

pub const NOT_LOGGED_IN: &str =
    "Not signed in — sign in to GitHub Copilot in your editor to see quotas";
pub const EXPIRED_MESSAGE: &str = "GitHub token rejected — sign in to Copilot in your editor again";
/// Org-managed (token-based-billing) seats report no per-seat quota at all.
pub const NO_SEAT_QUOTA: &str =
    "This Copilot seat reports no personal quota (organisation-managed seat)";

// ---------- credentials ----------

/// `<config>/github-copilot`, resolved the way the Copilot plugins do.
///
/// Not `dirs::config_dir()`: that is `~/Library/Application Support` on macOS
/// and the *roaming* `%APPDATA%` on Windows, and the plugins use neither.
pub fn config_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.trim().is_empty() {
            return Some(PathBuf::from(xdg).join("github-copilot"));
        }
    }
    if cfg!(target_os = "windows") {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            if !local.trim().is_empty() {
                return Some(PathBuf::from(local).join("github-copilot"));
            }
        }
    }
    super::home_dir().map(|h| h.join(".config").join("github-copilot"))
}

/// `apps.json` first (current), then `hosts.json` (older).
pub fn credential_candidates_in(dir: &Path) -> Vec<PathBuf> {
    vec![dir.join("apps.json"), dir.join("hosts.json")]
}

/// Same, rooted at the real config directory; empty when there is no home dir.
pub fn credential_candidates() -> Vec<PathBuf> {
    config_dir()
        .map(|d| credential_candidates_in(&d))
        .unwrap_or_default()
}

/// The credential file that currently exists, if any.
pub fn credentials_path() -> Option<PathBuf> {
    credential_candidates().into_iter().find(|p| p.is_file())
}

/// `true` when a github.com OAuth token can be read from disk. Consulted by
/// `settings::clamp` to decide whether the provider starts switched on.
pub fn has_credentials() -> bool {
    config_dir().and_then(|d| load_oauth_token_in(&d)).is_some()
}

/// One host entry of `apps.json` / `hosts.json`.
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
struct HostEntry {
    oauth_token: Option<String>,
}

/// The github.com OAuth token from the first credential file in `dir` that
/// has one.
///
/// The file is a JSON object keyed by host: `"github.com"` in `hosts.json`,
/// `"github.com:<appId>"` in `apps.json`. Only github.com keys are accepted —
/// a GitHub Enterprise entry's token must never be sent to api.github.com.
pub fn load_oauth_token_in(dir: &Path) -> Option<String> {
    credential_candidates_in(dir)
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .find_map(|text| oauth_token_from_json(&text))
}

/// Pure half of [`load_oauth_token`], so the host matching is testable.
pub fn oauth_token_from_json(text: &str) -> Option<String> {
    let hosts: BTreeMap<String, serde_json::Value> = serde_json::from_str(text).ok()?;
    hosts
        .iter()
        .filter(|(host, _)| *host == "github.com" || host.starts_with("github.com:"))
        .filter_map(|(_, value)| serde_json::from_value::<HostEntry>(value.clone()).ok())
        .filter_map(|entry| entry.oauth_token)
        .map(|token| token.trim().to_string())
        .find(|token| !token.is_empty())
}

// ---------- API response shape ----------
//
// Undocumented and drifting: buckets come and go, `quota_snapshots` did not
// always exist, and a field that is absent today may arrive as an explicit
// `null` tomorrow. Everything that is not an `Option` therefore goes through
// `null_default`, so one odd field can never discard the whole response.

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct UsageResponse {
    pub copilot_plan: Option<String>,
    pub access_type_sku: Option<String>,
    pub quota_reset_date: Option<String>,
    /// older free-tier shape, predates `quota_snapshots`
    pub limited_user_reset_date: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub quota_snapshots: BTreeMap<String, QuotaSnapshot>,
    #[serde(deserialize_with = "null_default")]
    pub limited_user_quotas: BTreeMap<String, f64>,
    #[serde(deserialize_with = "null_default")]
    pub monthly_quotas: BTreeMap<String, f64>,
    /// marks an organisation-managed seat with no personal allotment
    #[serde(deserialize_with = "null_default")]
    pub token_based_billing: bool,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct QuotaSnapshot {
    pub entitlement: Option<f64>,
    pub remaining: Option<f64>,
    pub percent_remaining: Option<f64>,
    #[serde(deserialize_with = "null_default")]
    pub unlimited: bool,
}

impl QuotaSnapshot {
    /// Used percent for this bucket, or `None` when it carries no real meter.
    ///
    /// Suppressed for an `unlimited` bucket, for GitHub's `-1` sentinel (what
    /// paid plans send for chat/completions under usage-based billing) and for
    /// a zero entitlement — an organisation-managed placeholder, or `premium`
    /// on a free account. Rendering any of those as "0 % used" would be a lie.
    pub fn used_percent(&self) -> Option<f64> {
        if self.unlimited || self.entitlement == Some(-1.0) || self.remaining == Some(-1.0) {
            return None;
        }
        if self.entitlement == Some(0.0) {
            return None;
        }
        if let Some(percent_remaining) = self.percent_remaining {
            return Some(clamp_percent(100.0 - percent_remaining));
        }
        match (self.entitlement, self.remaining) {
            (Some(entitlement), Some(remaining)) if entitlement > 0.0 => {
                Some(clamp_percent(100.0 - (remaining / entitlement) * 100.0))
            }
            _ => None,
        }
    }
}

// ---------- mapping ----------

/// Buckets in display order; the first one that has a meter becomes primary.
/// `premium_interactions` is the headline pool ("Credits" in GitHub's UI).
const BUCKETS: [(&str, &str); 3] = [
    ("premium_interactions", "Premium requests"),
    ("chat", "Chat"),
    ("completions", "Completions"),
];

/// `"2099-07-01"` or `"2099-01-15T00:00:00Z"` → RFC 3339 UTC.
///
/// Paid plans send a datetime, free plans a bare date; both have been seen in
/// the same field, so both are accepted and anything else is dropped.
pub fn parse_reset_date(raw: Option<&str>) -> Option<String> {
    let value = raw?.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(normalized) = normalize_rfc3339(value) {
        return Some(normalized);
    }
    let date = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    Some(
        date.and_hms_opt(0, 0, 0)?
            .and_utc()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )
}

/// Every bucket that reports a real meter, as account-wide monthly windows.
///
/// The quota period is a calendar month, so `window_seconds` stays `None` and
/// the kind is `other`: nothing here is a 5-hour or weekly window, and
/// pretending otherwise would put Copilot on the wrong ring.
pub fn map_windows(usage: &UsageResponse) -> Vec<QuotaWindow> {
    let resets_at = parse_reset_date(usage.quota_reset_date.as_deref())
        .or_else(|| parse_reset_date(usage.limited_user_reset_date.as_deref()));

    let mut windows: Vec<QuotaWindow> = BUCKETS
        .iter()
        .filter_map(|(key, label)| {
            let used = usage.quota_snapshots.get(*key)?.used_percent()?;
            Some(window(label, used, resets_at.clone()))
        })
        .collect();

    // Legacy free-tier shape: `limited_user_quotas` is what is *left* of
    // `monthly_quotas`. Only consulted when `quota_snapshots` produced nothing,
    // so a paid account that still carries both does not sprout free-tier bars.
    if windows.is_empty() {
        for (key, label) in BUCKETS {
            let (Some(remaining), Some(total)) = (
                usage.limited_user_quotas.get(key),
                usage.monthly_quotas.get(key),
            ) else {
                continue;
            };
            if *total <= 0.0 {
                continue;
            }
            windows.push(window(
                label,
                clamp_percent(100.0 - (remaining / total) * 100.0),
                resets_at.clone(),
            ));
        }
    }

    mark_primary(&mut windows);
    windows
}

fn window(label: &str, used_percent: f64, resets_at: Option<String>) -> QuotaWindow {
    QuotaWindow {
        kind: WindowKind::Other,
        label: format!("{label} · Monthly"),
        window_seconds: None,
        used_percent,
        resets_at,
        scope: None,
        is_primary: false,
    }
}

/// `"pro"` → `"Copilot Pro"`. Values seen in published clients: `pro`,
/// `business`, `enterprise`, `individual` (which is what a free seat reports,
/// distinguished by `access_type_sku`).
pub fn plan_label(plan: Option<&str>, sku: Option<&str>) -> Option<String> {
    let raw = plan?.trim();
    if raw.is_empty() {
        return None;
    }
    let free = sku.map(|s| s.contains("free")).unwrap_or(false);
    Some(match (raw, free) {
        ("individual", true) => "Copilot Free".to_string(),
        ("individual", false) => "Copilot Individual".to_string(),
        ("free", _) => "Copilot Free".to_string(),
        ("pro", _) => "Copilot Pro".to_string(),
        ("pro_plus", _) | ("proplus", _) => "Copilot Pro+".to_string(),
        ("business", _) => "Copilot Business".to_string(),
        ("enterprise", _) => "Copilot Enterprise".to_string(),
        (other, _) => format!("Copilot {other}"),
    })
}

// ---------- provider ----------

pub struct CopilotProvider {
    ctx: ProviderCtx,
    /// Resolved once at construction so a test can point it at a temp dir
    /// instead of mutating process-wide environment variables.
    config_dir: Option<PathBuf>,
}

impl CopilotProvider {
    pub fn new(ctx: ProviderCtx) -> Self {
        Self {
            ctx,
            config_dir: config_dir(),
        }
    }

    #[cfg(test)]
    fn rooted_at(ctx: ProviderCtx, config_dir: PathBuf) -> Self {
        Self {
            ctx,
            config_dir: Some(config_dir),
        }
    }

    fn token(&self) -> Option<String> {
        self.config_dir.as_deref().and_then(load_oauth_token_in)
    }

    fn credential_path(&self) -> Option<PathBuf> {
        let dir = self.config_dir.as_deref()?;
        let candidates = credential_candidates_in(dir);
        // the file that exists, else where it would be looked for first
        candidates
            .iter()
            .find(|p| p.is_file())
            .cloned()
            .or_else(|| candidates.into_iter().next())
    }
}

#[async_trait]
impl Provider for CopilotProvider {
    fn id(&self) -> &'static str {
        COPILOT_ID
    }

    fn display_name(&self) -> &'static str {
        DISPLAY_NAME
    }

    fn experimental(&self) -> bool {
        true
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: COPILOT_ID.into(),
            display_name: DISPLAY_NAME.into(),
            logged_in: self.token().is_some(),
            credential_path: self.credential_path().map(|p| p.display().to_string()),
            // no Copilot client is known to log token counts locally
            log_path: None,
            plan_label: None,
            experimental: true,
        }
    }

    async fn fetch(&self, http: &reqwest::Client) -> ProviderQuota {
        let Some(token) = self.token() else {
            let mut q = empty_quota(COPILOT_ID, DISPLAY_NAME, ProviderStatus::NotLoggedIn);
            q.error = Some(NOT_LOGGED_IN.into());
            return q;
        };

        let request = http
            .get(USAGE_URL)
            // `token`, not `Bearer`: this endpoint takes the OAuth token as-is
            .header("Authorization", format!("token {token}"))
            .header("Accept", "application/json")
            .header("Editor-Version", EDITOR_VERSION)
            .header("Editor-Plugin-Version", EDITOR_PLUGIN_VERSION)
            .header("User-Agent", COPILOT_USER_AGENT)
            .header("X-Github-Api-Version", GITHUB_API_VERSION);

        let resp = match request.send().await {
            Ok(r) => r,
            Err(e) => {
                log::warn!("copilot usage request failed: {e}");
                return degraded(
                    &self.ctx,
                    COPILOT_ID,
                    DISPLAY_NAME,
                    ProviderStatus::Error,
                    format!("Could not reach api.github.com: {e}"),
                );
            }
        };
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return degraded(
                &self.ctx,
                COPILOT_ID,
                DISPLAY_NAME,
                ProviderStatus::TokenExpired,
                EXPIRED_MESSAGE,
            );
        }
        if !status.is_success() {
            return degraded(
                &self.ctx,
                COPILOT_ID,
                DISPLAY_NAME,
                ProviderStatus::Error,
                format!("Copilot usage API returned HTTP {}", status.as_u16()),
            );
        }
        // Read the body first: `Response::json` reports a schema mismatch as an
        // opaque "error decoding response body" without naming the field.
        let body = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return degraded(
                    &self.ctx,
                    COPILOT_ID,
                    DISPLAY_NAME,
                    ProviderStatus::Error,
                    format!("Could not read the Copilot usage response: {e}"),
                );
            }
        };
        let usage: UsageResponse = match serde_json::from_slice(&body) {
            Ok(u) => u,
            Err(e) => {
                log::warn!("copilot usage response has an unexpected shape: {e}");
                return degraded(
                    &self.ctx,
                    COPILOT_ID,
                    DISPLAY_NAME,
                    ProviderStatus::Error,
                    format!("Unexpected usage response: {e}"),
                );
            }
        };

        let mut quota = empty_quota(COPILOT_ID, DISPLAY_NAME, ProviderStatus::Ok);
        quota.windows = map_windows(&usage);
        quota.plan = usage.copilot_plan.clone();
        quota.plan_label = plan_label(
            usage.copilot_plan.as_deref(),
            usage.access_type_sku.as_deref(),
        );
        quota.source = DataSource::Api;
        quota.fetched_at = now_rfc3339();
        // An organisation-managed seat legitimately has no personal meter: say
        // so instead of leaving a blank ring with no explanation.
        if quota.windows.is_empty() && usage.token_based_billing {
            quota.error = Some(NO_SEAT_QUOTA.into());
        }
        write_cache(&self.ctx, &quota);
        quota
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Fixtures below are FROM SOURCE, NOT CAPTURED LIVE. ----
    // They are the bodies asserted on by openusage's own Copilot tests
    // (Tests/OpenUsageTests/CopilotProviderTests.swift @ 7caf4ca) and the
    // response interface declared by ericc-ch/copilot-api. See
    // docs/PROVIDERS.md §1.2–§1.3 for the links.

    /// A paid plan: metered premium pool, chat present but generous.
    const PAID: &str = r#"{
      "copilot_plan": "pro",
      "quota_reset_date": "2099-01-15T00:00:00Z",
      "quota_snapshots": {
        "premium_interactions": {"entitlement": 300, "remaining": 123, "percent_remaining": 41, "quota_id": "premium"},
        "chat": {"entitlement": 1000, "remaining": 950, "percent_remaining": 95, "quota_id": "chat"}
      }
    }"#;

    /// A free `individual` seat: real chat/completions counts, premium is a
    /// zero-entitlement placeholder, and every bucket is token-based-billing.
    const FREE: &str = r#"{
      "copilot_plan": "individual",
      "access_type_sku": "free_limited_copilot",
      "token_based_billing": true,
      "quota_reset_date": "2099-07-01",
      "quota_snapshots": {
        "chat": {"entitlement": 200, "remaining": 182, "percent_remaining": 91.0, "overage_permitted": false, "token_based_billing": true},
        "completions": {"entitlement": 2000, "remaining": 1989, "percent_remaining": 99.4, "overage_permitted": false, "token_based_billing": true},
        "premium_interactions": {"entitlement": 0, "remaining": 0, "percent_remaining": 0.0, "overage_permitted": false, "token_based_billing": true}
      }
    }"#;

    #[test]
    fn paid_plan_meters_the_premium_pool_first() {
        let usage: UsageResponse = serde_json::from_str(PAID).unwrap();
        let w = map_windows(&usage);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].label, "Premium requests · Monthly");
        assert_eq!(w[0].used_percent, 59.0);
        assert_eq!(w[0].kind, WindowKind::Other);
        assert_eq!(w[0].window_seconds, None);
        assert_eq!(w[0].resets_at.as_deref(), Some("2099-01-15T00:00:00Z"));
        assert!(w[0].is_primary, "the premium pool is the headline meter");
        assert_eq!(w[1].label, "Chat · Monthly");
        assert_eq!(w[1].used_percent, 5.0);
        assert!(!w[1].is_primary);
        assert_eq!(
            plan_label(usage.copilot_plan.as_deref(), None).as_deref(),
            Some("Copilot Pro")
        );
    }

    #[test]
    fn free_plan_drops_the_zero_entitlement_placeholder() {
        let usage: UsageResponse = serde_json::from_str(FREE).unwrap();
        let w = map_windows(&usage);
        assert_eq!(w.len(), 2, "premium_interactions has no allotment to show");
        assert_eq!(w[0].label, "Chat · Monthly");
        assert!((w[0].used_percent - 9.0).abs() < 1e-9);
        assert!(w[0].is_primary);
        assert_eq!(w[1].label, "Completions · Monthly");
        assert!((w[1].used_percent - 0.6).abs() < 1e-9);
        assert_eq!(
            w[0].resets_at.as_deref(),
            Some("2099-07-01T00:00:00Z"),
            "a bare date is accepted as midnight UTC"
        );
        assert_eq!(
            plan_label(
                usage.copilot_plan.as_deref(),
                usage.access_type_sku.as_deref()
            )
            .as_deref(),
            Some("Copilot Free")
        );
    }

    #[test]
    fn unlimited_buckets_and_the_minus_one_sentinel_are_suppressed() {
        let usage: UsageResponse = serde_json::from_str(
            r#"{
              "copilot_plan": "business",
              "quota_snapshots": {
                "premium_interactions": {"entitlement": 300, "remaining": 123, "percent_remaining": 41},
                "chat": {"unlimited": true, "entitlement": 0, "remaining": 0},
                "completions": {"entitlement": -1, "remaining": -1}
              }
            }"#,
        )
        .unwrap();
        let w = map_windows(&usage);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].label, "Premium requests · Monthly");
        assert_eq!(w[0].resets_at, None, "no reset date in this response");
    }

    #[test]
    fn an_org_managed_seat_reports_no_windows_at_all() {
        let usage: UsageResponse = serde_json::from_str(
            r#"{"copilot_plan":"business","token_based_billing":true,
                "quota_snapshots":{"premium_interactions":{"entitlement":0,"remaining":0}}}"#,
        )
        .unwrap();
        assert!(map_windows(&usage).is_empty());
        assert!(usage.token_based_billing);
        assert_eq!(
            plan_label(usage.copilot_plan.as_deref(), None).as_deref(),
            Some("Copilot Business")
        );
    }

    #[test]
    fn the_legacy_free_shape_is_read_only_when_snapshots_say_nothing() {
        let legacy: UsageResponse = serde_json::from_str(
            r#"{"copilot_plan":"individual",
                "limited_user_quotas":{"chat":250,"completions":2000},
                "monthly_quotas":{"chat":500,"completions":4000},
                "limited_user_reset_date":"2099-02-15"}"#,
        )
        .unwrap();
        let w = map_windows(&legacy);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].used_percent, 50.0);
        assert_eq!(w[1].used_percent, 50.0);
        assert_eq!(w[0].resets_at.as_deref(), Some("2099-02-15T00:00:00Z"));

        // A paid response that still carries the legacy keys must not sprout
        // free-tier bars next to its real ones.
        let both: UsageResponse = serde_json::from_str(
            r#"{"copilot_plan":"pro",
                "quota_snapshots":{"premium_interactions":{"entitlement":300,"remaining":123,"percent_remaining":41},
                                   "chat":{"entitlement":-1,"remaining":-1}},
                "limited_user_quotas":{"chat":100,"completions":1000},
                "monthly_quotas":{"chat":500,"completions":4000}}"#,
        )
        .unwrap();
        let w = map_windows(&both);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].label, "Premium requests · Monthly");
    }

    #[test]
    fn explicit_nulls_and_unknown_fields_do_not_discard_the_response() {
        let usage: UsageResponse = serde_json::from_str(
            r#"{
              "copilot_plan": "pro",
              "analytics_tracking_id": "x",
              "organization_list": [],
              "quota_snapshots": {"premium_interactions": {"entitlement": 10, "remaining": 4, "percent_remaining": null}},
              "limited_user_quotas": null,
              "monthly_quotas": null,
              "token_based_billing": null,
              "some_field_added_next_year": {"nested": true}
            }"#,
        )
        .unwrap();
        assert!(!usage.token_based_billing);
        assert!(usage.limited_user_quotas.is_empty());
        let w = map_windows(&usage);
        assert_eq!(
            w.len(),
            1,
            "percent_remaining:null falls back to the counts"
        );
        assert_eq!(w[0].used_percent, 60.0);
    }

    #[test]
    fn only_github_dot_com_tokens_are_accepted() {
        // apps.json: the host key carries the Copilot app id.
        assert_eq!(
            oauth_token_from_json(
                r#"{"github.com:Iv1.b507a08c87ecfe98":{"user":"octocat","oauth_token":"gho_apps"}}"#
            )
            .as_deref(),
            Some("gho_apps")
        );
        // hosts.json: the bare host key.
        assert_eq!(
            oauth_token_from_json(r#"{"github.com":{"user":"octocat","oauth_token":"gho_hosts"}}"#)
                .as_deref(),
            Some("gho_hosts")
        );
        // An Enterprise entry's token must never be sent to api.github.com.
        assert_eq!(
            oauth_token_from_json(r#"{"ghe.example.com":{"oauth_token":"gho_enterprise"}}"#),
            None
        );
        // Tolerate junk rather than blowing up the refresh.
        assert_eq!(oauth_token_from_json(r#"{"github.com":{}}"#), None);
        assert_eq!(
            oauth_token_from_json(r#"{"github.com":{"oauth_token":"  "}}"#),
            None
        );
        assert_eq!(oauth_token_from_json("not json"), None);
        assert_eq!(oauth_token_from_json("[]"), None);
    }

    #[test]
    fn reset_dates_accept_both_shapes_and_reject_junk() {
        assert_eq!(
            parse_reset_date(Some("2099-01-15T00:00:00Z")).as_deref(),
            Some("2099-01-15T00:00:00Z")
        );
        assert_eq!(
            parse_reset_date(Some("2099-07-01")).as_deref(),
            Some("2099-07-01T00:00:00Z")
        );
        assert_eq!(parse_reset_date(Some("")), None);
        assert_eq!(parse_reset_date(Some("next tuesday")), None);
        assert_eq!(parse_reset_date(None), None);
    }

    #[test]
    fn apps_json_wins_over_the_older_hosts_json() {
        let dir = crate::commands::test_support::tempdir();
        std::fs::write(
            dir.join("hosts.json"),
            r#"{"github.com":{"oauth_token":"old"}}"#,
        )
        .unwrap();
        assert_eq!(load_oauth_token_in(&dir).as_deref(), Some("old"));
        std::fs::write(
            dir.join("apps.json"),
            r#"{"github.com:Iv1.b507a08c87ecfe98":{"oauth_token":"new"}}"#,
        )
        .unwrap();
        assert_eq!(load_oauth_token_in(&dir).as_deref(), Some("new"));
        // An unreadable/garbled apps.json must fall through, not shadow.
        std::fs::write(dir.join("apps.json"), "{ truncated").unwrap();
        assert_eq!(load_oauth_token_in(&dir).as_deref(), Some("old"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The only behaviour that *can* be checked against reality here: with no
    /// credential file the provider reports "not signed in" and never opens a
    /// socket. Everything above this line is a fixture; this is a real test.
    #[tokio::test]
    async fn without_credentials_the_provider_is_not_logged_in() {
        let dir = crate::commands::test_support::tempdir();
        let provider = CopilotProvider::rooted_at(ProviderCtx::default(), dir.clone());

        let info = provider.info();
        assert!(!info.logged_in);
        assert!(info.experimental, "the badge must survive refactors");
        assert!(info.log_path.is_none(), "Copilot logs no token counts");
        assert_eq!(
            info.credential_path.as_deref(),
            Some(dir.join("apps.json").display().to_string().as_str()),
            "the path is reported even when the file is absent, so the UI can name it"
        );

        // No cache dir, so `fetch` cannot be answered from disk either; it has
        // to short-circuit on the missing credential before any network use.
        let quota = provider.fetch(&super::super::http_client("test")).await;
        assert_eq!(quota.status, ProviderStatus::NotLoggedIn);
        assert_eq!(quota.error.as_deref(), Some(NOT_LOGGED_IN));
        assert!(quota.windows.is_empty());
        assert_eq!(quota.provider, COPILOT_ID);
        assert_eq!(quota.display_name, DISPLAY_NAME);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_config_dir_is_never_the_roaming_or_application_support_one() {
        // Whatever the platform, the plugins use a `github-copilot` directory
        // under an XDG-style config root — not ~/Library/Application Support.
        let dir = config_dir().expect("a home directory exists in tests");
        assert!(dir.ends_with("github-copilot"), "{}", dir.display());
        assert!(!dir.to_string_lossy().contains("Application Support"));
    }
}
