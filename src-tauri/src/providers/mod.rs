//! Provider abstraction + shared helpers. [BACKEND owns this directory]
//!
//! Providers are deliberately free of any Tauri dependency: they take a
//! `ProviderCtx` (directories + user agent) and a `reqwest::Client`, so the
//! `probe` example can exercise exactly the same code path as the app.

pub mod claude;
pub mod codex;

use crate::model::{
    AppSnapshot, DataSource, ProviderInfo, ProviderQuota, ProviderStatus, QuotaWindow, Settings,
    WindowKind,
};
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Duration;

pub const CLAUDE_ID: &str = "claude";
pub const CODEX_ID: &str = "codex";

/// Everything a provider needs that is not the HTTP client.
#[derive(Clone, Debug)]
pub struct ProviderCtx {
    /// `<data_dir>/cache`; `None` disables the on-disk last-good cache.
    pub cache_dir: Option<PathBuf>,
    /// value sent as `User-Agent`
    pub user_agent: String,
}

impl Default for ProviderCtx {
    fn default() -> Self {
        Self {
            cache_dir: None,
            user_agent: default_user_agent(),
        }
    }
}

impl ProviderCtx {
    /// Context rooted at the Tauri app data dir.
    pub fn with_data_dir(data_dir: &std::path::Path) -> Self {
        Self {
            cache_dir: Some(data_dir.join("cache")),
            user_agent: default_user_agent(),
        }
    }
}

pub fn default_user_agent() -> String {
    format!("ai-usage-sidebar/{}", env!("CARGO_PKG_VERSION"))
}

/// One quota source (Claude Code, Codex, …).
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stable id used in settings, the DB and the UI (`"claude"`, `"codex"`).
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    /// Cheap, offline description (credential/log paths, login state, plan).
    fn info(&self) -> ProviderInfo;
    /// Fetch live quota. Never fails: transport problems are reported through
    /// `ProviderQuota.status` / `.error` together with the last known windows.
    async fn fetch(&self, http: &reqwest::Client) -> ProviderQuota;
}

/// The provider registry, in default display order.
pub fn all_providers(ctx: &ProviderCtx) -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(claude::ClaudeProvider::new(ctx.clone())),
        Box::new(codex::CodexProvider::new(ctx.clone())),
    ]
}

/// Shared HTTP client (15 s timeout, gzip, provider-neutral user agent).
pub fn http_client(user_agent: &str) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(10))
        .user_agent(user_agent.to_string())
        .build()
        .unwrap_or_default()
}

// ---------- window helpers ----------

/// Clamp a percentage into 0..=100, mapping NaN to 0.
pub fn clamp_percent(p: f64) -> f64 {
    if p.is_finite() {
        p.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

/// Default human label for a window of `secs` seconds.
pub fn window_label(secs: Option<u64>) -> String {
    match secs.map(WindowKind::from_seconds) {
        Some(WindowKind::FiveHour) => "5-hour".to_string(),
        Some(WindowKind::SevenDay) => "Weekly".to_string(),
        _ => match secs {
            Some(s) if s >= 3600 => format!("{}h", (s as f64 / 3600.0).round() as u64),
            Some(s) => format!("{}m", (s as f64 / 60.0).round().max(1.0) as u64),
            None => "Window".to_string(),
        },
    }
}

/// Mark exactly one window primary: the non-scoped 5-hour window if the plan
/// has one, else the non-scoped weekly window, else the first window.
pub fn mark_primary(windows: &mut [QuotaWindow]) {
    for w in windows.iter_mut() {
        w.is_primary = false;
    }
    let pick = windows
        .iter()
        .position(|w| w.scope.is_none() && w.kind == WindowKind::FiveHour)
        .or_else(|| {
            windows
                .iter()
                .position(|w| w.scope.is_none() && w.kind == WindowKind::SevenDay)
        })
        .or_else(|| windows.iter().position(|w| w.scope.is_none()))
        .or(if windows.is_empty() { None } else { Some(0) });
    if let Some(i) = pick {
        windows[i].is_primary = true;
    }
}

// ---------- time helpers ----------

/// `now` as an RFC 3339 UTC string (second precision).
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// RFC 3339 → unix **milliseconds**.
pub fn rfc3339_to_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// Unix **seconds** → RFC 3339 UTC.
pub fn rfc3339_from_unix_secs(secs: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Unix **milliseconds** → RFC 3339 UTC.
pub fn rfc3339_from_unix_ms(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Re-format any RFC 3339 timestamp as UTC with second precision.
pub fn normalize_rfc3339(s: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| {
        d.with_timezone(&chrono::Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    })
}

// ---------- last-good cache ----------

fn cache_path(ctx: &ProviderCtx, provider: &str) -> Option<PathBuf> {
    ctx.cache_dir
        .as_ref()
        .map(|d| d.join(format!("quota-{provider}.json")))
}

/// Last successfully fetched quota for `provider`, if any.
pub fn read_cache(ctx: &ProviderCtx, provider: &str) -> Option<ProviderQuota> {
    let path = cache_path(ctx, provider)?;
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Persist a successful fetch so the UI always has something to show.
pub fn write_cache(ctx: &ProviderCtx, quota: &ProviderQuota) {
    let Some(path) = cache_path(ctx, &quota.provider) else {
        return;
    };
    let Ok(bytes) = serde_json::to_vec(quota) else {
        return;
    };
    if let Err(e) = crate::commands::settings::write_atomic(&path, &bytes) {
        log::debug!("could not cache {} quota: {}", quota.provider, e);
    }
}

// ---------- quota construction helpers ----------

/// Skeleton quota with no windows.
pub fn empty_quota(provider: &str, display_name: &str, status: ProviderStatus) -> ProviderQuota {
    ProviderQuota {
        provider: provider.to_string(),
        display_name: display_name.to_string(),
        plan: None,
        plan_label: None,
        account: None,
        windows: Vec::new(),
        fetched_at: now_rfc3339(),
        source: DataSource::Api,
        status,
        error: None,
        credits: None,
        next_attempt_at: None,
    }
}

/// Degraded result: keep the last known windows (disk cache) but report the
/// real status and message. Used for `token_expired` / `error` / `not_logged_in`.
pub fn degraded(
    ctx: &ProviderCtx,
    provider: &str,
    display_name: &str,
    status: ProviderStatus,
    message: impl Into<String>,
) -> ProviderQuota {
    let mut q = empty_quota(provider, display_name, status);
    q.error = Some(message.into());
    if let Some(cached) = read_cache(ctx, provider) {
        q.windows = cached.windows;
        q.plan = cached.plan;
        q.plan_label = cached.plan_label;
        q.account = cached.account;
        q.credits = cached.credits;
        q.source = DataSource::Cache;
        // `fetchedAt` of the *data*, not of this attempt, so the UI can age it.
        q.fetched_at = cached.fetched_at;
    }
    q
}

// ---------- rate limiting ----------

/// Bounds for a server-supplied `Retry-After`. A value below the minimum is
/// not worth obeying literally (we would walk straight back into the limit),
/// one above the maximum would freeze the widget for the rest of the day.
pub const RETRY_AFTER_MIN_SECS: u64 = 30;
pub const RETRY_AFTER_MAX_SECS: u64 = 3_600;
/// Used when the server sends `429` without a usable `Retry-After`.
pub const RETRY_AFTER_DEFAULT_SECS: u64 = 300;

/// Parse an HTTP `Retry-After` header into seconds from `now`.
///
/// Both forms of RFC 9110 §10.2.3 are accepted: delta-seconds (`"120"`) and
/// an HTTP-date (`"Wed, 21 Oct 2026 07:28:00 GMT"`). The result is clamped
/// into `RETRY_AFTER_MIN_SECS..=RETRY_AFTER_MAX_SECS`; anything unparsable is
/// `None` so the caller can fall back to its own schedule.
pub fn parse_retry_after(value: &str, now: chrono::DateTime<chrono::Utc>) -> Option<u64> {
    let raw = value.trim();
    if raw.is_empty() {
        return None;
    }
    let secs = if let Ok(delta) = raw.parse::<i64>() {
        delta
    } else {
        let parsed = chrono::DateTime::parse_from_rfc2822(raw)
            .map(|d| d.with_timezone(&chrono::Utc))
            .or_else(|_| {
                chrono::NaiveDateTime::parse_from_str(raw, "%a, %d %b %Y %H:%M:%S GMT")
                    .map(|d| d.and_utc())
            })
            .ok()?;
        (parsed - now).num_seconds()
    };
    Some((secs.max(0) as u64).clamp(RETRY_AFTER_MIN_SECS, RETRY_AFTER_MAX_SECS))
}

/// `Retry-After` of a response, already parsed and clamped.
pub fn retry_after_of(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    parse_retry_after(value, chrono::Utc::now())
}

/// RFC 3339 timestamp `retry_after_secs` (or the default wait) from now.
pub fn next_attempt_at(retry_after_secs: Option<u64>) -> Option<String> {
    let wait = retry_after_secs.unwrap_or(RETRY_AFTER_DEFAULT_SECS);
    rfc3339_from_unix_secs(chrono::Utc::now().timestamp() + wait as i64)
}

/// `HTTP 429` result: the last good windows stay on screen, the status says
/// why they are ageing and `next_attempt_at` says when the app may try again.
/// The scheduler may push that time further out (see `scheduler::PollInput`).
pub fn rate_limited(
    ctx: &ProviderCtx,
    provider: &str,
    display_name: &str,
    retry_after_secs: Option<u64>,
    message: impl Into<String>,
) -> ProviderQuota {
    let mut q = degraded(
        ctx,
        provider,
        display_name,
        ProviderStatus::RateLimited,
        message,
    );
    q.next_attempt_at = next_attempt_at(retry_after_secs);
    q
}

/// Expand `~` is not supported by `dirs`; this resolves the user home dir.
pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// A provider the user switched off in the settings.
pub fn disabled_quota(provider: &str, display_name: &str) -> ProviderQuota {
    empty_quota(provider, display_name, ProviderStatus::Disabled)
}

/// `true` when the settings do not explicitly disable `id`.
pub fn is_enabled(settings: &Settings, id: &str) -> bool {
    settings
        .providers
        .get(id)
        .map(|p| p.enabled)
        .unwrap_or(true)
}

/// Providers in the user's configured display order.
pub fn ordered_providers(ctx: &ProviderCtx, settings: &Settings) -> Vec<Box<dyn Provider>> {
    let mut list = all_providers(ctx);
    list.sort_by_key(|p| {
        settings
            .providers
            .get(p.id())
            .map(|s| s.order)
            .unwrap_or(i32::MAX)
    });
    list
}

/// Offline snapshot built purely from the on-disk `cache/quota-*.json` files,
/// so the UI has something to render before the first network round trip.
pub fn snapshot_from_cache(ctx: &ProviderCtx, settings: &Settings) -> AppSnapshot {
    let providers = ordered_providers(ctx, settings)
        .iter()
        .map(|p| {
            if !is_enabled(settings, p.id()) {
                return disabled_quota(p.id(), p.display_name());
            }
            match read_cache(ctx, p.id()) {
                Some(mut q) => {
                    q.source = DataSource::Cache;
                    q
                }
                None => empty_quota(p.id(), p.display_name(), ProviderStatus::Error),
            }
        })
        .collect();
    AppSnapshot {
        providers,
        generated_at: now_rfc3339(),
    }
}

/// Fetch quotas for every provider.
///
/// * `only` — refresh just this provider (others keep their previous value).
/// * `skip` — called for each candidate; `true` keeps the previous value
///   (the scheduler uses it to honour the per-provider error backoff).
///
/// Never fails: unreachable providers come back with a non-`ok` status.
pub async fn fetch_snapshot(
    ctx: &ProviderCtx,
    http: &reqwest::Client,
    settings: &Settings,
    only: Option<&str>,
    previous: &AppSnapshot,
    mut skip: impl FnMut(&str) -> bool,
) -> AppSnapshot {
    let mut out = Vec::new();
    for p in ordered_providers(ctx, settings) {
        let id = p.id();
        let prev = previous
            .providers
            .iter()
            .find(|q| q.provider == id)
            .cloned();
        if !is_enabled(settings, id) {
            out.push(disabled_quota(id, p.display_name()));
            continue;
        }
        let wanted = only.map(|o| o == id).unwrap_or(true);
        if !wanted || skip(id) {
            out.push(
                prev.unwrap_or_else(|| empty_quota(id, p.display_name(), ProviderStatus::Error)),
            );
            continue;
        }
        let quota = p.fetch(http).await;
        log::debug!(
            "{} refresh -> {:?} ({} windows)",
            id,
            quota.status,
            quota.windows.len()
        );
        out.push(quota);
    }
    AppSnapshot {
        providers: out,
        generated_at: now_rfc3339(),
    }
}

/// Offline description of every provider (login state, paths, plan).
pub fn provider_infos(ctx: &ProviderCtx, settings: &Settings) -> Vec<ProviderInfo> {
    ordered_providers(ctx, settings)
        .iter()
        .map(|p| p.info())
        .collect()
}

/// Serde helper for undocumented provider APIs: an explicit `null` reads as the
/// type's default. `#[serde(default)]` alone only covers an *absent* field, so
/// without this one `"list": null` discards the entire response.
pub(crate) fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + serde::Deserialize<'de>,
{
    Ok(<Option<T> as serde::Deserialize>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(kind: WindowKind, scope: Option<&str>) -> QuotaWindow {
        QuotaWindow {
            kind,
            label: "x".into(),
            window_seconds: None,
            used_percent: 0.0,
            resets_at: None,
            scope: scope.map(|s| s.to_string()),
            is_primary: false,
        }
    }

    #[test]
    fn retry_after_accepts_both_header_forms_and_stays_in_range() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-21T07:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        // delta-seconds
        assert_eq!(parse_retry_after("120", now), Some(120));
        assert_eq!(parse_retry_after("  600 ", now), Some(600));
        // HTTP-date (RFC 9110 §10.2.3)
        assert_eq!(
            parse_retry_after("Wed, 21 Oct 2026 07:28:00 GMT", now),
            Some(1_680)
        );
        // clamped: too eager, too far away, already in the past
        assert_eq!(parse_retry_after("1", now), Some(RETRY_AFTER_MIN_SECS));
        assert_eq!(parse_retry_after("99999", now), Some(RETRY_AFTER_MAX_SECS));
        assert_eq!(parse_retry_after("-5", now), Some(RETRY_AFTER_MIN_SECS));
        assert_eq!(
            parse_retry_after("Wed, 21 Oct 2026 06:00:00 GMT", now),
            Some(RETRY_AFTER_MIN_SECS)
        );
        // unusable values fall back to the caller's own schedule
        assert_eq!(parse_retry_after("", now), None);
        assert_eq!(parse_retry_after("soon", now), None);
    }

    #[test]
    fn window_kind_classification() {
        assert_eq!(WindowKind::from_seconds(18_000), WindowKind::FiveHour);
        assert_eq!(WindowKind::from_seconds(3_600), WindowKind::FiveHour);
        assert_eq!(WindowKind::from_seconds(21_600), WindowKind::FiveHour);
        assert_eq!(WindowKind::from_seconds(604_800), WindowKind::SevenDay);
        assert_eq!(WindowKind::from_seconds(6 * 86_400), WindowKind::SevenDay);
        assert_eq!(WindowKind::from_seconds(8 * 86_400), WindowKind::SevenDay);
        assert_eq!(WindowKind::from_seconds(86_400), WindowKind::Other);
        assert_eq!(WindowKind::from_seconds(30 * 86_400), WindowKind::Other);
    }

    #[test]
    fn labels_follow_the_window_length() {
        assert_eq!(window_label(Some(18_000)), "5-hour");
        assert_eq!(window_label(Some(604_800)), "Weekly");
        assert_eq!(window_label(Some(86_400)), "24h");
        assert_eq!(window_label(None), "Window");
    }

    #[test]
    fn primary_prefers_five_hour_then_weekly() {
        let mut ws = vec![
            win(WindowKind::SevenDay, None),
            win(WindowKind::FiveHour, None),
            win(WindowKind::SevenDay, Some("Fable")),
        ];
        mark_primary(&mut ws);
        assert!(ws[1].is_primary);
        assert_eq!(ws.iter().filter(|w| w.is_primary).count(), 1);

        // prolite: weekly only
        let mut ws = vec![win(WindowKind::SevenDay, None)];
        mark_primary(&mut ws);
        assert!(ws[0].is_primary);

        // scoped windows never win over a non-scoped one
        let mut ws = vec![
            win(WindowKind::FiveHour, Some("Spark")),
            win(WindowKind::SevenDay, None),
        ];
        mark_primary(&mut ws);
        assert!(!ws[0].is_primary);
        assert!(ws[1].is_primary);

        // only scoped windows → first one
        let mut ws = vec![win(WindowKind::FiveHour, Some("Spark"))];
        mark_primary(&mut ws);
        assert!(ws[0].is_primary);

        let mut ws: Vec<QuotaWindow> = vec![];
        mark_primary(&mut ws);
    }

    #[test]
    fn percent_is_clamped() {
        assert_eq!(clamp_percent(-3.0), 0.0);
        assert_eq!(clamp_percent(140.0), 100.0);
        assert_eq!(clamp_percent(f64::NAN), 0.0);
        assert_eq!(clamp_percent(42.5), 42.5);
    }

    #[test]
    fn rfc3339_helpers_round_trip() {
        let s = rfc3339_from_unix_secs(0).unwrap();
        assert_eq!(s, "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_from_unix_ms(1_000).unwrap(), "1970-01-01T00:00:01Z");
        assert_eq!(
            normalize_rfc3339("2026-09-15T06:20:00.525200+00:00").unwrap(),
            "2026-09-15T06:20:00Z"
        );
        assert!(normalize_rfc3339("not a time").is_none());
    }
}
