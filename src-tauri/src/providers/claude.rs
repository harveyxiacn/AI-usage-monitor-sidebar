//! Claude Code (Anthropic OAuth) quota provider. [BACKEND]
//!
//! Credentials come from `$CLAUDE_CONFIG_DIR/.credentials.json` (default
//! `~/.claude/.credentials.json`); on macOS the same JSON lives in the Keychain
//! under the service name `Claude Code-credentials`.

use super::{
    clamp_percent, degraded, empty_quota, mark_primary, normalize_rfc3339, now_rfc3339,
    write_cache, Provider, ProviderCtx, CLAUDE_ID,
};
use crate::model::{
    AccountInfo, CreditsInfo, DataSource, ProviderInfo, ProviderQuota, ProviderStatus, QuotaWindow,
    WindowKind,
};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const DISPLAY_NAME: &str = "Claude Code";
pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
pub const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
pub const OAUTH_BETA: &str = "oauth-2025-04-20";

const FIVE_HOUR_SECS: u64 = 5 * 3600;
const SEVEN_DAY_SECS: u64 = 7 * 86_400;
/// The profile endpoint only carries slow-moving data (name, e-mail, tier).
const PROFILE_TTL: Duration = Duration::from_secs(30 * 60);

pub const EXPIRED_MESSAGE: &str = "Claude Code login expired — run `claude` to refresh";

// ---------- credentials ----------

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<OauthBlock>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
struct OauthBlock {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    /// unix **milliseconds**
    #[serde(rename = "expiresAt")]
    expires_at: Option<i64>,
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
    #[serde(rename = "rateLimitTier")]
    rate_limit_tier: Option<String>,
}

/// What the rest of the module needs from the credentials file.
#[derive(Clone, Debug, Default)]
pub struct Credentials {
    pub access_token: String,
    pub expires_at_ms: Option<i64>,
    pub subscription_type: Option<String>,
    pub rate_limit_tier: Option<String>,
}

impl Credentials {
    pub fn is_expired(&self, now_ms: i64) -> bool {
        self.expires_at_ms.map(|e| e <= now_ms).unwrap_or(false)
    }
}

/// `$CLAUDE_CONFIG_DIR` or `~/.claude`.
pub fn config_dir() -> Option<PathBuf> {
    match std::env::var("CLAUDE_CONFIG_DIR") {
        Ok(v) if !v.trim().is_empty() => Some(PathBuf::from(v)),
        _ => super::home_dir().map(|h| h.join(".claude")),
    }
}

pub fn credentials_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join(".credentials.json"))
}

/// `~/.claude/projects` — the session-log root.
pub fn log_root() -> Option<PathBuf> {
    config_dir().map(|d| d.join("projects"))
}

/// Read the credentials file (or, on macOS, the Keychain item).
pub fn load_credentials() -> Option<Credentials> {
    let text = read_credentials_text()?;
    parse_credentials(&text)
}

fn read_credentials_text() -> Option<String> {
    if let Some(path) = credentials_path() {
        match std::fs::read_to_string(&path) {
            Ok(t) => return Some(t),
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                log::warn!("cannot read {}: {}", path.display(), e);
            }
            Err(_) => {}
        }
    }
    keychain_credentials()
}

#[cfg(target_os = "macos")]
fn keychain_credentials() -> Option<String> {
    let out = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let text = text.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(not(target_os = "macos"))]
fn keychain_credentials() -> Option<String> {
    None
}

/// Parse the credentials JSON. Exposed for tests.
pub fn parse_credentials(text: &str) -> Option<Credentials> {
    let file: CredentialsFile = serde_json::from_str(text).ok()?;
    let block = file.claude_ai_oauth?;
    let access_token = block.access_token.filter(|t| !t.is_empty())?;
    Some(Credentials {
        access_token,
        expires_at_ms: block.expires_at,
        subscription_type: block.subscription_type,
        rate_limit_tier: block.rate_limit_tier,
    })
}

// ---------- API response shapes (tolerant: unknown fields ignored) ----------

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct UsageResponse {
    pub five_hour: Option<LegacyWindow>,
    pub seven_day: Option<LegacyWindow>,
    pub seven_day_opus: Option<LegacyWindow>,
    pub seven_day_sonnet: Option<LegacyWindow>,
    pub limits: Vec<LimitEntry>,
    pub extra_usage: Option<ExtraUsage>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct LegacyWindow {
    /// already 0..100
    pub utilization: Option<f64>,
    pub resets_at: Option<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct LimitEntry {
    /// `session` | `weekly_all` | `weekly_scoped` | …
    pub kind: String,
    pub group: Option<String>,
    /// already 0..100
    pub percent: Option<f64>,
    pub resets_at: Option<String>,
    pub scope: Option<LimitScope>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct LimitScope {
    pub model: Option<ScopeModel>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ScopeModel {
    pub id: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ExtraUsage {
    pub is_enabled: bool,
    pub monthly_limit: Option<f64>,
    pub used_credits: Option<f64>,
    pub utilization: Option<f64>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ProfileResponse {
    pub account: Option<ProfileAccount>,
    pub organization: Option<ProfileOrg>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ProfileAccount {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub full_name: Option<String>,
    pub has_claude_max: bool,
    pub has_claude_pro: bool,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ProfileOrg {
    pub organization_type: Option<String>,
    pub rate_limit_tier: Option<String>,
}

// ---------- mapping ----------

/// Map a usage response to quota windows, preferring the generic `limits[]`
/// array and falling back to the legacy top-level keys.
pub fn map_windows(usage: &UsageResponse) -> Vec<QuotaWindow> {
    let mut windows = map_limits(&usage.limits);
    if windows.is_empty() {
        windows = map_legacy(usage);
    }
    mark_primary(&mut windows);
    windows
}

fn map_limits(limits: &[LimitEntry]) -> Vec<QuotaWindow> {
    let mut out = Vec::new();
    for l in limits {
        let percent = clamp_percent(l.percent.unwrap_or(0.0));
        let resets_at = l.resets_at.as_deref().and_then(normalize_rfc3339);
        match l.kind.as_str() {
            "session" => out.push(QuotaWindow {
                kind: WindowKind::FiveHour,
                label: "5-hour".into(),
                window_seconds: Some(FIVE_HOUR_SECS),
                used_percent: percent,
                resets_at,
                scope: None,
                is_primary: false,
            }),
            "weekly_all" => out.push(QuotaWindow {
                kind: WindowKind::SevenDay,
                label: "Weekly".into(),
                window_seconds: Some(SEVEN_DAY_SECS),
                used_percent: percent,
                resets_at,
                scope: None,
                is_primary: false,
            }),
            "weekly_scoped" => {
                let scope = l
                    .scope
                    .as_ref()
                    .and_then(|s| s.model.as_ref())
                    .and_then(|m| m.display_name.clone().or_else(|| m.id.clone()))
                    .unwrap_or_else(|| "Scoped".to_string());
                out.push(QuotaWindow {
                    kind: WindowKind::SevenDay,
                    label: format!("Weekly · {scope}"),
                    window_seconds: Some(SEVEN_DAY_SECS),
                    used_percent: percent,
                    resets_at,
                    scope: Some(scope),
                    is_primary: false,
                });
            }
            other => log::debug!("claude: ignoring unknown limit kind `{other}`"),
        }
    }
    out
}

fn map_legacy(usage: &UsageResponse) -> Vec<QuotaWindow> {
    let mut out = Vec::new();
    let mut push = |w: &Option<LegacyWindow>,
                    kind: WindowKind,
                    secs: u64,
                    label: &str,
                    scope: Option<&str>| {
        let Some(w) = w else { return };
        let Some(util) = w.utilization else { return };
        out.push(QuotaWindow {
            kind,
            label: label.to_string(),
            window_seconds: Some(secs),
            used_percent: clamp_percent(util),
            resets_at: w.resets_at.as_deref().and_then(normalize_rfc3339),
            scope: scope.map(|s| s.to_string()),
            is_primary: false,
        });
    };
    push(
        &usage.five_hour,
        WindowKind::FiveHour,
        FIVE_HOUR_SECS,
        "5-hour",
        None,
    );
    push(
        &usage.seven_day,
        WindowKind::SevenDay,
        SEVEN_DAY_SECS,
        "Weekly",
        None,
    );
    push(
        &usage.seven_day_opus,
        WindowKind::SevenDay,
        SEVEN_DAY_SECS,
        "Weekly · Opus",
        Some("Opus"),
    );
    push(
        &usage.seven_day_sonnet,
        WindowKind::SevenDay,
        SEVEN_DAY_SECS,
        "Weekly · Sonnet",
        Some("Sonnet"),
    );
    out
}

/// Human plan name from the subscription type and the rate-limit tier.
pub fn plan_label(
    subscription_type: Option<&str>,
    rate_limit_tier: Option<&str>,
) -> Option<String> {
    match rate_limit_tier.unwrap_or("") {
        "default_claude_max_5x" => return Some("Claude Max 5x".into()),
        "default_claude_max_20x" => return Some("Claude Max 20x".into()),
        _ => {}
    }
    let sub = subscription_type?.trim();
    if sub.is_empty() {
        return None;
    }
    Some(match sub {
        "pro" => "Claude Pro".to_string(),
        "team" => "Claude Team".to_string(),
        "enterprise" => "Claude Enterprise".to_string(),
        other => format!("Claude {}", capitalize(other)),
    })
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn credits_from(extra: &Option<ExtraUsage>) -> Option<CreditsInfo> {
    let extra = extra.as_ref()?;
    if !extra.is_enabled {
        return None;
    }
    Some(CreditsInfo {
        has_credits: true,
        unlimited: extra.monthly_limit.is_none(),
        balance: match (extra.used_credits, extra.monthly_limit) {
            (Some(used), Some(limit)) => Some(format!("{:.2}/{:.2}", used, limit)),
            (Some(used), None) => Some(format!("{:.2}", used)),
            _ => extra.utilization.map(|u| format!("{:.0}%", u)),
        },
    })
}

// ---------- profile cache ----------

#[derive(Clone, Debug, Default)]
struct CachedProfile {
    account: Option<AccountInfo>,
    rate_limit_tier: Option<String>,
}

static PROFILE_CACHE: Mutex<Option<(Instant, CachedProfile)>> = Mutex::new(None);

fn cached_profile() -> Option<CachedProfile> {
    let guard = PROFILE_CACHE.lock();
    let (at, p) = guard.as_ref()?;
    if at.elapsed() < PROFILE_TTL {
        Some(p.clone())
    } else {
        None
    }
}

fn store_profile(p: CachedProfile) {
    *PROFILE_CACHE.lock() = Some((Instant::now(), p));
}

/// Test/diagnostic helper: forget the cached profile.
pub fn clear_profile_cache() {
    *PROFILE_CACHE.lock() = None;
}

// ---------- provider ----------

pub struct ClaudeProvider {
    ctx: ProviderCtx,
}

impl ClaudeProvider {
    pub fn new(ctx: ProviderCtx) -> Self {
        Self { ctx }
    }

    async fn fetch_profile(&self, http: &reqwest::Client, token: &str) -> Option<CachedProfile> {
        if let Some(p) = cached_profile() {
            return Some(p);
        }
        let resp = http
            .get(PROFILE_URL)
            .bearer_auth(token)
            .header("anthropic-beta", OAUTH_BETA)
            .header("User-Agent", self.ctx.user_agent.clone())
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            log::debug!("claude profile returned {}", resp.status());
            return None;
        }
        let profile: ProfileResponse = resp.json().await.ok()?;
        let account = profile.account.as_ref().map(|a| AccountInfo {
            email: a.email.clone(),
            name: a.display_name.clone().or_else(|| a.full_name.clone()),
        });
        let p = CachedProfile {
            account,
            rate_limit_tier: profile.organization.and_then(|o| o.rate_limit_tier),
        };
        store_profile(p.clone());
        Some(p)
    }
}

#[async_trait]
impl Provider for ClaudeProvider {
    fn id(&self) -> &'static str {
        CLAUDE_ID
    }

    fn display_name(&self) -> &'static str {
        DISPLAY_NAME
    }

    fn info(&self) -> ProviderInfo {
        let creds = load_credentials();
        let now = chrono::Utc::now().timestamp_millis();
        ProviderInfo {
            id: CLAUDE_ID.into(),
            display_name: DISPLAY_NAME.into(),
            logged_in: creds.as_ref().map(|c| !c.is_expired(now)).unwrap_or(false),
            credential_path: credentials_path().map(|p| p.display().to_string()),
            log_path: log_root().map(|p| p.display().to_string()),
            plan_label: creds.as_ref().and_then(|c| {
                plan_label(c.subscription_type.as_deref(), c.rate_limit_tier.as_deref())
            }),
        }
    }

    async fn fetch(&self, http: &reqwest::Client) -> ProviderQuota {
        let Some(creds) = load_credentials() else {
            let mut q = empty_quota(CLAUDE_ID, DISPLAY_NAME, ProviderStatus::NotLoggedIn);
            q.error = Some("Not logged in — run `claude` to sign in".into());
            return q;
        };
        let now_ms = chrono::Utc::now().timestamp_millis();
        if creds.is_expired(now_ms) {
            log::info!("claude: access token expired");
            return degraded(
                &self.ctx,
                CLAUDE_ID,
                DISPLAY_NAME,
                ProviderStatus::TokenExpired,
                EXPIRED_MESSAGE,
            );
        }

        let resp = http
            .get(USAGE_URL)
            .bearer_auth(&creds.access_token)
            .header("anthropic-beta", OAUTH_BETA)
            .header("User-Agent", self.ctx.user_agent.clone())
            .header("Accept", "application/json")
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                log::warn!("claude usage request failed: {e}");
                return degraded(
                    &self.ctx,
                    CLAUDE_ID,
                    DISPLAY_NAME,
                    ProviderStatus::Error,
                    format!("Could not reach api.anthropic.com: {e}"),
                );
            }
        };

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return degraded(
                &self.ctx,
                CLAUDE_ID,
                DISPLAY_NAME,
                ProviderStatus::TokenExpired,
                EXPIRED_MESSAGE,
            );
        }
        if !status.is_success() {
            return degraded(
                &self.ctx,
                CLAUDE_ID,
                DISPLAY_NAME,
                ProviderStatus::Error,
                format!("Anthropic usage API returned HTTP {}", status.as_u16()),
            );
        }

        let usage: UsageResponse = match resp.json().await {
            Ok(u) => u,
            Err(e) => {
                return degraded(
                    &self.ctx,
                    CLAUDE_ID,
                    DISPLAY_NAME,
                    ProviderStatus::Error,
                    format!("Unexpected usage response: {e}"),
                );
            }
        };

        let profile = self.fetch_profile(http, &creds.access_token).await;
        let tier = profile
            .as_ref()
            .and_then(|p| p.rate_limit_tier.clone())
            .or_else(|| creds.rate_limit_tier.clone());

        let mut quota = empty_quota(CLAUDE_ID, DISPLAY_NAME, ProviderStatus::Ok);
        quota.windows = map_windows(&usage);
        quota.plan = creds.subscription_type.clone();
        quota.plan_label = plan_label(creds.subscription_type.as_deref(), tier.as_deref());
        quota.account = profile.and_then(|p| p.account);
        quota.credits = credits_from(&usage.extra_usage);
        quota.source = DataSource::Api;
        quota.fetched_at = now_rfc3339();
        write_cache(&self.ctx, &quota);
        quota
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
      "five_hour": {"utilization": 53.0, "resets_at": "2026-09-15T06:20:00.640814+00:00"},
      "seven_day": {"utilization": 37.0, "resets_at": "2026-09-18T20:00:00.640835+00:00"},
      "seven_day_opus": null,
      "seven_day_sonnet": null,
      "nimbus_quill": {"utilization": 0.0, "resets_at": null},
      "extra_usage": {"is_enabled": false, "monthly_limit": null},
      "limits": [
        {"kind":"session","group":"session","percent":53,"severity":"normal",
         "resets_at":"2026-09-15T06:20:00.640814+00:00","scope":null,"is_active":true},
        {"kind":"weekly_all","group":"weekly","percent":37,
         "resets_at":"2026-09-18T20:00:00.640835+00:00","scope":null},
        {"kind":"weekly_scoped","group":"weekly","percent":33,
         "resets_at":"2026-09-18T20:00:00.641025+00:00",
         "scope":{"model":{"id":null,"display_name":"Fable"}}},
        {"kind":"brand_new_kind","percent":5,"resets_at":null,"scope":null}
      ],
      "seven_day_breakdown": {"rows": []}
    }"#;

    #[test]
    fn maps_the_limits_array() {
        let usage: UsageResponse = serde_json::from_str(FIXTURE).unwrap();
        let w = map_windows(&usage);
        assert_eq!(w.len(), 3, "unknown kinds are skipped");

        assert_eq!(w[0].kind, WindowKind::FiveHour);
        assert_eq!(w[0].label, "5-hour");
        assert_eq!(w[0].used_percent, 53.0);
        assert_eq!(w[0].resets_at.as_deref(), Some("2026-09-15T06:20:00Z"));
        assert!(w[0].is_primary, "the 5-hour window is primary");
        assert!(w[0].scope.is_none());

        assert_eq!(w[1].kind, WindowKind::SevenDay);
        assert_eq!(w[1].label, "Weekly");
        assert_eq!(w[1].used_percent, 37.0);
        assert!(!w[1].is_primary);

        assert_eq!(w[2].kind, WindowKind::SevenDay);
        assert_eq!(w[2].scope.as_deref(), Some("Fable"));
        assert_eq!(w[2].label, "Weekly · Fable");
        assert!(!w[2].is_primary, "scoped windows are never primary");
    }

    #[test]
    fn falls_back_to_legacy_keys() {
        let mut usage: UsageResponse = serde_json::from_str(FIXTURE).unwrap();
        usage.limits.clear();
        usage.seven_day_opus = Some(LegacyWindow {
            utilization: Some(12.0),
            resets_at: Some("2026-09-18T20:00:00Z".into()),
        });
        let w = map_windows(&usage);
        assert_eq!(w.len(), 3);
        assert_eq!(w[0].kind, WindowKind::FiveHour);
        assert!(w[0].is_primary);
        assert_eq!(w[2].scope.as_deref(), Some("Opus"));
        assert_eq!(w[2].used_percent, 12.0);
    }

    #[test]
    fn tolerates_garbage_and_unknown_fields() {
        let usage: UsageResponse =
            serde_json::from_str(r#"{"totally":"unexpected","limits":[]}"#).unwrap();
        assert!(map_windows(&usage).is_empty());
    }

    #[test]
    fn plan_labels() {
        assert_eq!(
            plan_label(Some("max"), Some("default_claude_max_5x")).as_deref(),
            Some("Claude Max 5x")
        );
        assert_eq!(
            plan_label(Some("max"), Some("default_claude_max_20x")).as_deref(),
            Some("Claude Max 20x")
        );
        assert_eq!(plan_label(Some("pro"), None).as_deref(), Some("Claude Pro"));
        assert_eq!(
            plan_label(Some("team"), None).as_deref(),
            Some("Claude Team")
        );
        assert_eq!(
            plan_label(Some("enterprise"), None).as_deref(),
            Some("Claude Enterprise")
        );
        assert_eq!(plan_label(Some("max"), None).as_deref(), Some("Claude Max"));
        assert_eq!(plan_label(None, None), None);
    }

    #[test]
    fn credentials_parse_and_expiry() {
        let c = parse_credentials(
            r#"{"claudeAiOauth":{"accessToken":"sk-ant-x","expiresAt":1000,
                "subscriptionType":"max","rateLimitTier":"default_claude_max_5x",
                "unknown":"ignored"}}"#,
        )
        .unwrap();
        assert_eq!(c.subscription_type.as_deref(), Some("max"));
        assert!(c.is_expired(2000));
        assert!(!c.is_expired(500));
        assert!(parse_credentials("{}").is_none());
        assert!(parse_credentials("not json").is_none());
        assert!(parse_credentials(r#"{"claudeAiOauth":{"accessToken":""}}"#).is_none());
    }

    #[test]
    fn extra_usage_credits() {
        assert!(credits_from(&Some(ExtraUsage::default())).is_none());
        let c = credits_from(&Some(ExtraUsage {
            is_enabled: true,
            monthly_limit: Some(50.0),
            used_credits: Some(12.5),
            utilization: Some(25.0),
        }))
        .unwrap();
        assert!(c.has_credits);
        assert!(!c.unlimited);
        assert_eq!(c.balance.as_deref(), Some("12.50/50.00"));
    }
}
