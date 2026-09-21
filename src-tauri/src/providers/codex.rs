//! OpenAI Codex CLI (ChatGPT login) quota provider. [BACKEND]
//!
//! Credentials come from `$CODEX_HOME/auth.json` (default `~/.codex/auth.json`).
//! The plan and the expiry are read straight out of the access token's JWT
//! claims — the signature is irrelevant to us, we never mint tokens.

use super::{
    clamp_percent, degraded, empty_quota, mark_primary, now_rfc3339, rfc3339_from_unix_secs,
    window_label, write_cache, Provider, ProviderCtx, CODEX_ID,
};
use crate::model::{
    CreditsInfo, DataSource, ProviderInfo, ProviderQuota, ProviderStatus, QuotaWindow, WindowKind,
};
use async_trait::async_trait;
use base64::Engine;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const DISPLAY_NAME: &str = "Codex";
pub const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
pub const AUTH_CLAIM: &str = "https://api.openai.com/auth";

pub const API_KEY_HINT: &str =
    "Codex is using an API key; log in with `codex login` to see plan quotas";
pub const EXPIRED_MESSAGE: &str = "Codex login expired — run `codex login` to refresh";

/// How many session files the local-log fallback looks at (newest first).
const FALLBACK_FILES: usize = 20;

// ---------- credentials ----------

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct AuthFile {
    pub auth_mode: Option<String>,
    #[serde(rename = "OPENAI_API_KEY")]
    pub openai_api_key: Option<String>,
    pub tokens: Option<AuthTokens>,
    pub last_refresh: Option<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct AuthTokens {
    pub access_token: Option<String>,
    pub account_id: Option<String>,
}

/// `$CODEX_HOME` or `~/.codex`.
pub fn codex_home() -> Option<PathBuf> {
    match std::env::var("CODEX_HOME") {
        Ok(v) if !v.trim().is_empty() => Some(PathBuf::from(v)),
        _ => super::home_dir().map(|h| h.join(".codex")),
    }
}

pub fn credentials_path() -> Option<PathBuf> {
    codex_home().map(|d| d.join("auth.json"))
}

/// `~/.codex/sessions` — the session-log root.
pub fn log_root() -> Option<PathBuf> {
    codex_home().map(|d| d.join("sessions"))
}

/// `~/.codex/archived_sessions`, also scanned during ingestion when present.
pub fn archived_log_root() -> Option<PathBuf> {
    codex_home().map(|d| d.join("archived_sessions"))
}

pub fn load_auth() -> Option<AuthFile> {
    let path = credentials_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

// ---------- JWT claims (no signature verification) ----------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct JwtClaims {
    /// unix seconds
    pub exp: Option<i64>,
    pub plan_type: Option<String>,
    pub account_id: Option<String>,
}

impl JwtClaims {
    pub fn is_expired(&self, now_secs: i64) -> bool {
        self.exp.map(|e| e <= now_secs).unwrap_or(false)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawClaims {
    exp: Option<i64>,
    #[serde(rename = "https://api.openai.com/auth")]
    openai_auth: Option<OpenAiAuthClaim>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct OpenAiAuthClaim {
    chatgpt_plan_type: Option<String>,
    chatgpt_account_id: Option<String>,
}

/// Decode the *payload* of a JWT. Never validates the signature — the token is
/// already trusted (it came from the user's own disk), we just want the claims.
pub fn parse_jwt_claims(token: &str) -> Option<JwtClaims> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let raw: RawClaims = serde_json::from_slice(&bytes).ok()?;
    Some(JwtClaims {
        exp: raw.exp,
        plan_type: raw
            .openai_auth
            .as_ref()
            .and_then(|a| a.chatgpt_plan_type.clone()),
        account_id: raw.openai_auth.and_then(|a| a.chatgpt_account_id),
    })
}

// ---------- API response shapes ----------
//
// The endpoint is undocumented and drifts: fields come and go, and an empty
// list or flag is as often an explicit `null` as it is absent. `serde(default)`
// only covers *absent*, so everything that is not an `Option` goes through
// `null_default`, and one odd field can never discard the whole response.

use super::null_default;

/// Integers occasionally arrive as floats (`604800.0`); a value that is not a
/// number at all reads as missing instead of failing the response.
fn lenient_int<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: TryFrom<i64>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64)))
        .and_then(|n| T::try_from(n).ok()))
}

/// `balance` has been seen as a string; tolerate a bare number as well.
fn string_or_number<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(
        match Option::<serde_json::Value>::deserialize(deserializer)? {
            Some(serde_json::Value::String(s)) => Some(s),
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            _ => None,
        },
    )
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct UsageResponse {
    pub plan_type: Option<String>,
    pub email: Option<String>,
    pub rate_limit: Option<RateLimit>,
    #[serde(deserialize_with = "null_default")]
    pub additional_rate_limits: Vec<AdditionalRateLimit>,
    pub credits: Option<Credits>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct RateLimit {
    pub primary_window: Option<Window>,
    pub secondary_window: Option<Window>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Window {
    pub used_percent: Option<f64>,
    #[serde(deserialize_with = "lenient_int")]
    pub limit_window_seconds: Option<u64>,
    /// unix seconds
    #[serde(deserialize_with = "lenient_int")]
    pub reset_at: Option<i64>,
    #[serde(deserialize_with = "lenient_int")]
    pub reset_after_seconds: Option<i64>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct AdditionalRateLimit {
    pub limit_name: Option<String>,
    pub metered_feature: Option<String>,
    pub rate_limit: Option<RateLimit>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Credits {
    #[serde(deserialize_with = "null_default")]
    pub has_credits: bool,
    #[serde(deserialize_with = "null_default")]
    pub unlimited: bool,
    #[serde(deserialize_with = "string_or_number")]
    pub balance: Option<String>,
}

// ---------- mapping ----------

fn map_window(w: &Window, scope: Option<&str>) -> QuotaWindow {
    let secs = w.limit_window_seconds;
    let base = window_label(secs);
    QuotaWindow {
        kind: secs
            .map(WindowKind::from_seconds)
            .unwrap_or(WindowKind::Other),
        label: match scope {
            Some(s) => format!("{s} · {base}"),
            None => base,
        },
        window_seconds: secs,
        used_percent: clamp_percent(w.used_percent.unwrap_or(0.0)),
        resets_at: w.reset_at.and_then(rfc3339_from_unix_secs),
        scope: scope.map(|s| s.to_string()),
        is_primary: false,
    }
}

/// Windows for a `rate_limit` block. Classification is purely by
/// `limit_window_seconds`: primary is *not* necessarily the 5-hour window.
pub fn map_rate_limit(rl: &RateLimit, scope: Option<&str>) -> Vec<QuotaWindow> {
    let mut out = Vec::new();
    if let Some(w) = &rl.primary_window {
        out.push(map_window(w, scope));
    }
    if let Some(w) = &rl.secondary_window {
        out.push(map_window(w, scope));
    }
    out
}

pub fn map_windows(usage: &UsageResponse) -> Vec<QuotaWindow> {
    let mut windows = usage
        .rate_limit
        .as_ref()
        .map(|rl| map_rate_limit(rl, None))
        .unwrap_or_default();
    for extra in &usage.additional_rate_limits {
        let Some(rl) = &extra.rate_limit else {
            continue;
        };
        let name = extra
            .limit_name
            .clone()
            .or_else(|| extra.metered_feature.clone())
            .unwrap_or_else(|| "Other".to_string());
        windows.extend(map_rate_limit(rl, Some(&name)));
    }
    mark_primary(&mut windows);
    windows
}

pub fn plan_label(plan_type: Option<&str>) -> Option<String> {
    let raw = plan_type?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(match raw {
        "plus" => "ChatGPT Plus".to_string(),
        "pro" => "ChatGPT Pro".to_string(),
        "prolite" => "ChatGPT Pro Lite".to_string(),
        "free" => "ChatGPT Free".to_string(),
        "team" => "ChatGPT Team".to_string(),
        "business" => "ChatGPT Business".to_string(),
        "enterprise" => "ChatGPT Enterprise".to_string(),
        "edu" => "ChatGPT Edu".to_string(),
        other => format!("ChatGPT {other}"),
    })
}

fn credits_from(c: &Option<Credits>) -> Option<CreditsInfo> {
    let c = c.as_ref()?;
    Some(CreditsInfo {
        has_credits: c.has_credits,
        unlimited: c.unlimited,
        balance: c.balance.clone(),
    })
}

// ---------- local-log fallback ----------

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
struct LogLine {
    #[serde(rename = "type")]
    kind: String,
    timestamp: Option<String>,
    payload: Option<LogPayload>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
struct LogPayload {
    #[serde(rename = "type")]
    kind: String,
    rate_limits: Option<LogRateLimits>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct LogRateLimits {
    pub primary: Option<LogWindow>,
    pub secondary: Option<LogWindow>,
    pub plan_type: Option<String>,
    pub credits: Option<Credits>,
    #[serde(skip)]
    pub observed_at: Option<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct LogWindow {
    pub used_percent: Option<f64>,
    pub window_minutes: Option<u64>,
    /// unix seconds
    pub resets_at: Option<i64>,
}

impl LogWindow {
    fn to_quota_window(&self) -> QuotaWindow {
        let secs = self.window_minutes.map(|m| m.saturating_mul(60));
        QuotaWindow {
            kind: secs
                .map(WindowKind::from_seconds)
                .unwrap_or(WindowKind::Other),
            label: window_label(secs),
            window_seconds: secs,
            used_percent: clamp_percent(self.used_percent.unwrap_or(0.0)),
            resets_at: self.resets_at.and_then(rfc3339_from_unix_secs),
            scope: None,
            is_primary: false,
        }
    }
}

pub fn map_log_rate_limits(rl: &LogRateLimits) -> Vec<QuotaWindow> {
    let mut out = Vec::new();
    if let Some(p) = &rl.primary {
        out.push(p.to_quota_window());
    }
    if let Some(s) = &rl.secondary {
        out.push(s.to_quota_window());
    }
    mark_primary(&mut out);
    out
}

/// The newest `.jsonl` files under `root`, most recently modified first.
pub fn newest_session_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .filter_map(|e| {
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((mtime, e.into_path()))
        })
        .collect();
    files.sort_by_key(|(mtime, _)| std::cmp::Reverse(*mtime));
    files.into_iter().take(limit).map(|(_, p)| p).collect()
}

/// Last `event_msg`/`token_count` line carrying `payload.rate_limits` in the
/// newest session files. Used when the token is expired or the API is down.
pub fn rate_limits_from_logs(root: &Path) -> Option<LogRateLimits> {
    newest_session_files(root, FALLBACK_FILES)
        .iter()
        .filter_map(|path| rate_limits_in_file(path))
        .max_by_key(|limits| observed_ms(limits.observed_at.as_deref()))
}

fn observed_ms(timestamp: Option<&str>) -> i64 {
    timestamp
        .and_then(crate::commands::ingest::claude::parse_ts_ms)
        .unwrap_or(0)
}

/// Scan one file for the *last* `token_count` line with rate limits.
pub fn rate_limits_in_file(path: &Path) -> Option<LogRateLimits> {
    let (text, _) = crate::commands::ingest::read_from_offset(path, 0).ok()?;
    let mut found = None;
    for line in text.lines() {
        // Cheap pre-filter: JSON parsing every line of a big rollout is slow.
        if !line.contains("rate_limits") || !line.contains("token_count") {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<LogLine>(line) else {
            continue;
        };
        if parsed.kind != "event_msg" {
            continue;
        }
        let Some(payload) = parsed.payload else {
            continue;
        };
        if payload.kind != "token_count" {
            continue;
        }
        if let Some(mut rl) = payload.rate_limits {
            if rl.primary.is_none() && rl.secondary.is_none() {
                continue;
            }
            rl.observed_at = parsed
                .timestamp
                .as_deref()
                .and_then(super::normalize_rfc3339)
                .or_else(|| {
                    path.metadata()
                        .ok()?
                        .modified()
                        .ok()?
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .and_then(|age| rfc3339_from_unix_secs(age.as_secs().try_into().ok()?))
                });
            found = Some(rl);
        }
    }
    found
}

/// Build a degraded quota out of the local session logs, falling back to the
/// on-disk cache when the logs have nothing either.
fn from_local_logs(ctx: &ProviderCtx, status: ProviderStatus, message: &str) -> ProviderQuota {
    let from_log = [log_root(), archived_log_root()]
        .into_iter()
        .flatten()
        .filter_map(|root| rate_limits_from_logs(&root))
        .max_by_key(|limits| observed_ms(limits.observed_at.as_deref()));
    let Some(rl) = from_log else {
        return degraded(ctx, CODEX_ID, DISPLAY_NAME, status, message);
    };
    let windows = map_log_rate_limits(&rl);
    if windows.is_empty() {
        return degraded(ctx, CODEX_ID, DISPLAY_NAME, status, message);
    }
    if let Some(cache) = super::read_cache(ctx, CODEX_ID) {
        if observed_ms(Some(&cache.fetched_at)) > observed_ms(rl.observed_at.as_deref()) {
            return degraded(ctx, CODEX_ID, DISPLAY_NAME, status, message);
        }
    }
    let mut q = empty_quota(CODEX_ID, DISPLAY_NAME, status);
    q.windows = windows;
    q.plan = rl.plan_type.clone();
    q.plan_label = plan_label(rl.plan_type.as_deref());
    q.credits = credits_from(&rl.credits);
    q.source = DataSource::LocalLog;
    q.fetched_at = rl
        .observed_at
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".into());
    q.error = Some(message.to_string());
    q
}

// ---------- provider ----------

pub struct CodexProvider {
    ctx: ProviderCtx,
}

impl CodexProvider {
    pub fn new(ctx: ProviderCtx) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Provider for CodexProvider {
    fn id(&self) -> &'static str {
        CODEX_ID
    }

    fn display_name(&self) -> &'static str {
        DISPLAY_NAME
    }

    fn info(&self) -> ProviderInfo {
        let auth = load_auth();
        let claims = auth
            .as_ref()
            .and_then(|a| a.tokens.as_ref())
            .and_then(|t| t.access_token.as_deref())
            .and_then(parse_jwt_claims);
        let now = chrono::Utc::now().timestamp();
        let logged_in = auth
            .as_ref()
            .map(|a| {
                a.auth_mode.as_deref() != Some("apikey")
                    && a.tokens
                        .as_ref()
                        .and_then(|t| t.access_token.as_ref())
                        .is_some()
            })
            .unwrap_or(false)
            && !claims.as_ref().map(|c| c.is_expired(now)).unwrap_or(false);
        ProviderInfo {
            id: CODEX_ID.into(),
            display_name: DISPLAY_NAME.into(),
            logged_in,
            credential_path: credentials_path().map(|p| p.display().to_string()),
            log_path: log_root().map(|p| p.display().to_string()),
            plan_label: claims.and_then(|c| plan_label(c.plan_type.as_deref())),
        }
    }

    async fn fetch(&self, http: &reqwest::Client) -> ProviderQuota {
        let Some(auth) = load_auth() else {
            let mut q = empty_quota(CODEX_ID, DISPLAY_NAME, ProviderStatus::NotLoggedIn);
            q.error = Some("Not logged in — run `codex login` to sign in".into());
            return q;
        };
        let token = auth
            .tokens
            .as_ref()
            .and_then(|t| t.access_token.clone())
            .filter(|t| !t.is_empty());
        if auth.auth_mode.as_deref() == Some("apikey") || token.is_none() {
            let mut q = empty_quota(CODEX_ID, DISPLAY_NAME, ProviderStatus::NotLoggedIn);
            q.error = Some(API_KEY_HINT.into());
            return q;
        }
        let token = token.expect("checked above");
        let claims = parse_jwt_claims(&token).unwrap_or_default();
        let account_id = auth
            .tokens
            .as_ref()
            .and_then(|t| t.account_id.clone())
            .or_else(|| claims.account_id.clone())
            .unwrap_or_default();

        if claims.is_expired(chrono::Utc::now().timestamp()) {
            log::info!("codex: access token expired, falling back to session logs");
            return from_local_logs(&self.ctx, ProviderStatus::TokenExpired, EXPIRED_MESSAGE);
        }

        let mut req = http
            .get(USAGE_URL)
            .bearer_auth(&token)
            .header("User-Agent", self.ctx.user_agent.clone())
            .header("Accept", "application/json");
        if !account_id.is_empty() {
            req = req.header("ChatGPT-Account-Id", account_id);
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                log::warn!("codex usage request failed: {e}");
                return from_local_logs(
                    &self.ctx,
                    ProviderStatus::Error,
                    &format!("Could not reach chatgpt.com: {e}"),
                );
            }
        };
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return from_local_logs(&self.ctx, ProviderStatus::TokenExpired, EXPIRED_MESSAGE);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = super::retry_after_of(resp.headers());
            log::warn!(
                "codex usage API answered HTTP 429 (Retry-After: {})",
                retry_after.map_or("absent".to_string(), |s| format!("{s}s"))
            );
            let mut q = from_local_logs(
                &self.ctx,
                ProviderStatus::RateLimited,
                "ChatGPT is rate-limiting the usage endpoint (HTTP 429); showing the last known values until the next attempt",
            );
            q.next_attempt_at = super::next_attempt_at(retry_after);
            return q;
        }
        if !status.is_success() {
            return from_local_logs(
                &self.ctx,
                ProviderStatus::Error,
                &format!("Codex usage API returned HTTP {}", status.as_u16()),
            );
        }
        // Read the body first: `Response::json` reports a schema mismatch as
        // an opaque "error decoding response body" without naming the field.
        let body = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                log::warn!("codex usage body could not be read: {e}");
                return from_local_logs(
                    &self.ctx,
                    ProviderStatus::Error,
                    &format!("Could not read the Codex usage response: {e}"),
                );
            }
        };
        let usage: UsageResponse = match serde_json::from_slice(&body) {
            Ok(u) => u,
            Err(e) => {
                // serde's message carries the field/position, never a value.
                log::warn!("codex usage response has an unexpected shape: {e}");
                return from_local_logs(
                    &self.ctx,
                    ProviderStatus::Error,
                    &format!("Unexpected usage response: {e}"),
                );
            }
        };

        let plan = usage.plan_type.clone().or_else(|| claims.plan_type.clone());
        let mut quota = empty_quota(CODEX_ID, DISPLAY_NAME, ProviderStatus::Ok);
        quota.windows = map_windows(&usage);
        quota.plan_label = plan_label(plan.as_deref());
        quota.plan = plan;
        quota.account = usage.email.clone().map(|email| crate::model::AccountInfo {
            email: Some(email),
            name: None,
        });
        quota.credits = credits_from(&usage.credits);
        quota.source = DataSource::Api;
        quota.fetched_at = now_rfc3339();
        write_cache(&self.ctx, &quota);
        quota
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// prolite: only a weekly window, and it is the *primary* one.
    const PROLITE: &str = r#"{
      "plan_type":"prolite","email":"user@example.com",
      "rate_limit":{"allowed":true,"limit_reached":false,
        "primary_window":{"used_percent":75,"limit_window_seconds":604800,
                          "reset_after_seconds":369313,"reset_at":1789807722},
        "secondary_window":null},
      "additional_rate_limits":[
        {"limit_name":"GPT-5.3-Codex-Spark","metered_feature":"codex_bengalfox",
         "rate_limit":{"primary_window":{"used_percent":0,"limit_window_seconds":18000,"reset_at":1789456410},
                       "secondary_window":{"used_percent":0,"limit_window_seconds":604800,"reset_at":1790043210}}}],
      "credits":{"has_credits":false,"unlimited":false,"balance":"0"},
      "unknown_new_field":123
    }"#;

    /// plus: 5-hour primary + weekly secondary.
    const PLUS: &str = r#"{
      "plan_type":"plus",
      "rate_limit":{"primary_window":{"used_percent":12.5,"limit_window_seconds":18000,"reset_at":1789456410},
                    "secondary_window":{"used_percent":44,"limit_window_seconds":604800,"reset_at":1790043210}}
    }"#;

    /// 2026-09: the endpoint began sending explicit nulls where it used to
    /// omit fields, which failed the whole response ("error decoding response
    /// body") and blanked the ring.
    #[test]
    fn explicit_nulls_and_drifting_types_do_not_discard_the_response() {
        let usage: UsageResponse = serde_json::from_str(
            r#"{
              "plan_type": "prolite",
              "rate_limit": {
                "allowed": true,
                "primary_window": {
                  "used_percent": 4,
                  "limit_window_seconds": 604800.0,
                  "reset_after_seconds": 597660,
                  "reset_at": "soon"
                },
                "secondary_window": null
              },
              "code_review_rate_limit": null,
              "additional_rate_limits": null,
              "model_usage": {"some-model": {"available": true}},
              "credits": {"has_credits": null, "unlimited": false, "balance": 0},
              "promo": null
            }"#,
        )
        .unwrap();
        assert!(usage.additional_rate_limits.is_empty());
        assert_eq!(
            usage.credits.as_ref().unwrap().balance.as_deref(),
            Some("0")
        );
        let windows = map_windows(&usage);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].kind, WindowKind::SevenDay);
        assert_eq!(windows[0].used_percent, 4.0);
    }

    #[test]
    fn prolite_weekly_is_primary_and_scoped_windows_never_are() {
        let usage: UsageResponse = serde_json::from_str(PROLITE).unwrap();
        let w = map_windows(&usage);
        assert_eq!(w.len(), 3);

        assert_eq!(w[0].kind, WindowKind::SevenDay);
        assert_eq!(w[0].label, "Weekly");
        assert_eq!(w[0].used_percent, 75.0);
        assert_eq!(w[0].window_seconds, Some(604_800));
        assert_eq!(w[0].resets_at.as_deref(), Some("2026-09-19T08:48:42Z"));
        assert!(w[0].is_primary);

        assert_eq!(w[1].scope.as_deref(), Some("GPT-5.3-Codex-Spark"));
        assert_eq!(w[1].kind, WindowKind::FiveHour);
        assert_eq!(w[1].label, "GPT-5.3-Codex-Spark · 5-hour");
        assert!(!w[1].is_primary);
        assert_eq!(w[2].label, "GPT-5.3-Codex-Spark · Weekly");
        assert!(!w[2].is_primary);

        assert_eq!(
            plan_label(usage.plan_type.as_deref()).as_deref(),
            Some("ChatGPT Pro Lite")
        );
        let c = credits_from(&usage.credits).unwrap();
        assert!(!c.has_credits);
        assert_eq!(c.balance.as_deref(), Some("0"));
    }

    #[test]
    fn plus_five_hour_primary_plus_weekly_secondary() {
        let usage: UsageResponse = serde_json::from_str(PLUS).unwrap();
        let w = map_windows(&usage);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].kind, WindowKind::FiveHour);
        assert_eq!(w[0].label, "5-hour");
        assert_eq!(w[0].used_percent, 12.5);
        assert!(w[0].is_primary);
        assert_eq!(w[1].kind, WindowKind::SevenDay);
        assert_eq!(w[1].label, "Weekly");
        assert!(!w[1].is_primary);
        assert_eq!(
            plan_label(usage.plan_type.as_deref()).as_deref(),
            Some("ChatGPT Plus")
        );
    }

    #[test]
    fn odd_window_lengths_are_labelled_by_hours() {
        let usage: UsageResponse = serde_json::from_str(
            r#"{"rate_limit":{"primary_window":{"used_percent":5,"limit_window_seconds":86400}}}"#,
        )
        .unwrap();
        let w = map_windows(&usage);
        assert_eq!(w[0].kind, WindowKind::Other);
        assert_eq!(w[0].label, "24h");
        assert!(w[0].is_primary, "a lone window is always primary");
    }

    #[test]
    fn plan_labels() {
        for (raw, want) in [
            ("plus", "ChatGPT Plus"),
            ("pro", "ChatGPT Pro"),
            ("prolite", "ChatGPT Pro Lite"),
            ("free", "ChatGPT Free"),
            ("team", "ChatGPT Team"),
            ("business", "ChatGPT Business"),
            ("enterprise", "ChatGPT Enterprise"),
            ("edu", "ChatGPT Edu"),
            ("unobtainium", "ChatGPT unobtainium"),
        ] {
            assert_eq!(plan_label(Some(raw)).as_deref(), Some(want));
        }
        assert_eq!(plan_label(None), None);
        assert_eq!(plan_label(Some("  ")), None);
    }

    /// A synthetic unsigned token: `header.payload.` with a fake signature.
    fn synthetic_jwt(payload: &serde_json::Value) -> String {
        let b64 = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
        format!(
            "{}.{}.{}",
            b64(br#"{"alg":"none","typ":"JWT"}"#),
            b64(serde_json::to_string(payload).unwrap().as_bytes()),
            "not-a-real-signature"
        )
    }

    #[test]
    fn jwt_claims_are_parsed_without_verification() {
        let token = synthetic_jwt(&serde_json::json!({
            "exp": 1790217892i64,
            "sub": "user-123",
            "https://api.openai.com/auth": {
                "chatgpt_plan_type": "prolite",
                "chatgpt_account_id": "4f0c1e15-f7ce-46c0-a973-2000a8902176"
            },
            "https://api.openai.com/profile": {"email": "user@example.com"}
        }));
        let c = parse_jwt_claims(&token).unwrap();
        assert_eq!(c.exp, Some(1_790_217_892));
        assert_eq!(c.plan_type.as_deref(), Some("prolite"));
        assert_eq!(
            c.account_id.as_deref(),
            Some("4f0c1e15-f7ce-46c0-a973-2000a8902176")
        );
        assert!(c.is_expired(1_790_217_893));
        assert!(!c.is_expired(1_000_000_000));

        // no claims at all, and malformed input
        let empty = parse_jwt_claims(&synthetic_jwt(&serde_json::json!({}))).unwrap();
        assert_eq!(empty, JwtClaims::default());
        assert!(!empty.is_expired(i64::MAX));
        assert!(parse_jwt_claims("nodots").is_none());
        assert!(parse_jwt_claims("a.!!!not-base64!!!.c").is_none());
    }

    #[test]
    fn auth_file_parsing() {
        let a: AuthFile = serde_json::from_str(
            r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,
                "tokens":{"id_token":"x","access_token":"y","refresh_token":"z","account_id":"acc"},
                "last_refresh":"2026-09-14T00:00:00Z","future":"ignored"}"#,
        )
        .unwrap();
        assert_eq!(a.auth_mode.as_deref(), Some("chatgpt"));
        assert_eq!(a.tokens.unwrap().account_id.as_deref(), Some("acc"));

        let k: AuthFile =
            serde_json::from_str(r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-x"}"#).unwrap();
        assert_eq!(k.auth_mode.as_deref(), Some("apikey"));
        assert!(k.tokens.is_none());
    }

    #[test]
    fn local_log_fallback_mapping() {
        let dir = tempdir();
        let path = dir.join("rollout-test.jsonl");
        // Two token_count lines; the *last* one wins.
        let lines = [
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{},"rate_limits":{"primary":{"used_percent":10.0,"window_minutes":10080,"resets_at":1789807722},"secondary":null,"plan_type":"prolite"}}}"#,
            r#"{"type":"response_item","payload":{"type":"message"}}"#,
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{},"rate_limits":{"primary":{"used_percent":75.0,"window_minutes":10080,"resets_at":1789807722},"secondary":null,"plan_type":"prolite","credits":{"has_credits":false,"unlimited":false,"balance":"0"}}}}"#,
            "not json at all",
        ];
        std::fs::write(&path, lines.join("\n")).unwrap();

        let rl = rate_limits_in_file(&path).unwrap();
        assert_eq!(rl.plan_type.as_deref(), Some("prolite"));
        let w = map_log_rate_limits(&rl);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].used_percent, 75.0);
        assert_eq!(w[0].kind, WindowKind::SevenDay);
        assert_eq!(w[0].window_seconds, Some(604_800));
        assert!(w[0].is_primary);

        assert!(rate_limits_from_logs(&dir).is_some());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn local_log_age_uses_record_time_and_empty_limits_do_not_erase_it() {
        let dir = tempdir();
        let path = dir.join("rollout.jsonl");
        let record = |timestamp: &str, percent: u32| {
            format!(
                r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","rate_limits":{{"primary":{{"used_percent":{percent},"window_minutes":10080}}}}}}}}"#
            )
        };
        std::fs::write(&path, format!("{}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"rate_limits\":{{}}}}}}\n", record("2026-09-15T01:00:00Z", 40))).unwrap();
        let limits = rate_limits_in_file(&path).unwrap();
        assert_eq!(limits.observed_at.as_deref(), Some("2026-09-15T01:00:00Z"));
        assert_eq!(map_log_rate_limits(&limits)[0].used_percent, 40.0);
        std::fs::write(
            dir.join("newer.jsonl"),
            format!("{}\n", record("2026-09-17T02:00:00Z", 60)),
        )
        .unwrap();
        assert_eq!(
            rate_limits_from_logs(&dir).unwrap().observed_at.as_deref(),
            Some("2026-09-17T02:00:00Z")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    pub(crate) fn tempdir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "ai-usage-sidebar-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
