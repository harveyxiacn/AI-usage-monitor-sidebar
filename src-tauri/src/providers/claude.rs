//! Claude Code (Anthropic OAuth) quota provider. [BACKEND]
//!
//! Credentials come from `$CLAUDE_CONFIG_DIR/.credentials.json` (default
//! `~/.claude/.credentials.json`); on macOS the same JSON lives in the Keychain
//! under the service name `Claude Code-credentials`. For an extra account (a
//! non-default `CLAUDE_CONFIG_DIR`) the file is tried first and then, on macOS,
//! the Keychain item `Claude Code-credentials-<first 8 hex of sha256(dir)>`
//! (see [`keychain_service_for`] for the sources and their limits).

use super::{
    clamp_percent, degraded, empty_quota, mark_primary, normalize_rfc3339, now_rfc3339,
    rate_limited, retry_after_of, write_cache, Provider, ProviderCtx, CLAUDE_ID,
};
use crate::model::{
    AccountInfo, CreditsInfo, DataSource, ExtraSeverity, ProviderInfo, ProviderQuota,
    ProviderStatus, QuotaExtra, QuotaWindow, WindowKind,
};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

/// Treat a token as expired this long before its stated expiry, so a clock
/// that runs slightly ahead of Anthropic's does not send a dead token.
pub const EXPIRY_SKEW_MS: i64 = 60_000;

impl Credentials {
    /// Expired, or about to be (`EXPIRY_SKEW_MS`).
    pub fn needs_refresh(&self, now_ms: i64) -> bool {
        self.is_expired(now_ms.saturating_add(EXPIRY_SKEW_MS))
    }
}

/// Decide which credentials a fetch may use. Claude Code refreshes its own
/// token in the background, so when the ones read first look expired the
/// file/Keychain is read once more (`reload`) before giving up. `None` means
/// the login really is expired.
pub fn usable_credentials(
    first: Credentials,
    now_ms: i64,
    reload: impl FnOnce() -> Option<Credentials>,
) -> Option<Credentials> {
    if !first.needs_refresh(now_ms) {
        return Some(first);
    }
    reload().filter(|c| !c.needs_refresh(now_ms))
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

/// Keychain item Claude Code uses on macOS instead of a credentials file.
pub const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// Where the credentials actually come from on this machine, for diagnostics.
/// On macOS that is normally the login Keychain, not a file that exists.
pub fn credential_source() -> Option<String> {
    let path = credentials_path();
    #[cfg(target_os = "macos")]
    if !path.as_ref().map(|p| p.is_file()).unwrap_or(false) {
        return Some(format!("Keychain: {KEYCHAIN_SERVICE}"));
    }
    path.map(|p| p.display().to_string())
}

/// `~/.claude/projects` — the session-log root.
pub fn log_root() -> Option<PathBuf> {
    config_dir().map(|d| d.join("projects"))
}

/// `<configDir>/projects` of an extra account.
pub fn log_root_in(dir: &Path) -> PathBuf {
    dir.join("projects")
}

/// Credentials file of an extra account's config dir.
pub fn credentials_path_in(dir: &Path) -> PathBuf {
    dir.join(".credentials.json")
}

/// Credentials of an extra account: `<dir>/.credentials.json`, else (macOS
/// only) the Keychain item Claude Code files for that config dir. Never the
/// primary account's item `Claude Code-credentials`, so a missing login can
/// not be answered with another account's token.
pub fn load_credentials_from(dir: &Path) -> Option<Credentials> {
    let text = match std::fs::read_to_string(credentials_path_in(dir)) {
        Ok(t) => t,
        Err(_) => keychain_credentials_for_dir(dir)?,
    };
    parse_credentials(&text)
}

/// SHA-256 (FIPS 180-4), lower-case hex. Only used to derive a Keychain
/// service name, so no new dependency is pulled in for it.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for start in (0..msg.len()).step_by(64) {
        let mut w = [0u32; 64];
        for (i, slot) in w.iter_mut().take(16).enumerate() {
            let o = start + i * 4;
            *slot = u32::from_be_bytes([msg[o], msg[o + 1], msg[o + 2], msg[o + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(v);
        }
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}

/// Keychain service name Claude Code uses on macOS for the login of the
/// non-default `CLAUDE_CONFIG_DIR` `dir`: `Claude Code-credentials-` plus the
/// first 8 hex digits of the SHA-256 of the directory string.
///
/// **Source and confidence.** Anthropic documents only the default item
/// (`Claude Code-credentials`); the suffix rule is what independent tools
/// reverse-engineered from Claude Code (openusage #423, ccdm #69, the VS Code
/// usage extension #38, cc-donut #84; see docs/PROVIDERS.md §5). They agree
/// on the format but not on every detail, so this is a best effort: the
/// directory string is hashed exactly as given (the settings path, which the
/// user types as they would set `CLAUDE_CONFIG_DIR`; no `realpath`, and no
/// NFC normalisation, so a path with non-ASCII characters may not match).
/// A wrong guess finds no item and the account simply falls back to its
/// credentials file; it can never read another account's item, because the
/// name always carries the hash of *this* directory.
pub fn keychain_service_for(dir: &Path) -> String {
    format!(
        "{KEYCHAIN_SERVICE}-{}",
        &sha256_hex(dir.to_string_lossy().as_bytes())[..8]
    )
}

/// Service names to try for `dir`: the exact string, then the same without
/// trailing separators (a path pasted with a trailing `/`).
fn keychain_services_for(dir: &Path) -> Vec<String> {
    let exact = keychain_service_for(dir);
    let raw = dir.to_string_lossy();
    let trimmed = raw.trim_end_matches(['/', '\\']);
    let mut out = vec![exact];
    if !trimmed.is_empty() && trimmed != raw {
        let alt = keychain_service_for(Path::new(trimmed));
        if !out.contains(&alt) {
            out.push(alt);
        }
    }
    out
}

/// The Keychain item's JSON for an extra account, macOS only.
fn keychain_credentials_for_dir(dir: &Path) -> Option<String> {
    keychain_services_for(dir)
        .iter()
        .find_map(|service| keychain_credentials_of(service))
}

/// Whether the Keychain holds an item for `dir` (attributes only: no secret
/// is requested, so macOS shows no access prompt). Always `false` elsewhere.
pub fn keychain_item_exists_for_dir(dir: &Path) -> bool {
    keychain_services_for(dir)
        .iter()
        .any(|service| keychain_item_exists(service))
}

/// Why an extra account has no usable login, for the status line.
pub fn account_login_hint(dir: &Path, macos: bool) -> String {
    if macos && !credentials_path_in(dir).is_file() {
        return format!(
            "No login found for {} — no credentials file and no Keychain item `{}`. Run \
             `CLAUDE_CONFIG_DIR={} claude` and sign in with /login",
            dir.display(),
            keychain_service_for(dir),
            dir.display()
        );
    }
    format!(
        "Not logged in — run `CLAUDE_CONFIG_DIR={} claude` and sign in with /login",
        dir.display()
    )
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

/// How long a Keychain answer is reused. Reading the item shells out to
/// `security`, which is slow and can raise a Keychain access prompt, so the
/// 15 s+ refresh tick must not re-read it every round.
#[cfg(target_os = "macos")]
const KEYCHAIN_TTL: Duration = Duration::from_secs(120);

/// Answers per Keychain service name (the primary item and each extra
/// account's own).
#[cfg(target_os = "macos")]
static KEYCHAIN_CACHE: Mutex<Vec<(String, Instant, Option<String>)>> = Mutex::new(Vec::new());

#[cfg(target_os = "macos")]
fn keychain_credentials() -> Option<String> {
    keychain_credentials_of(KEYCHAIN_SERVICE)
}

#[cfg(target_os = "macos")]
fn keychain_credentials_of(service: &str) -> Option<String> {
    if let Some((_, at, cached)) = KEYCHAIN_CACHE.lock().iter().find(|(s, _, _)| s == service) {
        if at.elapsed() < KEYCHAIN_TTL {
            return cached.clone();
        }
    }
    let fresh = read_keychain_item(service);
    let mut cache = KEYCHAIN_CACHE.lock();
    cache.retain(|(s, _, _)| s != service);
    cache.push((service.to_string(), Instant::now(), fresh.clone()));
    fresh
}

#[cfg(target_os = "macos")]
fn keychain_item_exists(service: &str) -> bool {
    std::process::Command::new("security")
        .args(["find-generic-password", "-s", service])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn read_keychain_item(service: &str) -> Option<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", service, "-w"])
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

#[cfg(not(target_os = "macos"))]
fn keychain_credentials_of(_service: &str) -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
fn keychain_item_exists(_service: &str) -> bool {
    false
}

/// Drop any cached Keychain answer so the next read sees a token that
/// `claude` has just refreshed. A no-op off macOS.
pub fn forget_cached_credentials() {
    #[cfg(target_os = "macos")]
    {
        KEYCHAIN_CACHE.lock().clear();
    }
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
    #[serde(deserialize_with = "super::null_default")]
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
    #[serde(deserialize_with = "super::null_default")]
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
    #[serde(deserialize_with = "super::null_default")]
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
    #[serde(deserialize_with = "super::null_default")]
    pub has_claude_max: bool,
    #[serde(deserialize_with = "super::null_default")]
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
                forecast: None,
            }),
            "weekly_all" => out.push(QuotaWindow {
                kind: WindowKind::SevenDay,
                label: "Weekly".into(),
                window_seconds: Some(SEVEN_DAY_SECS),
                used_percent: percent,
                resets_at,
                scope: None,
                is_primary: false,
                forecast: None,
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
                    forecast: None,
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
            forecast: None,
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

/// Claude's extras. `extra_usage` is mostly already covered by the credits
/// line above; the one number that line drops is the utilization percentage,
/// and only when both `used_credits` and `monthly_limit` are known (it then
/// shows "12.50/50.00"). Surface exactly that, so nothing is shown twice.
pub fn map_extras(usage: &UsageResponse) -> Vec<QuotaExtra> {
    let Some(extra) = &usage.extra_usage else {
        return Vec::new();
    };
    if !extra.is_enabled || extra.used_credits.is_none() || extra.monthly_limit.is_none() {
        return Vec::new();
    }
    extra
        .utilization
        .map(|u| {
            vec![QuotaExtra::value(
                "extra_usage",
                format!("{:.0}%", clamp_percent(u)),
                ExtraSeverity::Info,
            )]
        })
        .unwrap_or_default()
}

// ---------- profile cache ----------

#[derive(Clone, Debug, Default)]
struct CachedProfile {
    account: Option<AccountInfo>,
    rate_limit_tier: Option<String>,
}

/// One entry per access token (so two accounts do not evict each other).
static PROFILE_CACHE: Mutex<Option<HashMap<u64, (Instant, CachedProfile)>>> = Mutex::new(None);

/// More distinct tokens than accounts can ever exist; a rotating token would
/// otherwise leave one dead entry per refresh behind.
const PROFILE_CACHE_MAX: usize = 16;

fn token_cache_key(token: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    token.hash(&mut hasher);
    hasher.finish()
}

fn cached_profile(token: &str) -> Option<CachedProfile> {
    let guard = PROFILE_CACHE.lock();
    let (at, p) = guard.as_ref()?.get(&token_cache_key(token))?;
    (at.elapsed() < PROFILE_TTL).then(|| p.clone())
}

fn store_profile(token: &str, p: CachedProfile) {
    let mut guard = PROFILE_CACHE.lock();
    let map = guard.get_or_insert_with(HashMap::new);
    map.retain(|_, (at, _)| at.elapsed() < PROFILE_TTL);
    if map.len() >= PROFILE_CACHE_MAX {
        map.clear();
    }
    map.insert(token_cache_key(token), (Instant::now(), p));
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
        if let Some(p) = cached_profile(token) {
            return Some(p);
        }
        let resp = http
            .get(self.ctx.endpoint(PROFILE_URL))
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
        store_profile(token, p.clone());
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

    fn account(&self) -> Option<&super::AccountRef> {
        self.ctx.account.as_ref()
    }

    fn info(&self) -> ProviderInfo {
        let creds = self.load();
        let now = chrono::Utc::now().timestamp_millis();
        ProviderInfo {
            id: CLAUDE_ID.into(),
            display_name: DISPLAY_NAME.into(),
            logged_in: creds.as_ref().map(|c| !c.is_expired(now)).unwrap_or(false),
            credential_path: match &self.ctx.account {
                Some(a) => Some(credentials_path_in(&a.config_dir).display().to_string()),
                None => credential_source(),
            },
            log_path: match &self.ctx.account {
                Some(a) => Some(a.config_dir.join("projects").display().to_string()),
                None => log_root().map(|p| p.display().to_string()),
            },
            plan_label: creds.as_ref().and_then(|c| {
                plan_label(c.subscription_type.as_deref(), c.rate_limit_tier.as_deref())
            }),
            experimental: false,
        }
    }

    async fn fetch(&self, http: &reqwest::Client) -> ProviderQuota {
        super::tag_account(self.fetch_primary_or_account(http).await, self.account())
    }
}

impl ClaudeProvider {
    /// Credentials of this instance's account.
    fn load(&self) -> Option<Credentials> {
        match &self.ctx.account {
            Some(a) => load_credentials_from(&a.config_dir),
            None => load_credentials(),
        }
    }

    /// Drop cached Keychain answers (a no-op off macOS).
    fn forget(&self) {
        forget_cached_credentials();
    }

    async fn fetch_primary_or_account(&self, http: &reqwest::Client) -> ProviderQuota {
        let Some(first) = self.load() else {
            let mut q = empty_quota(CLAUDE_ID, DISPLAY_NAME, ProviderStatus::NotLoggedIn);
            q.error = Some(match &self.ctx.account {
                Some(a) => account_login_hint(&a.config_dir, cfg!(target_os = "macos")),
                None => "Not logged in — run `claude` to sign in".into(),
            });
            return q;
        };
        let now_ms = chrono::Utc::now().timestamp_millis();
        let reload = || {
            // A stale Keychain read must not keep an expired token alive
            // after the user has re-run `claude`.
            self.forget();
            self.load()
        };
        let Some(creds) = usable_credentials(first, now_ms, reload) else {
            log::info!("claude: access token expired");
            return degraded(
                &self.ctx,
                CLAUDE_ID,
                DISPLAY_NAME,
                ProviderStatus::TokenExpired,
                EXPIRED_MESSAGE,
            );
        };
        self.fetch_usage(http, creds).await
    }

    /// The usage request itself, for credentials already judged usable.
    async fn fetch_usage(&self, http: &reqwest::Client, creds: Credentials) -> ProviderQuota {
        let url = self.ctx.endpoint(USAGE_URL);
        let resp = super::send_with_retry(&self.ctx, || {
            http.get(&url)
                .bearer_auth(&creds.access_token)
                .header("anthropic-beta", OAUTH_BETA)
                .header("User-Agent", self.ctx.user_agent.clone())
                .header("Accept", "application/json")
        })
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
            self.forget();
            return degraded(
                &self.ctx,
                CLAUDE_ID,
                DISPLAY_NAME,
                ProviderStatus::TokenExpired,
                EXPIRED_MESSAGE,
            );
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            // The usage endpoint itself is rate-limited — and the OAuth token
            // is shared with Claude Code's own calls, so this is not an error
            // of ours. Keep the last good windows and obey `Retry-After`.
            let retry_after = retry_after_of(resp.headers());
            log::warn!(
                "claude usage API answered HTTP 429 (Retry-After: {})",
                retry_after.map_or("absent".to_string(), |s| format!("{s}s"))
            );
            return rate_limited(
                &self.ctx,
                CLAUDE_ID,
                DISPLAY_NAME,
                retry_after,
                "Anthropic is rate-limiting the usage endpoint (HTTP 429); showing the last known values until the next attempt",
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
        quota.extras = map_extras(&usage);
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
    fn profile_cache_is_invalidated_after_account_credentials_change() {
        clear_profile_cache();
        store_profile("synthetic-account-a", CachedProfile::default());
        assert!(cached_profile("synthetic-account-a").is_some());
        assert!(cached_profile("synthetic-account-b").is_none());
        clear_profile_cache();
    }

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
    fn credential_source_names_the_real_store_on_every_os() {
        let Some(source) = credential_source() else {
            return; // machine without a resolvable home directory
        };
        let keychain = format!("Keychain: {KEYCHAIN_SERVICE}");
        assert!(
            source.ends_with(".credentials.json") || source == keychain,
            "unexpected credential source `{source}`"
        );
        // Only macOS, and only while no credentials file shadows the item.
        assert_eq!(
            source == keychain,
            cfg!(target_os = "macos") && !credentials_path().is_some_and(|p| p.is_file()),
        );
        // Clearing the (possibly absent) cache must never panic or block.
        forget_cached_credentials();
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

    #[test]
    fn extras_only_add_what_the_credits_line_leaves_out() {
        let with = |extra: Option<ExtraUsage>| {
            map_extras(&UsageResponse {
                extra_usage: extra,
                ..UsageResponse::default()
            })
        };
        assert!(with(None).is_empty());
        assert!(with(Some(ExtraUsage::default())).is_empty(), "disabled");
        // utilization alone is already the credits balance — no duplicate row
        assert!(with(Some(ExtraUsage {
            is_enabled: true,
            utilization: Some(25.0),
            ..ExtraUsage::default()
        }))
        .is_empty());

        let extras = with(Some(ExtraUsage {
            is_enabled: true,
            monthly_limit: Some(50.0),
            used_credits: Some(12.5),
            utilization: Some(25.0),
        }));
        assert_eq!(extras.len(), 1);
        assert_eq!(extras[0].kind, "extra_usage");
        assert_eq!(extras[0].value.as_deref(), Some("25%"));
        assert_eq!(extras[0].severity, ExtraSeverity::Info);

        // the fixture's extra_usage is disabled, so a real response is clean
        let usage: UsageResponse = serde_json::from_str(FIXTURE).unwrap();
        assert!(map_extras(&usage).is_empty());
    }

    // ---------- expiry decision ----------

    fn creds(expires_at_ms: Option<i64>) -> Credentials {
        Credentials {
            access_token: "tok".into(),
            expires_at_ms,
            ..Credentials::default()
        }
    }

    #[test]
    fn a_token_is_refreshed_a_minute_before_it_expires() {
        let now = 1_000_000;
        assert!(!creds(Some(now + EXPIRY_SKEW_MS + 1)).needs_refresh(now));
        assert!(creds(Some(now + EXPIRY_SKEW_MS)).needs_refresh(now));
        assert!(creds(Some(now - 1)).needs_refresh(now));
        assert!(!creds(None).needs_refresh(now), "no expiry means valid");
        // `is_expired` itself stays exact (it drives the "logged in" flag).
        assert!(!creds(Some(now + 1)).is_expired(now));
    }

    #[test]
    fn an_expired_token_gets_one_reload_before_the_login_is_declared_expired() {
        let now = 1_000_000;
        // Fresh credentials are used as they are; no reload happens.
        let used = usable_credentials(creds(Some(now + 3_600_000)), now, || {
            panic!("must not reload a valid token")
        });
        assert!(used.is_some());
        // Expired on first read, refreshed by Claude Code in the meantime.
        let mut fresh = creds(Some(now + 3_600_000));
        fresh.access_token = "new".into();
        let used = usable_credentials(creds(Some(now - 5)), now, || Some(fresh)).unwrap();
        assert_eq!(used.access_token, "new");
        // Still expired after the reload, or unreadable: the login is expired.
        assert!(
            usable_credentials(creds(Some(now - 5)), now, || Some(creds(Some(now - 5)))).is_none()
        );
        assert!(usable_credentials(creds(Some(now - 5)), now, || None).is_none());
    }

    // ---------- fetch against a local stub ----------

    use crate::commands::test_support::StubServer;

    fn stub_provider(server: &StubServer) -> ClaudeProvider {
        ClaudeProvider::new(ProviderCtx {
            api_base: Some(server.base.clone()),
            retry_delay: Duration::from_millis(5),
            ..ProviderCtx::default()
        })
    }

    async fn fetch_stub(server: &StubServer) -> ProviderQuota {
        let http = reqwest::Client::new();
        stub_provider(server).fetch_usage(&http, creds(None)).await
    }

    #[tokio::test]
    async fn a_200_answer_becomes_windows() {
        let server = StubServer::start(vec![("200 OK", vec![], FIXTURE.to_string())]);
        let q = fetch_stub(&server).await;
        assert_eq!(q.status, ProviderStatus::Ok);
        assert!(!q.windows.is_empty());
        assert_eq!(q.source, DataSource::Api);
    }

    #[tokio::test]
    async fn a_401_is_final_and_reported_as_an_expired_login() {
        let server = StubServer::start(vec![("401 Unauthorized", vec![], "{}".into())]);
        let q = fetch_stub(&server).await;
        assert_eq!(q.status, ProviderStatus::TokenExpired);
        assert_eq!(server.hits(), 1, "401 is never retried");
    }

    #[tokio::test]
    async fn a_429_keeps_retry_after_and_is_never_retried() {
        let server = StubServer::start(vec![(
            "429 Too Many Requests",
            vec![("Retry-After", "120")],
            "{}".into(),
        )]);
        let q = fetch_stub(&server).await;
        assert_eq!(q.status, ProviderStatus::RateLimited);
        assert!(q.next_attempt_at.is_some());
        assert_eq!(server.hits(), 1);
    }

    #[tokio::test]
    async fn a_503_is_retried_once_and_then_reported() {
        let server = StubServer::start(vec![("503 Service Unavailable", vec![], "{}".into())]);
        let q = fetch_stub(&server).await;
        assert_eq!(q.status, ProviderStatus::Error);
        assert_eq!(server.hits(), 2, "exactly one retry");
    }

    #[tokio::test]
    async fn a_transient_502_followed_by_a_200_succeeds() {
        let server = StubServer::start(vec![
            ("502 Bad Gateway", vec![], "{}".into()),
            ("200 OK", vec![], FIXTURE.to_string()),
        ]);
        let q = fetch_stub(&server).await;
        assert_eq!(q.status, ProviderStatus::Ok);
    }

    #[test]
    fn the_profile_cache_keeps_one_entry_per_token() {
        clear_profile_cache();
        store_profile("synthetic-token-a", CachedProfile::default());
        store_profile("synthetic-token-b", CachedProfile::default());
        assert!(cached_profile("synthetic-token-a").is_some());
        assert!(cached_profile("synthetic-token-b").is_some());
        clear_profile_cache();
    }

    #[test]
    fn an_extra_account_reads_its_own_file_and_explains_a_missing_login() {
        let dir = crate::commands::test_support::tempdir();
        assert!(load_credentials_from(&dir).is_none());
        std::fs::write(
            credentials_path_in(&dir),
            r#"{"claudeAiOauth":{"accessToken":"synthetic","subscriptionType":"pro"}}"#,
        )
        .unwrap();
        let c = load_credentials_from(&dir).unwrap();
        assert_eq!(c.subscription_type.as_deref(), Some("pro"));

        let hint = account_login_hint(&dir, false);
        assert!(hint.contains("CLAUDE_CONFIG_DIR=") && hint.contains("/login"));
        std::fs::remove_file(credentials_path_in(&dir)).unwrap();
        let mac = account_login_hint(&dir, true);
        assert!(mac.contains("Keychain item `Claude Code-credentials-") && mac.contains("/login"));
        // with the file present macOS gives the normal sign-in hint
        std::fs::write(credentials_path_in(&dir), "{}").unwrap();
        assert!(!account_login_hint(&dir, true).contains("Keychain"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sha256_matches_the_published_test_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // two blocks (the 56-byte NIST vector pads into a second block)
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn an_extra_account_keychain_service_is_derived_from_its_own_directory() {
        let a = keychain_service_for(Path::new("/home/u/.claude-work"));
        let b = keychain_service_for(Path::new("/home/u/.claude-home"));
        assert_ne!(a, b, "every config dir has its own item");
        assert_ne!(a, KEYCHAIN_SERVICE, "never the primary account's item");
        assert!(a.starts_with("Claude Code-credentials-"));
        assert_eq!(a.len(), KEYCHAIN_SERVICE.len() + 1 + 8);
        assert!(a[KEYCHAIN_SERVICE.len() + 1..]
            .bytes()
            .all(|c| c.is_ascii_hexdigit()));
        // a pasted trailing slash is tried as typed and without
        let tries = keychain_services_for(Path::new("/home/u/.claude-work/"));
        assert_eq!(tries.len(), 2);
        assert_eq!(tries[1], a);
        assert_eq!(
            keychain_services_for(Path::new("/home/u/.claude-work")).len(),
            1
        );
    }

    #[tokio::test]
    async fn an_extra_account_is_tagged_and_caches_separately() {
        let server = StubServer::start(vec![("200 OK", vec![], FIXTURE.to_string())]);
        let data = crate::commands::test_support::tempdir();
        let home = crate::commands::test_support::tempdir();
        std::fs::write(
            credentials_path_in(&home),
            r#"{"claudeAiOauth":{"accessToken":"synthetic-work-token","expiresAt":4102444800000}}"#,
        )
        .unwrap();
        let ctx = ProviderCtx {
            api_base: Some(server.base.clone()),
            retry_delay: Duration::from_millis(5),
            ..ProviderCtx::with_data_dir(&data)
        };
        let provider = ClaudeProvider::new(ctx.with_account(super::super::AccountRef {
            id: "work".into(),
            label: "Work".into(),
            config_dir: home.clone(),
        }));
        let q = provider.fetch(&reqwest::Client::new()).await;
        assert_eq!(q.status, ProviderStatus::Ok);
        assert_eq!(q.provider, "claude");
        assert_eq!(q.account_id.as_deref(), Some("work"));
        assert_eq!(q.account_label.as_deref(), Some("Work"));
        assert_eq!(q.display_name, "Claude Code · Work");
        assert_eq!(q.key(), "claude@work");
        assert!(data.join("cache").join("quota-claude@work.json").is_file());
        assert!(!data.join("cache").join("quota-claude.json").exists());

        // an account without a login is its own status, with its own hint
        let empty = crate::commands::test_support::tempdir();
        let missing = ClaudeProvider::new(ctx.with_account(super::super::AccountRef {
            id: "side".into(),
            label: "Side".into(),
            config_dir: empty.clone(),
        }));
        let q = missing.fetch(&reqwest::Client::new()).await;
        assert_eq!(q.status, ProviderStatus::NotLoggedIn);
        assert_eq!(q.account_id.as_deref(), Some("side"));
        let hint = q.error.unwrap();
        assert!(hint.contains("CLAUDE_CONFIG_DIR=") || hint.contains("macOS"));
        for d in [data, home, empty] {
            std::fs::remove_dir_all(d).ok();
        }
    }
}
