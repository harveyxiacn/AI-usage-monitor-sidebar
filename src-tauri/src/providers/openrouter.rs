//! OpenRouter credit provider. [BACKEND]
//!
//! **Experimental — not verified against a live account.** The response shapes
//! below come from the public API reference (openrouter.ai/docs, fetched
//! 2026-10-02; see docs/PROVIDERS.md §4), the fixtures in the tests are written
//! from those pages, not captured from a real key.
//!
//! There is no CLI login to borrow, so the credential is an API key the user
//! already exports in the environment. The *name* of that variable is a
//! setting (`openrouterKeyEnv`, default `OPENROUTER_API_KEY`); the key itself is
//! never stored, logged or shown, and is only ever sent to openrouter.ai.
//!
//! Two calls, both `Authorization: Bearer <key>`:
//!
//! * `GET /api/v1/key` — the key's own limit / usage (any key may call it).
//! * `GET /api/v1/credits` — account-wide purchased credits and usage. The
//!   docs say only *management* keys may call it, so a 403 here is expected for
//!   an ordinary key and is not an error.

use super::{
    clamp_percent, degraded, empty_quota, mark_primary, now_rfc3339, null_default, rate_limited,
    retry_after_of, write_cache, Provider, ProviderCtx, OPENROUTER_ID,
};
use crate::model::{
    CreditsInfo, ExtraSeverity, ProviderInfo, ProviderQuota, ProviderStatus, QuotaExtra,
    QuotaWindow, WindowKind,
};
use async_trait::async_trait;
use chrono::{Datelike, TimeZone, Utc};
use serde::Deserialize;

pub const DISPLAY_NAME: &str = "OpenRouter";
pub const KEY_URL: &str = "https://openrouter.ai/api/v1/key";
pub const CREDITS_URL: &str = "https://openrouter.ai/api/v1/credits";
/// Environment variable read when the setting does not name another one.
pub const DEFAULT_KEY_ENV: &str = "OPENROUTER_API_KEY";

pub const EXPIRED_MESSAGE: &str =
    "OpenRouter rejected the API key — check the key in your environment";

/// `true` for a plausible environment variable *name* (not a key): upper-case
/// letters, digits and `_`, not starting with a digit. Same rule as the
/// assessment feature's `api_key_env`, so pasting a key by mistake is refused.
pub fn is_valid_key_env(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
}

pub fn not_logged_in_message(env_name: &str) -> String {
    format!("No API key — set the {env_name} environment variable and restart the app")
}

/// Value of the configured variable, trimmed; empty counts as unset.
fn key_from(lookup: fn(&str) -> Option<String>, env_name: &str) -> Option<String> {
    lookup(env_name)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_lookup(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

// ---------- API response shapes ----------

#[derive(Deserialize, Default)]
struct KeyEnvelope {
    #[serde(default, deserialize_with = "null_default")]
    data: KeyData,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct KeyData {
    pub label: Option<String>,
    /// Credit limit of the key in USD; `null` = no limit.
    pub limit: Option<f64>,
    /// `"daily"`, `"weekly"`, `"monthly"` or `null`.
    pub limit_reset: Option<String>,
    pub limit_remaining: Option<f64>,
    #[serde(deserialize_with = "null_default")]
    pub usage: f64,
    #[serde(deserialize_with = "null_default")]
    pub is_free_tier: bool,
}

#[derive(Deserialize, Default)]
struct CreditsEnvelope {
    #[serde(default, deserialize_with = "null_default")]
    data: CreditsData,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct CreditsData {
    #[serde(deserialize_with = "null_default")]
    pub total_credits: f64,
    #[serde(deserialize_with = "null_default")]
    pub total_usage: f64,
}

// ---------- mapping ----------

fn usd(v: f64) -> String {
    format!("${:.2}", v.max(0.0))
}

/// Next reset of a `limit_reset` period. OpenRouter documents resets at
/// midnight UTC (weekly: Monday); not verified against a live key.
fn next_reset(period: &str, now: chrono::DateTime<Utc>) -> Option<(String, u64)> {
    let today = now.date_naive();
    let (date, secs) = match period {
        "daily" => (today.succ_opt()?, 86_400),
        "weekly" => {
            let ahead = 7 - today.weekday().num_days_from_monday() as i64;
            (today + chrono::Duration::days(ahead), 604_800)
        }
        "monthly" => {
            let (y, m) = if today.month() == 12 {
                (today.year() + 1, 1)
            } else {
                (today.year(), today.month() + 1)
            };
            (chrono::NaiveDate::from_ymd_opt(y, m, 1)?, 30 * 86_400)
        }
        _ => return None,
    };
    let at = Utc
        .from_utc_datetime(&date.and_hms_opt(0, 0, 0)?)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    Some((at, secs))
}

fn credit_window(
    label: &str,
    used_percent: f64,
    resets_at: Option<String>,
    window_seconds: Option<u64>,
) -> QuotaWindow {
    QuotaWindow {
        kind: WindowKind::Other,
        label: label.to_string(),
        window_seconds,
        used_percent: clamp_percent(used_percent),
        resets_at,
        scope: None,
        is_primary: false,
        forecast: None,
    }
}

/// Windows and extras for a key. `credits` is only consulted for a key with no
/// limit of its own.
pub fn map_quota(
    key: &KeyData,
    credits: Option<&CreditsData>,
    now: chrono::DateTime<Utc>,
) -> (Vec<QuotaWindow>, Vec<QuotaExtra>, Option<CreditsInfo>) {
    let mut windows = Vec::new();
    let mut extras = Vec::new();
    let mut info = None;

    match key.limit.filter(|l| *l > 0.0) {
        Some(limit) => {
            // `limit_remaining` already accounts for a periodic reset, whereas
            // `usage` is lifetime: prefer it, fall back to usage / limit.
            let remaining = key.limit_remaining.unwrap_or(limit - key.usage);
            let used = (limit - remaining) / limit * 100.0;
            let reset = key.limit_reset.as_deref().and_then(|p| next_reset(p, now));
            windows.push(credit_window(
                "Credits",
                used,
                reset.as_ref().map(|r| r.0.clone()),
                reset.map(|r| r.1),
            ));
            extras.push(QuotaExtra::value(
                "credit_balance",
                usd(remaining),
                ExtraSeverity::Info,
            ));
            info = Some(CreditsInfo {
                has_credits: remaining > 0.0,
                unlimited: false,
                balance: Some(usd(remaining)),
            });
        }
        None => match credits.filter(|c| c.total_credits > 0.0) {
            Some(c) => {
                let remaining = c.total_credits - c.total_usage;
                windows.push(credit_window(
                    "Credits",
                    c.total_usage / c.total_credits * 100.0,
                    None,
                    None,
                ));
                extras.push(QuotaExtra::value(
                    "credit_balance",
                    usd(remaining),
                    ExtraSeverity::Info,
                ));
                info = Some(CreditsInfo {
                    has_credits: remaining > 0.0,
                    unlimited: false,
                    balance: Some(usd(remaining)),
                });
            }
            // No limit and no readable balance (an ordinary key cannot call
            // /credits): all that is known is how much this key has spent.
            None => extras.push(QuotaExtra::value(
                "credit_spent",
                usd(key.usage),
                ExtraSeverity::Info,
            )),
        },
    }
    mark_primary(&mut windows);
    (windows, extras, info)
}

// ---------- provider ----------

pub struct OpenRouterProvider {
    ctx: ProviderCtx,
    key_env: String,
    lookup: fn(&str) -> Option<String>,
}

impl OpenRouterProvider {
    pub fn new(ctx: ProviderCtx) -> Self {
        let key_env = if is_valid_key_env(&ctx.openrouter_key_env) {
            ctx.openrouter_key_env.clone()
        } else {
            DEFAULT_KEY_ENV.to_string()
        };
        Self {
            ctx,
            key_env,
            lookup: env_lookup,
        }
    }

    #[cfg(test)]
    fn with_lookup(ctx: ProviderCtx, lookup: fn(&str) -> Option<String>) -> Self {
        Self {
            lookup,
            ..Self::new(ctx)
        }
    }

    fn key(&self) -> Option<String> {
        key_from(self.lookup, &self.key_env)
    }

    fn fail(&self, status: ProviderStatus, message: String) -> ProviderQuota {
        degraded(&self.ctx, OPENROUTER_ID, DISPLAY_NAME, status, message)
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    fn id(&self) -> &'static str {
        OPENROUTER_ID
    }

    fn display_name(&self) -> &'static str {
        DISPLAY_NAME
    }

    fn experimental(&self) -> bool {
        true
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: OPENROUTER_ID.into(),
            display_name: DISPLAY_NAME.into(),
            logged_in: self.key().is_some(),
            // the variable's name, never its value
            credential_path: Some(format!("${}", self.key_env)),
            // OpenRouter is an API gateway: there are no local session logs
            log_path: None,
            plan_label: None,
            experimental: true,
        }
    }

    async fn fetch(&self, http: &reqwest::Client) -> ProviderQuota {
        let Some(key) = self.key() else {
            let mut q = empty_quota(OPENROUTER_ID, DISPLAY_NAME, ProviderStatus::NotLoggedIn);
            q.error = Some(not_logged_in_message(&self.key_env));
            return q;
        };

        let resp = match super::send_with_retry(&self.ctx, || {
            http.get(self.ctx.endpoint(KEY_URL))
                .bearer_auth(&key)
                .header("Accept", "application/json")
        })
        .await
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!("openrouter key request failed: {e}");
                return self.fail(
                    ProviderStatus::Error,
                    format!("Could not reach openrouter.ai: {e}"),
                );
            }
        };
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return self.fail(ProviderStatus::TokenExpired, EXPIRED_MESSAGE.into());
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = retry_after_of(resp.headers());
            log::warn!("openrouter answered HTTP 429");
            return rate_limited(
                &self.ctx,
                OPENROUTER_ID,
                DISPLAY_NAME,
                retry_after,
                "OpenRouter is rate-limiting the key endpoint (HTTP 429); showing the last known values until the next attempt",
            );
        }
        if !status.is_success() {
            return self.fail(
                ProviderStatus::Error,
                format!("OpenRouter key API returned HTTP {}", status.as_u16()),
            );
        }
        let body = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return self.fail(
                    ProviderStatus::Error,
                    format!("Could not read the OpenRouter response: {e}"),
                )
            }
        };
        let key_data = match serde_json::from_slice::<KeyEnvelope>(&body) {
            Ok(env) => env.data,
            Err(e) => {
                log::warn!("openrouter key response has an unexpected shape: {e}");
                return self.fail(
                    ProviderStatus::Error,
                    format!("Unexpected OpenRouter response: {e}"),
                );
            }
        };

        // Account-wide balance, only worth a second call for an unlimited key.
        let credits = if key_data.limit.filter(|l| *l > 0.0).is_none() {
            self.fetch_credits(http, &key).await
        } else {
            None
        };

        let (windows, extras, info) = map_quota(&key_data, credits.as_ref(), Utc::now());
        let mut quota = empty_quota(OPENROUTER_ID, DISPLAY_NAME, ProviderStatus::Ok);
        quota.windows = windows;
        quota.extras = extras;
        quota.credits = info;
        quota.plan_label = Some(
            if key_data.is_free_tier {
                "Free tier"
            } else {
                "Pay as you go"
            }
            .to_string(),
        );
        quota.fetched_at = now_rfc3339();
        write_cache(&self.ctx, &quota);
        quota
    }
}

impl OpenRouterProvider {
    /// `None` for every failure: an ordinary key gets 403 by design, and a
    /// missing balance must not turn a good `/key` answer into an error.
    async fn fetch_credits(&self, http: &reqwest::Client, key: &str) -> Option<CreditsData> {
        let resp = super::send_with_retry(&self.ctx, || {
            http.get(self.ctx.endpoint(CREDITS_URL))
                .bearer_auth(key)
                .header("Accept", "application/json")
        })
        .await
        .ok()?;
        if !resp.status().is_success() {
            log::debug!("openrouter credits: HTTP {}", resp.status().as_u16());
            return None;
        }
        let body = resp.bytes().await.ok()?;
        serde_json::from_slice::<CreditsEnvelope>(&body)
            .ok()
            .map(|e| e.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::StubServer;
    use std::time::Duration;

    // ---- Fixtures are written from the API reference, NOT captured live. ----

    const LIMITED: &str = r#"{"data":{"label":"sk-or-v1-abc...xyz","limit":10.0,"limit_reset":"monthly",
        "limit_remaining":7.5,"include_byok_in_limit":false,"usage":2.5,"usage_daily":0.1,
        "usage_weekly":0.5,"usage_monthly":2.5,"byok_usage":0,"is_free_tier":false}}"#;
    const UNLIMITED: &str = r#"{"data":{"label":"k","limit":null,"limit_reset":null,
        "limit_remaining":null,"usage":12.25,"is_free_tier":false}}"#;
    const CREDITS: &str = r#"{"data":{"total_credits":100.0,"total_usage":25.0}}"#;

    fn stub(server: &StubServer) -> OpenRouterProvider {
        OpenRouterProvider::with_lookup(
            ProviderCtx {
                api_base: Some(server.base.clone()),
                retry_delay: Duration::from_millis(5),
                ..ProviderCtx::default()
            },
            |_| Some("sk-or-test-not-a-real-key".into()),
        )
    }

    fn ok(body: &str) -> (&'static str, Vec<(&'static str, &'static str)>, String) {
        ("200 OK", vec![], body.to_string())
    }

    #[tokio::test]
    async fn a_limited_key_becomes_a_credits_window_with_its_reset() {
        let server = StubServer::start(vec![ok(LIMITED)]);
        let q = stub(&server).fetch(&reqwest::Client::new()).await;
        assert_eq!(q.status, ProviderStatus::Ok);
        assert_eq!(q.windows.len(), 1);
        let w = &q.windows[0];
        assert_eq!(w.label, "Credits");
        assert!((w.used_percent - 25.0).abs() < 1e-9);
        assert!(w.resets_at.is_some(), "monthly limit has a reset");
        assert!(w.is_primary);
        assert_eq!(server.hits(), 1, "a limited key needs no /credits call");
        assert_eq!(q.extras[0].value.as_deref(), Some("$7.50"));
        assert_eq!(q.plan_label.as_deref(), Some("Pay as you go"));
    }

    #[tokio::test]
    async fn an_unlimited_key_falls_back_to_the_account_balance() {
        let server = StubServer::start(vec![ok(UNLIMITED), ok(CREDITS)]);
        let q = stub(&server).fetch(&reqwest::Client::new()).await;
        assert_eq!(q.status, ProviderStatus::Ok);
        assert_eq!(q.windows.len(), 1);
        assert!((q.windows[0].used_percent - 25.0).abs() < 1e-9);
        assert_eq!(q.windows[0].resets_at, None);
        assert_eq!(q.credits.unwrap().balance.as_deref(), Some("$75.00"));
    }

    #[tokio::test]
    async fn an_ordinary_key_without_credits_access_only_reports_spend() {
        let server = StubServer::start(vec![ok(UNLIMITED), ("403 Forbidden", vec![], "{}".into())]);
        let q = stub(&server).fetch(&reqwest::Client::new()).await;
        assert_eq!(q.status, ProviderStatus::Ok);
        assert!(q.windows.is_empty());
        assert_eq!(q.extras[0].kind, "credit_spent");
        assert_eq!(q.extras[0].value.as_deref(), Some("$12.25"));
    }

    #[tokio::test]
    async fn a_401_means_the_key_was_rejected() {
        let server = StubServer::start(vec![("401 Unauthorized", vec![], "{}".into())]);
        let q = stub(&server).fetch(&reqwest::Client::new()).await;
        assert_eq!(q.status, ProviderStatus::TokenExpired);
        assert_eq!(q.error.as_deref(), Some(EXPIRED_MESSAGE));
    }

    #[tokio::test]
    async fn a_429_is_rate_limited_not_an_error() {
        let server = StubServer::start(vec![(
            "429 Too Many Requests",
            vec![("Retry-After", "120")],
            "{}".into(),
        )]);
        let q = stub(&server).fetch(&reqwest::Client::new()).await;
        assert_eq!(q.status, ProviderStatus::RateLimited);
        assert!(q.next_attempt_at.is_some());
        assert_eq!(server.hits(), 1, "429 is final, not retried");
    }

    #[tokio::test]
    async fn a_missing_variable_is_not_logged_in_and_makes_no_request() {
        let server = StubServer::start(vec![ok(LIMITED)]);
        let p = OpenRouterProvider::with_lookup(
            ProviderCtx {
                api_base: Some(server.base.clone()),
                ..ProviderCtx::default()
            },
            |_| None,
        );
        let q = p.fetch(&reqwest::Client::new()).await;
        assert_eq!(q.status, ProviderStatus::NotLoggedIn);
        assert!(q.error.unwrap().contains(DEFAULT_KEY_ENV));
        assert_eq!(server.hits(), 0);
        assert!(!p.info().logged_in);
        // an empty value counts as unset
        assert_eq!(key_from(|_| Some("  ".into()), "X"), None);
    }

    #[test]
    fn a_pasted_key_is_not_a_valid_variable_name() {
        assert!(is_valid_key_env("OPENROUTER_API_KEY"));
        assert!(!is_valid_key_env("sk-or-v1-abc"));
        assert!(!is_valid_key_env("1KEY"));
        assert!(!is_valid_key_env(""));
        let p = OpenRouterProvider::new(ProviderCtx {
            openrouter_key_env: "sk-or-v1-abc".into(),
            ..ProviderCtx::default()
        });
        assert_eq!(p.key_env, DEFAULT_KEY_ENV);
    }

    #[test]
    fn reset_times_follow_utc_midnights() {
        let now = Utc.with_ymd_and_hms(2026, 10, 2, 15, 0, 0).unwrap(); // a Friday
        assert_eq!(next_reset("daily", now).unwrap().0, "2026-10-03T00:00:00Z");
        assert_eq!(next_reset("weekly", now).unwrap().0, "2026-10-05T00:00:00Z");
        assert_eq!(
            next_reset("monthly", now).unwrap().0,
            "2026-11-01T00:00:00Z"
        );
        let dec = Utc.with_ymd_and_hms(2026, 12, 31, 1, 0, 0).unwrap();
        assert_eq!(
            next_reset("monthly", dec).unwrap().0,
            "2027-01-01T00:00:00Z"
        );
        assert!(next_reset("hourly", now).is_none());
    }

    #[test]
    fn limit_remaining_wins_over_lifetime_usage() {
        // lifetime usage 50 but this period's remaining says 90 % left
        let key = KeyData {
            limit: Some(10.0),
            limit_remaining: Some(9.0),
            usage: 50.0,
            limit_reset: Some("daily".into()),
            ..KeyData::default()
        };
        let (w, _, _) = map_quota(&key, None, Utc::now());
        assert!((w[0].used_percent - 10.0).abs() < 1e-9);
        // no limit_remaining: usage / limit, clamped
        let key = KeyData {
            limit: Some(10.0),
            usage: 25.0,
            ..KeyData::default()
        };
        let (w, _, _) = map_quota(&key, None, Utc::now());
        assert_eq!(w[0].used_percent, 100.0);
    }
}
