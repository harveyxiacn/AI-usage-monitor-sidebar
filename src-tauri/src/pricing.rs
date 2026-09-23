//! Default price list + cost estimation. [BACKEND owns this file]
//!
//! Prices are USD per 1M tokens (input / output / cache write / cache read) and
//! mirror the public API prices. Subscription users do not actually pay per
//! token — the estimate is a *comparison indicator* only (ARCHITECTURE §9).

use crate::model::{events, PriceUpdateStatus, PricingEntry, PricingTable, TokenTotals};
use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

pub const PRICING_FILE: &str = "pricing.json";
/// The official, versioned source used unless the user configured another
/// HTTPS source in Settings. This table can be revised without an app release.
pub const DEFAULT_PRICING_URL: &str =
    "https://raw.githubusercontent.com/harveyxiacn/AI-usage-monitor-sidebar/main/pricing.json";
/// Revision of [`DEFAULTS`]. Keep this in sync with the shipped `pricing.json`.
pub const BUILTIN_PRICING_UPDATED_AT: &str = "2026-09-23T00:00:00Z";

/// `(model id, input, output, cache write, cache read)` — USD / 1M tokens.
/// Sources checked 2026-09-23:
/// https://platform.claude.com/docs/en/about-claude/pricing
/// https://claude.com/pricing
/// https://www.anthropic.com/claude-opus-5-5
/// https://developers.openai.com/api/docs/pricing
/// Standard short-context rates; excludes fast-mode, long-context and cache
/// TTL adjustments. Where no cache-write price is published, use input rate.
const DEFAULTS: &[(&str, f64, f64, f64, f64)] = &[
    // --- Anthropic ---
    ("claude-opus-4", 15.0, 75.0, 18.75, 1.5),
    ("claude-opus-4-1", 15.0, 75.0, 18.75, 1.5),
    ("claude-opus-4-5", 5.0, 25.0, 6.25, 0.5),
    ("claude-opus-4-6", 5.0, 25.0, 6.25, 0.5),
    ("claude-opus-4-7", 5.0, 25.0, 6.25, 0.5),
    ("claude-opus-4-8", 5.0, 25.0, 6.25, 0.5),
    ("claude-opus-5", 5.0, 25.0, 6.25, 0.5),
    ("claude-opus-5-5", 4.0, 20.0, 5.0, 0.2),
    ("claude-sonnet-4", 3.0, 15.0, 3.75, 0.3),
    ("claude-sonnet-4-5", 3.0, 15.0, 3.75, 0.3),
    ("claude-sonnet-4-6", 3.0, 15.0, 3.75, 0.3),
    ("claude-sonnet-5", 2.0, 10.0, 2.5, 0.2),
    ("claude-haiku-4-5", 1.0, 5.0, 1.25, 0.1),
    ("claude-fable-5", 10.0, 50.0, 12.5, 1.0),
    ("claude-mythos-5", 10.0, 50.0, 12.5, 1.0),
    ("claude-fable-5-1", 10.0, 50.0, 12.5, 0.25),
    ("claude-mythos-5-1", 10.0, 50.0, 12.5, 0.25),
    // --- OpenAI ---
    ("gpt-5", 1.25, 10.0, 1.25, 0.125),
    ("gpt-5-codex", 1.25, 10.0, 1.25, 0.125),
    ("gpt-5.1", 1.25, 10.0, 1.25, 0.125),
    ("gpt-5.1-codex", 1.25, 10.0, 1.25, 0.125),
    // https://developers.openai.com/api/docs/models/gpt-5.1-codex-max
    ("gpt-5.1-codex-max", 1.25, 10.0, 1.25, 0.125),
    // https://developers.openai.com/api/docs/models/gpt-5.1-codex-mini
    ("gpt-5.1-codex-mini", 0.25, 2.0, 0.25, 0.025),
    ("gpt-5.2", 1.75, 14.0, 1.75, 0.175),
    // https://developers.openai.com/api/docs/models/gpt-5.2-codex
    ("gpt-5.2-codex", 1.75, 14.0, 1.75, 0.175),
    ("gpt-5.3-codex", 1.75, 14.0, 1.75, 0.175),
    ("gpt-5.4", 2.5, 15.0, 2.5, 0.25),
    ("gpt-5.5", 5.0, 30.0, 5.0, 0.5),
    ("gpt-5.6-sol", 4.0, 20.0, 5.0, 0.4),
    ("gpt-5.6-terra", 2.0, 12.0, 2.5, 0.2),
    ("gpt-5.6-luna", 0.2, 1.2, 0.25, 0.02),
    ("gpt-6-sol", 2.0, 10.0, 2.5, 0.2),
    ("gpt-6-luna", 0.1, 0.5, 0.125, 0.01),
    // Keep Astra last so unknown GPT-6 variants retain the prior family price.
    ("gpt-6-astra", 10.0, 50.0, 12.5, 1.0),
    ("gpt-5-mini", 0.25, 2.0, 0.25, 0.025),
    ("gpt-5-nano", 0.05, 0.4, 0.05, 0.005),
    ("codex-mini", 1.5, 6.0, 1.5, 0.375),
    // https://developers.openai.com/api/docs/models/codex-mini-latest
    ("codex-mini-latest", 1.5, 6.0, 1.5, 0.375),
];

/// The built-in table (no user overrides applied).
pub fn default_table() -> PricingTable {
    PricingTable {
        entries: DEFAULTS
            .iter()
            .map(|(p, i, o, cw, cr)| PricingEntry {
                model_pattern: (*p).to_string(),
                input_per_m: *i,
                output_per_m: *o,
                cache_write_per_m: *cw,
                cache_read_per_m: *cr,
            })
            .collect(),
        updated_at: Some(BUILTIN_PRICING_UPDATED_AT.to_string()),
    }
}

pub fn pricing_path(config_dir: &Path) -> PathBuf {
    config_dir.join(PRICING_FILE)
}

/// Resolve the configured source once at the boundary. Empty settings are not
/// "off": they mean the project's published pricing table.
pub fn effective_pricing_url(configured: &str) -> String {
    let configured = configured.trim();
    if configured.is_empty() {
        DEFAULT_PRICING_URL.to_string()
    } else {
        configured.to_string()
    }
}

/// A readable complete table is a deliberate user override. A broken file is
/// ignored exactly as [`load_with_base`] ignores it, so it cannot prevent a
/// user from recovering through the source table.
pub fn has_custom_pricing(config_dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(pricing_path(config_dir)) else {
        return false;
    };
    // Match `load_with_base`: once the schema parses, it is a complete local
    // selection, even if individual malformed entries are subsequently
    // filtered out. Treating it otherwise could let an update appear to apply
    // while this file still wins after restart.
    serde_json::from_str::<PricingTable>(&text).is_ok()
}

/// The saved table is the user's complete selection, including deletions.
/// Defaults are used only when no readable, valid pricing file exists.
pub fn load(config_dir: &Path) -> PricingTable {
    load_with_base(config_dir, default_table())
}

/// Like `load`, but with an explicit fallback table — bundled defaults or the
/// currently applied cache for the selected pricing source.
pub fn load_with_base(config_dir: &Path, base: PricingTable) -> PricingTable {
    let path = pricing_path(config_dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return base;
    };
    let overrides: PricingTable = match serde_json::from_str(&text) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("ignoring invalid {}: {}", path.display(), e);
            return base;
        }
    };
    let mut table = PricingTable {
        entries: Vec::new(),
        updated_at: overrides.updated_at,
    };
    for mut entry in overrides.entries {
        if !valid_entry(&entry) {
            log::warn!("ignoring invalid pricing entry");
            continue;
        }
        entry.model_pattern = entry.model_pattern.trim().to_ascii_lowercase();
        match table
            .entries
            .iter_mut()
            .find(|e| e.model_pattern.eq_ignore_ascii_case(&entry.model_pattern))
        {
            Some(existing) => *existing = entry,
            None => table.entries.push(entry),
        }
    }
    table
}

/// Persist the user's complete table atomically and return its normalized form.
pub fn save(config_dir: &Path, table: &PricingTable) -> Result<PricingTable> {
    anyhow::ensure!(
        table.entries.iter().all(valid_entry),
        "pricing entries require a model name and finite, non-negative rates"
    );
    let mut to_write = table.clone();
    to_write.updated_at = Some(crate::commands::providers::now_rfc3339());
    std::fs::create_dir_all(config_dir).ok();
    crate::commands::settings::write_atomic(
        &pricing_path(config_dir),
        &serde_json::to_vec_pretty(&to_write).context("serialize pricing table")?,
    )?;
    Ok(load(config_dir))
}

fn valid_entry(entry: &PricingEntry) -> bool {
    !entry.model_pattern.trim().is_empty()
        && [
            entry.input_per_m,
            entry.output_per_m,
            entry.cache_write_per_m,
            entry.cache_read_per_m,
        ]
        .iter()
        .all(|rate| rate.is_finite() && *rate >= 0.0)
}

// ---------- remote price list ----------
//
// Empty `Settings.pricingUrl` uses the project's published HTTPS source; a
// non-empty value selects a user-provided HTTPS source instead.

pub const REMOTE_CACHE_FILE: &str = "pricing-remote.json";
/// A price list is a few kilobytes; anything larger is not one.
const REMOTE_MAX_BYTES: usize = 256 * 1024;
const REMOTE_MAX_ENTRIES: usize = 2_000;
/// Model ids are short and boring; a longer or stranger one is a red flag.
const REMOTE_MAX_PATTERN_LEN: usize = 100;
/// USD per 1M tokens. Nothing real is anywhere near this.
const REMOTE_MAX_RATE: f64 = 10_000.0;
/// At most one fetch a day.
pub const REMOTE_MIN_INTERVAL_MS: i64 = 24 * 3_600 * 1_000;

/// The cached remote table, tagged with where and when it came from.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCache {
    pub url: String,
    pub fetched_at_ms: i64,
    pub table: PricingTable,
}

pub fn remote_cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join(REMOTE_CACHE_FILE)
}

/// `Ok` only for an absolute `https://` URL with a host. Plain HTTP and
/// anything exotic (`file:`, `data:`, …) is refused.
pub fn validate_url(url: &str) -> Result<reqwest::Url> {
    let trimmed = url.trim();
    anyhow::ensure!(!trimmed.is_empty(), "no pricing URL configured");
    let parsed = reqwest::Url::parse(trimmed).context("pricing URL is not a URL")?;
    anyhow::ensure!(parsed.scheme() == "https", "the pricing URL must be https");
    anyhow::ensure!(parsed.host().is_some(), "the pricing URL has no host");
    Ok(parsed)
}

/// Strictly validate a downloaded table: a remote file is untrusted input.
/// Returns the normalized table, or an error naming the first problem.
pub fn validate_remote(table: &PricingTable) -> Result<PricingTable> {
    anyhow::ensure!(!table.entries.is_empty(), "the pricing table is empty");
    anyhow::ensure!(
        table.entries.len() <= REMOTE_MAX_ENTRIES,
        "the pricing table has more than {REMOTE_MAX_ENTRIES} entries"
    );
    let mut out = PricingTable {
        entries: Vec::with_capacity(table.entries.len()),
        updated_at: table.updated_at.clone(),
    };
    for entry in &table.entries {
        let pattern = entry.model_pattern.trim().to_ascii_lowercase();
        anyhow::ensure!(!pattern.is_empty(), "a pricing entry has no model name");
        anyhow::ensure!(
            pattern.len() <= REMOTE_MAX_PATTERN_LEN,
            "model name `{pattern}` is unreasonably long"
        );
        anyhow::ensure!(
            pattern
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._:/+".contains(c)),
            "model name `{pattern}` contains unexpected characters"
        );
        for rate in [
            entry.input_per_m,
            entry.output_per_m,
            entry.cache_write_per_m,
            entry.cache_read_per_m,
        ] {
            anyhow::ensure!(
                rate.is_finite() && (0.0..=REMOTE_MAX_RATE).contains(&rate),
                "model `{pattern}` has an implausible rate ({rate})"
            );
        }
        anyhow::ensure!(
            !out.entries.iter().any(|e| e.model_pattern == pattern),
            "model `{pattern}` appears twice"
        );
        out.entries.push(PricingEntry {
            model_pattern: pattern,
            ..entry.clone()
        });
    }
    Ok(out)
}

fn parsed_updated_at(table: &PricingTable) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    table
        .updated_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
}

fn table_is_older(remote: &PricingTable, current: &PricingTable) -> bool {
    matches!(
        (parsed_updated_at(remote), parsed_updated_at(current)),
        (Some(remote_at), Some(current_at)) if remote_at < current_at
    )
}

/// Whether a legacy cache refresh should be downloaded now (pure). The new
/// price-update flow keeps checked candidates in memory and only writes this
/// cache after an explicit apply.
pub fn should_fetch_remote(
    url: &str,
    cache: Option<&RemoteCache>,
    now_ms: i64,
    manual: bool,
) -> bool {
    let url = effective_pricing_url(url);
    if manual {
        return true;
    }
    match cache {
        None => true,
        Some(c) if c.url != url => true,
        Some(c) => now_ms.saturating_sub(c.fetched_at_ms) >= REMOTE_MIN_INTERVAL_MS,
    }
}

/// The cached remote table, but only if it was fetched from `url`.
pub fn cached_remote(data_dir: &Path, url: &str) -> Option<RemoteCache> {
    let text = std::fs::read_to_string(remote_cache_path(data_dir)).ok()?;
    let cache: RemoteCache = serde_json::from_str(&text).ok()?;
    (cache.url == effective_pricing_url(url)).then_some(cache)
}

/// The table the app starts from: the cached remote list when the user
/// enabled one, otherwise the bundled defaults. Any problem falls back.
pub fn base_table(data_dir: &Path, url: &str) -> PricingTable {
    let url = effective_pricing_url(url);
    match cached_remote(data_dir, &url) {
        Some(cache) => {
            match validate_remote(&cache.table) {
                Ok(table) if !table_is_older(&table, &default_table()) => table,
                Ok(_) => {
                    log::warn!("cached remote pricing table predates the bundled table, using built-in prices");
                    default_table()
                }
                Err(e) => {
                    log::warn!(
                        "cached remote pricing table is unusable ({e:#}), using the built-in one"
                    );
                    default_table()
                }
            }
        }
        None => default_table(),
    }
}

/// Fetch and validate an untrusted price table. This deliberately does not
/// touch disk: checking for an update must not make it active after restart.
pub async fn fetch_remote(http: &reqwest::Client, url: &str) -> Result<PricingTable> {
    let url = effective_pricing_url(url);
    let parsed = validate_url(&url)?;
    let response = http
        .get(parsed)
        .header("Accept", "application/json")
        .send()
        .await
        .context("could not reach the pricing URL")?;
    anyhow::ensure!(
        response.status().is_success(),
        "the pricing URL returned HTTP {}",
        response.status().as_u16()
    );
    if let Some(len) = response.content_length() {
        anyhow::ensure!(
            len as usize <= REMOTE_MAX_BYTES,
            "the pricing table is larger than {REMOTE_MAX_BYTES} bytes"
        );
    }
    let body = response.bytes().await.context("could not read the body")?;
    anyhow::ensure!(
        body.len() <= REMOTE_MAX_BYTES,
        "the pricing table is larger than {REMOTE_MAX_BYTES} bytes"
    );
    let parsed: PricingTable =
        serde_json::from_slice(&body).context("the pricing table is not valid JSON")?;
    validate_remote(&parsed)
}

/// Persist a previously validated table as the applied source table.
pub fn cache_remote(data_dir: &Path, url: &str, table: &PricingTable) -> Result<()> {
    let url = effective_pricing_url(url);
    let table = validate_remote(table)?;
    let cache = RemoteCache {
        url: url.clone(),
        fetched_at_ms: crate::commands::store::now_ms(),
        table,
    };
    crate::commands::settings::write_atomic(
        &remote_cache_path(data_dir),
        &serde_json::to_vec_pretty(&cache).context("serialize the pricing cache")?,
    )?;
    log::info!(
        "pricing table applied from {url} ({} models)",
        cache.table.entries.len()
    );
    Ok(())
}

/// Legacy cache-refresh helper. New code should call [`fetch_remote`] while
/// checking and [`cache_remote`] only after the user asks to apply.
pub async fn refresh_remote(
    http: &reqwest::Client,
    data_dir: &Path,
    url: &str,
    manual: bool,
) -> Result<Option<PricingTable>> {
    let url = effective_pricing_url(url);
    let existing = cached_remote(data_dir, &url);
    if !should_fetch_remote(
        &url,
        existing.as_ref(),
        crate::commands::store::now_ms(),
        manual,
    ) {
        return Ok(None);
    }
    let table = fetch_remote(http, &url).await?;
    cache_remote(data_dir, &url, &table)?;
    Ok(Some(table))
}

// ---------- independent price-update delivery ----------

/// First background price check stays out of the way during launch.
pub const PRICE_FIRST_CHECK_DELAY_SEC: u64 = 60;
/// A successful or unsuccessful automatic check is repeated once a day.
pub const PRICE_CHECK_INTERVAL_SEC: u64 = 24 * 60 * 60;

#[derive(Clone, Debug)]
struct PendingPricing {
    url: String,
    table: PricingTable,
}

/// Candidate tables are intentionally process-local. A check must never write
/// `pricing-remote.json`, because that file is the applied table loaded at the
/// next start.
pub struct PriceUpdateState {
    status: Mutex<PriceUpdateStatus>,
    pending: Mutex<Option<PendingPricing>>,
    source: Mutex<String>,
    operation: tokio::sync::Mutex<()>,
}

impl Default for PriceUpdateState {
    fn default() -> Self {
        Self {
            status: Mutex::new(PriceUpdateStatus::default()),
            pending: Mutex::new(None),
            source: Mutex::new(DEFAULT_PRICING_URL.to_string()),
            operation: tokio::sync::Mutex::new(()),
        }
    }
}

/// Set up the update state and its independent daily check loop.
pub fn setup(app: &AppHandle) {
    let source = source_of(app);
    let initial = PriceUpdateStatus {
        custom_pricing: has_custom_pricing(&app.state::<crate::state::AppState>().config_dir),
        ..PriceUpdateStatus::default()
    };
    app.manage(PriceUpdateState::default());
    if let Some(state) = app.try_state::<PriceUpdateState>() {
        *state.source.lock() = source;
    }
    store_status(app, initial);

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(PRICE_FIRST_CHECK_DELAY_SEC)).await;
        loop {
            if handle
                .state::<crate::state::AppState>()
                .settings
                .read()
                .auto_pricing_check
            {
                check(&handle).await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(PRICE_CHECK_INTERVAL_SEC)).await;
        }
    });
}

pub fn status(app: &AppHandle) -> PriceUpdateStatus {
    app.try_state::<PriceUpdateState>()
        .map(|state| state.status.lock().clone())
        .unwrap_or_default()
}

fn store_status(app: &AppHandle, status: PriceUpdateStatus) {
    store_status_for_settings(app, status, None);
}

fn store_status_for_settings(
    app: &AppHandle,
    status: PriceUpdateStatus,
    settings: Option<&crate::model::Settings>,
) {
    if let Some(state) = app.try_state::<PriceUpdateState>() {
        *state.status.lock() = status.clone();
    }
    match settings {
        Some(settings) => {
            crate::window::tray::sync_price_update_for_settings(app, &status, settings)
        }
        None => crate::window::tray::sync_price_update(app, &status),
    }
    if let Err(e) = app.emit(events::PRICE_UPDATE_STATUS, &status) {
        log::warn!("could not emit {}: {e}", events::PRICE_UPDATE_STATUS);
    }
}

fn update_price_status(
    app: &AppHandle,
    f: impl FnOnce(&mut PriceUpdateStatus),
) -> PriceUpdateStatus {
    let mut next = status(app);
    f(&mut next);
    store_status(app, next.clone());
    next
}

fn update_price_status_for_settings(
    app: &AppHandle,
    settings: &crate::model::Settings,
    f: impl FnOnce(&mut PriceUpdateStatus),
) -> PriceUpdateStatus {
    let mut next = status(app);
    f(&mut next);
    store_status_for_settings(app, next.clone(), Some(settings));
    next
}

/// Keep the public status correct after a user saves a complete local table.
pub fn sync_custom_pricing(app: &AppHandle) {
    let custom = has_custom_pricing(&app.state::<crate::state::AppState>().config_dir);
    if custom {
        if let Some(state) = app.try_state::<PriceUpdateState>() {
            *state.pending.lock() = None;
        }
    }
    update_price_status(app, |s| {
        s.custom_pricing = custom;
        if custom {
            s.available = false;
        }
    });
}

/// Persist a complete local table under the same lock used by checking and
/// source switching. This prevents a second Settings window from losing an
/// edit while `use_source_pricing` has the original file temporarily backed up.
pub async fn save_custom(app: &AppHandle, table: PricingTable) -> Result<PricingTable, String> {
    let update = app
        .try_state::<PriceUpdateState>()
        .ok_or_else(|| "price update service is not ready".to_string())?;
    let _operation = update.operation.lock().await;
    let state = app.state::<crate::state::AppState>();
    let merged = save(&state.config_dir, &table).map_err(|e| format!("{e:#}"))?;
    *state.pricing.write() = merged.clone();
    *update.pending.lock() = None;
    update_price_status(app, |s| {
        s.available = false;
        s.custom_pricing = true;
        s.error = None;
    });
    Ok(merged)
}

/// A settings-source change must immediately stop showing an offer from the
/// old URL. With no local table, also restore the new source's applied cache
/// (or bundled defaults) in memory rather than carrying old-source prices.
pub fn settings_changed(app: &AppHandle, settings: &crate::model::Settings) {
    let Some(update) = app.try_state::<PriceUpdateState>() else {
        return;
    };
    let source = effective_pricing_url(&settings.pricing_url);
    if *update.source.lock() == source {
        return;
    }
    *update.source.lock() = source.clone();
    *update.pending.lock() = None;
    // `use_source` may have temporarily renamed a custom table while it
    // applies a downloaded source. Do not inspect that transient filesystem
    // state or replace in-memory prices while the operation owns it. The
    // operation reconciles after it either commits or restores the file.
    // When it is idle, doing the reconciliation synchronously keeps a normal
    // settings change immediate.
    if let Ok(_operation) = update.operation.try_lock() {
        reconcile_selected_source(app, &source, settings);
        return;
    }
    update_price_status_for_settings(app, settings, |s| {
        s.available = false;
        s.revision = None;
        s.checked_at = None;
        s.error = None;
    });

    // The settings write lock is still held by its caller here, so this must
    // run later: reconciling reads the current Settings for the tray update.
    // It waits for the same operation lock as `use_source` and `apply`.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(update) = handle.try_state::<PriceUpdateState>() else {
            return;
        };
        let _operation = update.operation.lock().await;
        let settings = handle
            .state::<crate::state::AppState>()
            .settings
            .read()
            .clone();
        reconcile_selected_source(&handle, &source, &settings);
    });
}

/// Read the complete pricing selection that should be active for `source`.
/// A readable local table wins; otherwise the source's applied cache (or the
/// built-in prices) is the base. Keeping this in one place is what lets a
/// failed source switch restore the file and the in-memory table together.
fn selected_pricing(config_dir: &Path, data_dir: &Path, source: &str) -> (PricingTable, bool) {
    let custom = has_custom_pricing(config_dir);
    let table = load_with_base(config_dir, base_table(data_dir, source));
    (table, custom)
}

/// Reconcile the active file/cache choice into memory. Callers that change a
/// source hold `PriceUpdateState::operation`; this function keeps `source`
/// locked until the matching table and status have been stored, so a second
/// settings change cannot leave an older source's table in memory.
fn reconcile_selected_source(app: &AppHandle, source: &str, settings: &crate::model::Settings) {
    let Some(update) = app.try_state::<PriceUpdateState>() else {
        return;
    };
    let selected_source = update.source.lock();
    if selected_source.as_str() != source {
        return;
    }
    let state = app.state::<crate::state::AppState>();
    let (table, custom) = selected_pricing(&state.config_dir, &state.data_dir, source);
    *state.pricing.write() = table;
    update_price_status_for_settings(app, settings, |s| {
        s.available = false;
        s.revision = None;
        s.checked_at = None;
        s.error = None;
        s.custom_pricing = custom;
    });
}

/// Reconcile after an operation restores a local table or notices that the
/// selected source changed. These callers are outside Settings' write lock.
fn reconcile_active_source(app: &AppHandle) {
    let settings = app
        .state::<crate::state::AppState>()
        .settings
        .read()
        .clone();
    let source = effective_pricing_url(&settings.pricing_url);
    reconcile_selected_source(app, &source, &settings);
}

/// Entry order affects family-price fallback precedence, so a revision is
/// available only when the ordered entries differ. Valid timestamps stop an
/// older remote list from replacing a newer active one.
fn table_is_update(remote: &PricingTable, current: &PricingTable) -> bool {
    if remote.entries == current.entries {
        return false;
    }
    !table_is_older(remote, current)
}

fn source_of(app: &AppHandle) -> String {
    effective_pricing_url(
        &app.state::<crate::state::AppState>()
            .settings
            .read()
            .pricing_url,
    )
}

/// Download and compare one source revision. The candidate lives only in
/// [`PriceUpdateState`] until the user explicitly applies it.
pub async fn check(app: &AppHandle) -> PriceUpdateStatus {
    let Some(update) = app.try_state::<PriceUpdateState>() else {
        return PriceUpdateStatus::default();
    };
    {
        let mut status = update.status.lock();
        if status.checking || status.applying {
            return status.clone();
        }
        status.checking = true;
        status.error = None;
        let next = status.clone();
        drop(status);
        store_status(app, next);
    }
    let _operation = update.operation.lock().await;
    let source = source_of(app);
    let http = app.state::<crate::state::AppState>().http.clone();
    let fetched = fetch_remote(&http, &source).await;
    let now = chrono::Utc::now().to_rfc3339();

    // Settings can change while the HTTP request is in flight. The stale
    // response may not become the pending candidate or change availability.
    if source != source_of(app) {
        log::debug!("discarding pricing check from stale source {source}");
        return update_price_status(app, |s| {
            s.checking = false;
            s.available = false;
            s.revision = None;
            s.error = None;
        });
    }

    match fetched {
        Ok(table) => {
            // A complete user table is a separate override. Compare source
            // revisions against the currently applied source base, never the
            // override's local `updatedAt`.
            let state = app.state::<crate::state::AppState>();
            let current = base_table(&state.data_dir, &source);
            let available = table_is_update(&table, &current);
            *update.pending.lock() = available.then_some(PendingPricing {
                url: source,
                table: table.clone(),
            });
            update_price_status(app, |s| {
                s.checking = false;
                s.applying = false;
                s.available = available;
                s.revision = table.updated_at.clone();
                s.checked_at = Some(now);
                s.error = None;
                s.custom_pricing =
                    has_custom_pricing(&app.state::<crate::state::AppState>().config_dir);
            })
        }
        Err(e) => {
            let message = format!("{e:#}");
            log::warn!("pricing update check failed: {message}");
            update_price_status(app, |s| {
                s.checking = false;
                s.checked_at = Some(now);
                s.error = Some(message);
                s.custom_pricing =
                    has_custom_pricing(&app.state::<crate::state::AppState>().config_dir);
            })
        }
    }
}

fn complete_apply(app: &AppHandle, table: PricingTable) -> PricingTable {
    let state = app.state::<crate::state::AppState>();
    let effective = load_with_base(&state.config_dir, table);
    *state.pricing.write() = effective.clone();
    effective
}

/// Apply the last checked candidate. This command refuses a saved complete
/// user table, so no local edits are ever silently overwritten.
pub async fn apply(app: &AppHandle) -> Result<PricingTable, String> {
    let update = app
        .try_state::<PriceUpdateState>()
        .ok_or_else(|| "price update service is not ready".to_string())?;
    {
        let mut status = update.status.lock();
        if status.checking || status.applying {
            return Err("a price update operation is already running".into());
        }
        if has_custom_pricing(&app.state::<crate::state::AppState>().config_dir) {
            status.custom_pricing = true;
            let next = status.clone();
            drop(status);
            store_status(app, next);
            return Err("a custom pricing table is active; use_source_pricing first".into());
        }
        status.applying = true;
        status.error = None;
        let next = status.clone();
        drop(status);
        store_status(app, next);
    }
    let _operation = update.operation.lock().await;
    let pending = update.pending.lock().clone();
    let result = (|| -> Result<PricingTable, String> {
        let pending =
            pending.ok_or_else(|| "no checked pricing update is available".to_string())?;
        if pending.url != source_of(app) {
            return Err("the pricing source changed; check the new source before applying".into());
        }
        if has_custom_pricing(&app.state::<crate::state::AppState>().config_dir) {
            return Err("a custom pricing table is active; use_source_pricing first".into());
        }
        let state = app.state::<crate::state::AppState>();
        cache_remote(&state.data_dir, &pending.url, &pending.table)
            .map_err(|e| format!("{e:#}"))?;
        if pending.url != source_of(app) || has_custom_pricing(&state.config_dir) {
            return Err(
                "the pricing source changed while applying; the active table was kept".into(),
            );
        }
        // Settings changes serialize on `source`. Hold it through the memory
        // commit so a new source cannot be set between this final check and
        // `complete_apply`.
        let selected_source = update.source.lock();
        if selected_source.as_str() != pending.url {
            return Err(
                "the pricing source changed while applying; the active table was kept".into(),
            );
        }
        Ok(complete_apply(app, pending.table))
    })();
    if result.is_ok() {
        *update.pending.lock() = None;
    }
    let error = result.as_ref().err().cloned();
    update_price_status(app, |s| {
        s.applying = false;
        s.available = false;
        s.error = error;
        s.custom_pricing = has_custom_pricing(&app.state::<crate::state::AppState>().config_dir);
    });
    result
}

fn backup_custom_pricing(config_dir: &Path) -> Result<Option<PathBuf>> {
    if !has_custom_pricing(config_dir) {
        return Ok(None);
    }
    let source = pricing_path(config_dir);
    let timestamp = crate::commands::store::now_ms();
    let mut attempt = 0_u32;
    loop {
        let backup = config_dir.join(format!("pricing.custom.{timestamp}.{attempt}.json.bak"));
        if backup.exists() {
            attempt = attempt.saturating_add(1);
            continue;
        }
        std::fs::rename(&source, &backup)
            .with_context(|| format!("back up {}", source.display()))?;
        return Ok(Some(backup));
    }
}

fn restore_custom_pricing(backup: Option<&Path>, config_dir: &Path) {
    if let Some(backup) = backup {
        if let Err(e) = std::fs::rename(backup, pricing_path(config_dir)) {
            log::error!(
                "could not restore custom pricing table {}: {e}",
                backup.display()
            );
        }
    }
}

/// Explicitly discard the active local table in favour of the configured
/// source. The original file is retained beside it as a timestamped backup.
pub async fn use_source(app: &AppHandle) -> Result<PricingTable, String> {
    let update = app
        .try_state::<PriceUpdateState>()
        .ok_or_else(|| "price update service is not ready".to_string())?;
    {
        let mut status = update.status.lock();
        if status.checking || status.applying {
            return Err("a price update operation is already running".into());
        }
        status.applying = true;
        status.error = None;
        let next = status.clone();
        drop(status);
        store_status(app, next);
    }
    let _operation = update.operation.lock().await;
    let source = source_of(app);
    let http = app.state::<crate::state::AppState>().http.clone();
    let result = async {
        let table = fetch_remote(&http, &source)
            .await
            .map_err(|e| format!("{e:#}"))?;
        if source != source_of(app) {
            return Err("the pricing source changed; try again".into());
        }
        let state = app.state::<crate::state::AppState>();
        if table_is_older(&table, &base_table(&state.data_dir, &source)) {
            return Err("the source pricing revision predates the active price table".into());
        }
        let backup = backup_custom_pricing(&state.config_dir).map_err(|e| format!("{e:#}"))?;
        if let Err(e) = cache_remote(&state.data_dir, &source, &table) {
            restore_custom_pricing(backup.as_deref(), &state.config_dir);
            reconcile_active_source(app);
            return Err(format!("{e:#}"));
        }
        if source != source_of(app) {
            restore_custom_pricing(backup.as_deref(), &state.config_dir);
            reconcile_active_source(app);
            return Err(
                "the pricing source changed while applying; the custom table was restored".into(),
            );
        }
        // Keep the final source check and in-memory commit together. A
        // concurrent settings change then waits here and performs its own
        // reconciliation once this operation releases its lock.
        let selected_source = update.source.lock();
        if selected_source.as_str() != source {
            drop(selected_source);
            restore_custom_pricing(backup.as_deref(), &state.config_dir);
            reconcile_active_source(app);
            return Err(
                "the pricing source changed while applying; the custom table was restored".into(),
            );
        }
        Ok(complete_apply(app, table))
    }
    .await;
    if result.is_ok() {
        *update.pending.lock() = None;
    }
    let error = result.as_ref().err().cloned();
    update_price_status(app, |s| {
        s.applying = false;
        s.available = false;
        s.error = error;
        s.custom_pricing = has_custom_pricing(&app.state::<crate::state::AppState>().config_dir);
    });
    result
}

/// Compatibility path for older frontends: their explicit "refresh prices"
/// action still checks and applies in one user-initiated operation.
pub async fn refresh_legacy(app: &AppHandle) -> Result<PricingTable, String> {
    let checked = check(app).await;
    if let Some(error) = checked.error {
        return Err(error);
    }
    if checked.custom_pricing {
        return Err("a custom pricing table is active; use_source_pricing first".into());
    }
    if checked.available {
        apply(app).await
    } else {
        Ok(app.state::<crate::state::AppState>().pricing.read().clone())
    }
}

/// Lowercase the model and drop a trailing date suffix (`-20250514`).
pub fn normalize_model(model: &str) -> String {
    let lower = model.trim().to_ascii_lowercase();
    if let Some(idx) = lower.rfind('-') {
        let suffix = &lower[idx + 1..];
        if (6..=8).contains(&suffix.len()) && suffix.bytes().all(|b| b.is_ascii_digit()) {
            return lower[..idx].to_string();
        }
    }
    lower
}

/// How a model id was priced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchKind {
    /// The table prices this exact model (or a user prefix covering it).
    Exact,
    /// Nearest known family — approximate, and reported as such.
    Family,
}

/// Leading components two ids must share to count as the same family.
const FAMILY_MIN_SHARED: usize = 2;

/// Split a normalized model id into comparable components. `-` and `.` are
/// both separators, so `gpt-5.3-codex` is `["gpt", "5", "3", "codex"]` and
/// shares two components with `gpt-5.1`.
fn components(name: &str) -> Vec<&str> {
    name.split(['-', '.']).filter(|c| !c.is_empty()).collect()
}

/// The closest known family for a model the table does not price exactly.
///
/// Model names drift fast (`gpt-5.9`, `claude-opus-4-99`), and an unpriced
/// new model silently turns the whole estimate into "unknown". Among the
/// entries that share the longest leading run of components, the **most
/// generic** one wins (`gpt-5.9` → `gpt-5`, not `gpt-5.1-codex-max`), with
/// the later table entry as the tie-break. The result is an approximation:
/// callers must mark it as one.
pub fn find_family_entry<'a>(table: &'a PricingTable, model: &str) -> Option<&'a PricingEntry> {
    let name = normalize_model(model);
    let want = components(&name);
    if want.len() < FAMILY_MIN_SHARED {
        return None;
    }
    let mut best: Option<((usize, usize, usize), &PricingEntry)> = None;
    for (index, entry) in table.entries.iter().enumerate() {
        let pattern = normalize_model(&entry.model_pattern);
        let have = components(&pattern);
        let shared = want
            .iter()
            .zip(have.iter())
            .take_while(|(a, b)| a == b)
            .count();
        if shared < FAMILY_MIN_SHARED {
            continue;
        }
        // More shared components first, then the shortest (most generic)
        // pattern, then the entry that comes later in the table.
        let key = (shared, usize::MAX - have.len(), index);
        if best.is_none_or(|(current, _)| key > current) {
            best = Some((key, entry));
        }
    }
    best.map(|(_, entry)| entry)
}

/// Price `model`, falling back to the closest known family.
pub fn find_match<'a>(
    table: &'a PricingTable,
    model: &str,
) -> Option<(&'a PricingEntry, MatchKind)> {
    if let Some(entry) = find_entry(table, model) {
        return Some((entry, MatchKind::Exact));
    }
    find_family_entry(table, model).map(|entry| (entry, MatchKind::Family))
}

/// Built-in patterns require an exact normalized model name. Additional
/// user-defined patterns use longest-prefix matching.
pub fn find_entry<'a>(table: &'a PricingTable, model: &str) -> Option<&'a PricingEntry> {
    let name = normalize_model(model);
    table
        .entries
        .iter()
        .filter(|e| {
            let p = e.model_pattern.to_ascii_lowercase();
            !p.is_empty()
                && if DEFAULTS.iter().any(|entry| entry.0 == p) {
                    name == p
                } else {
                    name.starts_with(&p)
                }
        })
        .max_by_key(|e| e.model_pattern.len())
}

/// Estimated USD cost of `totals` for `model`, together with whether the
/// price came from the model itself or only from its family.
///
/// Reasoning tokens are already part of `output_tokens` for both providers and
/// are therefore not charged twice.
pub fn estimate_cost_kind(
    table: &PricingTable,
    model: &str,
    totals: &TokenTotals,
) -> Option<(f64, MatchKind)> {
    let (e, kind) = find_match(table, model)?;
    let m = 1_000_000.0;
    Some((
        totals.input_tokens as f64 * e.input_per_m / m
            + totals.output_tokens as f64 * e.output_per_m / m
            + totals.cache_write_tokens as f64 * e.cache_write_per_m / m
            + totals.cache_read_tokens as f64 * e.cache_read_per_m / m,
        kind,
    ))
}

/// Estimated USD cost of `totals` for `model`, or `None` when not even a
/// family matches.
pub fn estimate_cost(table: &PricingTable, model: &str, totals: &TokenTotals) -> Option<f64> {
    estimate_cost_kind(table, model, totals).map(|(cost, _)| cost)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::tempdir;

    fn totals(input: i64, output: i64, cache_write: i64, cache_read: i64) -> TokenTotals {
        TokenTotals {
            input_tokens: input,
            output_tokens: output,
            cache_write_tokens: cache_write,
            cache_read_tokens: cache_read,
            ..TokenTotals::default()
        }
    }

    #[test]
    fn longest_prefix_wins() {
        let t = default_table();
        assert_eq!(find_entry(&t, "gpt-5").unwrap().model_pattern, "gpt-5");
        assert_eq!(
            find_entry(&t, "gpt-5-codex").unwrap().model_pattern,
            "gpt-5-codex"
        );
        assert_eq!(
            find_entry(&t, "gpt-5-mini").unwrap().model_pattern,
            "gpt-5-mini"
        );
        assert_eq!(
            find_entry(&t, "gpt-5.1-codex").unwrap().model_pattern,
            "gpt-5.1-codex"
        );
        assert_eq!(
            find_entry(&t, "claude-opus-4-5").unwrap().model_pattern,
            "claude-opus-4-5"
        );
        assert_eq!(
            find_entry(&t, "claude-opus-4-1").unwrap().model_pattern,
            "claude-opus-4-1"
        );
        assert_eq!(
            find_entry(&t, "claude-sonnet-4").unwrap().model_pattern,
            "claude-sonnet-4"
        );
    }

    #[test]
    fn matching_ignores_case_and_date_suffixes() {
        let t = default_table();
        assert_eq!(
            normalize_model("claude-opus-4-1-20250805"),
            "claude-opus-4-1"
        );
        assert_eq!(
            normalize_model("Claude-Sonnet-4-5-20250929"),
            "claude-sonnet-4-5"
        );
        assert_eq!(normalize_model("gpt-6-astra"), "gpt-6-astra");
        assert_eq!(
            find_entry(&t, "claude-opus-4-1-20250805")
                .unwrap()
                .model_pattern,
            "claude-opus-4-1"
        );
        assert_eq!(find_entry(&t, "GPT-5.2").unwrap().model_pattern, "gpt-5.2");
    }

    #[test]
    fn unknown_models_have_no_exact_price() {
        let t = default_table();
        assert!(find_entry(&t, "llama-9").is_none());
        assert!(estimate_cost(&t, "llama-9", &totals(1000, 1000, 0, 0)).is_none());
        assert!(estimate_cost(&t, "", &totals(1, 1, 1, 1)).is_none());
        assert!(find_entry(&t, "gpt-5.9").is_none());
        assert!(find_entry(&t, "gpt-5.3-codex-spark").is_none());
        assert!(find_entry(&t, "claude-opus-4-99").is_none());
    }

    #[test]
    fn a_new_model_falls_back_to_its_closest_family() {
        let t = default_table();
        let family = |model: &str| {
            find_family_entry(&t, model)
                .map(|e| e.model_pattern.clone())
                .unwrap_or_default()
        };
        // The longest shared run of components wins…
        assert_eq!(family("gpt-5.3-codex-spark"), "gpt-5.3-codex");
        assert_eq!(family("claude-opus-4-99"), "claude-opus-4");
        assert_eq!(family("claude-sonnet-4-9-20270101"), "claude-sonnet-4");
        // …and among equals the most generic entry, not a random variant.
        assert_eq!(family("gpt-5.9"), "gpt-5");
        assert_eq!(family("gpt-6-nova"), "gpt-6-astra");
        // A model from another vendor stays unknown rather than borrowing a
        // price from something that merely starts with the same letters.
        assert_eq!(family("llama-9"), "");
        assert_eq!(family("mistral-large"), "");
        assert_eq!(family("gpt"), "", "one component is not a family");
        assert_eq!(family(""), "");
    }

    #[test]
    fn family_prices_are_reported_as_approximate() {
        let t = default_table();
        assert_eq!(
            find_match(&t, "gpt-5.3-codex").map(|(e, k)| (e.model_pattern.as_str(), k)),
            Some(("gpt-5.3-codex", MatchKind::Exact))
        );
        assert_eq!(
            find_match(&t, "gpt-5.3-codex-spark").map(|(e, k)| (e.model_pattern.as_str(), k)),
            Some(("gpt-5.3-codex", MatchKind::Family))
        );
        assert!(find_match(&t, "llama-9").is_none());

        // gpt-5.3-codex: 1.75 / 14 / 1.75 / 0.175 per 1M
        let (cost, kind) =
            estimate_cost_kind(&t, "gpt-5.3-codex-spark", &totals(1_000_000, 0, 0, 0)).unwrap();
        assert!((cost - 1.75).abs() < 1e-9);
        assert_eq!(kind, MatchKind::Family);
    }

    fn remote_table(pattern: &str, rate: f64) -> PricingTable {
        PricingTable {
            entries: vec![PricingEntry {
                model_pattern: pattern.into(),
                input_per_m: rate,
                output_per_m: rate,
                cache_write_per_m: rate,
                cache_read_per_m: rate,
            }],
            updated_at: None,
        }
    }

    #[test]
    fn the_shipped_pricing_json_matches_the_built_in_table() {
        // `pricing.json` at the repo root is what the project can host for
        // `pricingUrl`; it must stay in step with DEFAULTS and pass exactly
        // the validation a downloaded table gets.
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../pricing.json");
        let text = std::fs::read_to_string(&path).expect("pricing.json is shipped");
        let table: PricingTable = serde_json::from_str(&text).expect("valid schema");
        let validated = validate_remote(&table).expect("passes remote validation");
        assert_eq!(
            validated.entries,
            default_table().entries,
            "pricing.json drifted from pricing.rs DEFAULTS"
        );
        assert_eq!(
            validated.updated_at,
            default_table().updated_at,
            "pricing.json drifted from the built-in pricing revision"
        );
    }

    #[test]
    fn only_https_urls_are_accepted() {
        assert!(validate_url("https://raw.githubusercontent.com/a/b/main/pricing.json").is_ok());
        assert!(validate_url("  https://example.com/p.json  ").is_ok());
        for bad in [
            "",
            "   ",
            "http://example.com/p.json",
            "file:///etc/passwd",
            "data:application/json,{}",
            "example.com/p.json",
            "https://",
        ] {
            assert!(validate_url(bad).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn a_remote_table_is_validated_strictly() {
        assert_eq!(
            validate_remote(&remote_table("GPT-5.9 ", 2.0))
                .unwrap()
                .entries[0]
                .model_pattern,
            "gpt-5.9",
            "names are trimmed and lower-cased"
        );
        let cases: [(&str, PricingTable); 6] = [
            (
                "empty",
                PricingTable {
                    entries: vec![],
                    updated_at: None,
                },
            ),
            ("no name", remote_table("  ", 1.0)),
            ("long name", remote_table(&"x".repeat(101), 1.0)),
            ("odd characters", remote_table("gpt-5 <script>", 1.0)),
            ("negative rate", remote_table("gpt-5", -1.0)),
            ("absurd rate", remote_table("gpt-5", 1e9)),
        ];
        for (why, table) in cases {
            assert!(validate_remote(&table).is_err(), "{why} must be refused");
        }
        let mut infinite = remote_table("gpt-5", 1.0);
        infinite.entries[0].output_per_m = f64::INFINITY;
        assert!(validate_remote(&infinite).is_err());
        let mut duplicated = remote_table("gpt-5", 1.0);
        duplicated.entries.push(duplicated.entries[0].clone());
        assert!(validate_remote(&duplicated).is_err());
    }

    #[test]
    fn the_official_remote_table_and_custom_sources_are_rate_limited_daily() {
        let now = 1_800_000_000_000i64;
        let url = "https://example.com/pricing.json";
        let fresh = RemoteCache {
            url: url.into(),
            fetched_at_ms: now - 3_600_000,
            table: remote_table("gpt-5", 1.0),
        };
        // Empty settings resolve to the official source and therefore have
        // the same daily schedule as a configured source.
        assert!(should_fetch_remote("", None, now, false));
        assert!(should_fetch_remote("   ", None, now, true));
        // First run, and then once a day.
        assert!(should_fetch_remote(url, None, now, false));
        assert!(!should_fetch_remote(url, Some(&fresh), now, false));
        assert!(should_fetch_remote(url, Some(&fresh), now, true), "manual");
        let stale = RemoteCache {
            fetched_at_ms: now - REMOTE_MIN_INTERVAL_MS,
            ..fresh.clone()
        };
        assert!(should_fetch_remote(url, Some(&stale), now, false));
        // A different URL invalidates the cache.
        let other = RemoteCache {
            url: "https://example.com/other.json".into(),
            ..fresh.clone()
        };
        assert!(should_fetch_remote(url, Some(&other), now, false));
    }

    #[test]
    fn the_base_table_uses_a_valid_cache_and_falls_back_otherwise() {
        let dir = tempdir();
        let url = "https://example.com/pricing.json";
        assert_eq!(
            base_table(&dir, url).entries.len(),
            default_table().entries.len(),
            "no cache → the built-in table"
        );

        let write = |cache: &RemoteCache| {
            std::fs::create_dir_all(&dir).ok();
            std::fs::write(
                remote_cache_path(&dir),
                serde_json::to_vec_pretty(cache).unwrap(),
            )
            .unwrap();
        };
        let cache = RemoteCache {
            url: url.into(),
            fetched_at_ms: 1,
            table: remote_table("gpt-9-nebula", 7.0),
        };
        write(&cache);
        let table = base_table(&dir, url);
        assert_eq!(table.entries.len(), 1);
        assert_eq!(find_entry(&table, "gpt-9-nebula").unwrap().input_per_m, 7.0);
        // A cache fetched from another URL is ignored (the user changed it).
        assert_eq!(
            base_table(&dir, "https://example.com/other.json")
                .entries
                .len(),
            default_table().entries.len()
        );
        // So is a cache that no longer passes validation.
        write(&RemoteCache {
            table: remote_table("gpt-9-nebula", -1.0),
            ..cache.clone()
        });
        assert_eq!(
            base_table(&dir, url).entries.len(),
            default_table().entries.len()
        );
        // A saved user table still wins over the remote one.
        std::fs::write(remote_cache_path(&dir), serde_json::to_vec(&cache).unwrap()).ok();
        save(&dir, &remote_table("my-model", 1.0)).unwrap();
        let effective = load_with_base(&dir, base_table(&dir, url));
        assert_eq!(effective.entries.len(), 1);
        assert_eq!(effective.entries[0].model_pattern, "my-model");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn restoring_a_custom_table_reconciles_the_selected_prices() {
        // This mirrors a source switch that discovers its URL changed after
        // moving the custom file aside. The next in-memory reconciliation must
        // choose the restored file, rather than the new source's cache.
        let dir = tempdir();
        let config_dir = dir.join("config");
        let data_dir = dir.join("data");
        let source = "https://example.com/new-pricing.json";
        save(&config_dir, &remote_table("my-local-model", 9.0)).unwrap();
        cache_remote(&data_dir, source, &remote_table("source-model", 1.0)).unwrap();

        let backup = backup_custom_pricing(&config_dir).unwrap();
        let (during_switch, custom_during_switch) =
            selected_pricing(&config_dir, &data_dir, source);
        assert!(!custom_during_switch);
        assert_eq!(during_switch.entries[0].model_pattern, "source-model");

        restore_custom_pricing(backup.as_deref(), &config_dir);
        let (restored, custom_restored) = selected_pricing(&config_dir, &data_dir, source);
        assert!(custom_restored);
        assert_eq!(restored.entries[0].model_pattern, "my-local-model");
        assert_eq!(restored.entries[0].input_per_m, 9.0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn documented_codex_variants_have_their_own_exact_prices() {
        let table = default_table();
        for (model, input, output, cached) in [
            ("gpt-5.1-codex-max", 1.25, 10.0, 0.125),
            ("gpt-5.1-codex-mini", 0.25, 2.0, 0.025),
            ("gpt-5.2-codex", 1.75, 14.0, 0.175),
            ("codex-mini-latest", 1.5, 6.0, 0.375),
        ] {
            let entry = find_entry(&table, model).expect("documented model must be priced");
            assert_eq!(entry.model_pattern, model);
            assert_eq!(
                (
                    entry.input_per_m,
                    entry.output_per_m,
                    entry.cache_read_per_m
                ),
                (input, output, cached)
            );
            let estimated =
                estimate_cost(&table, model, &totals(1_000_000, 1_000_000, 0, 1_000_000)).unwrap();
            assert!((estimated - input - output - cached).abs() < 1e-9);
            assert!(find_entry(&table, &format!("{model}-unverified")).is_none());
        }
    }

    #[test]
    fn new_models_use_their_own_standard_prices() {
        let table = default_table();
        for (model, input, output, cache_write, cache_read) in [
            ("claude-opus-5-5", 4.0, 20.0, 5.0, 0.2),
            ("gpt-6-sol", 2.0, 10.0, 2.5, 0.2),
            ("gpt-6-luna", 0.1, 0.5, 0.125, 0.01),
        ] {
            let observed = if model.starts_with("claude-") {
                format!("{model}-20260922")
            } else {
                model.to_string()
            };
            let (entry, kind) = find_match(&table, &observed).unwrap();
            assert_eq!(kind, MatchKind::Exact, "{observed} used a family price");
            assert_eq!(entry.model_pattern, model);
            assert_eq!(
                (
                    entry.input_per_m,
                    entry.output_per_m,
                    entry.cache_write_per_m,
                    entry.cache_read_per_m
                ),
                (input, output, cache_write, cache_read)
            );
            let estimated = estimate_cost(
                &table,
                &observed,
                &totals(1_000_000, 1_000_000, 1_000_000, 1_000_000),
            )
            .unwrap();
            assert!((estimated - input - output - cache_write - cache_read).abs() < 1e-9);
        }
    }

    #[test]
    fn cost_uses_all_four_rates() {
        let t = default_table();
        // opus-4-5: 5 / 25 / 6.25 / 0.5 per 1M
        let cost = estimate_cost(
            &t,
            "claude-opus-4-5",
            &totals(1_000_000, 1_000_000, 1_000_000, 1_000_000),
        )
        .unwrap();
        assert!((cost - (5.0 + 25.0 + 6.25 + 0.5)).abs() < 1e-9);

        let cost = estimate_cost(&t, "gpt-5-nano", &totals(2_000_000, 1_000_000, 0, 0)).unwrap();
        assert!((cost - (0.1 + 0.4)).abs() < 1e-9);
    }

    #[test]
    fn saved_table_replaces_defaults_and_preserves_custom_prefixes() {
        let dir = tempdir();
        let overrides = PricingTable {
            entries: vec![
                PricingEntry {
                    model_pattern: "gpt-5".into(),
                    input_per_m: 99.0,
                    output_per_m: 1.0,
                    cache_write_per_m: 1.0,
                    cache_read_per_m: 1.0,
                },
                PricingEntry {
                    model_pattern: "my-local-model".into(),
                    input_per_m: 0.0,
                    output_per_m: 0.0,
                    cache_write_per_m: 0.0,
                    cache_read_per_m: 0.0,
                },
            ],
            updated_at: None,
        };
        let merged = save(&dir, &overrides).unwrap();
        assert!(merged.updated_at.is_some());
        assert_eq!(find_entry(&merged, "gpt-5").unwrap().input_per_m, 99.0);
        assert!(find_entry(&merged, "gpt-5-codex").is_none());
        assert_eq!(merged.entries.len(), 2);
        assert_eq!(
            find_entry(&merged, "my-local-model-x")
                .unwrap()
                .output_per_m,
            0.0
        );
        assert_eq!(load(&dir).entries.len(), merged.entries.len());

        std::fs::write(pricing_path(&dir), "{{ broken").unwrap();
        assert_eq!(load(&dir).entries.len(), default_table().entries.len());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deleting_builtin_custom_or_every_price_survives_reload() {
        let dir = tempdir();
        let mut table = default_table();
        table.entries.push(PricingEntry {
            model_pattern: "custom-model".into(),
            input_per_m: 1.0,
            output_per_m: 2.0,
            cache_write_per_m: 1.0,
            cache_read_per_m: 0.1,
        });
        save(&dir, &table).unwrap();
        table
            .entries
            .retain(|entry| !matches!(entry.model_pattern.as_str(), "gpt-5" | "custom-model"));
        save(&dir, &table).unwrap();
        let reloaded = load(&dir);
        assert!(find_entry(&reloaded, "gpt-5").is_none());
        assert!(find_entry(&reloaded, "custom-model").is_none());
        assert_eq!(reloaded.entries.len(), table.entries.len());
        table.entries.clear();
        assert!(save(&dir, &table).unwrap().entries.is_empty());
        assert!(load(&dir).entries.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_prices_do_not_replace_the_saved_table() {
        let dir = tempdir();
        let table = default_table();
        save(&dir, &table).unwrap();
        let before = std::fs::read(pricing_path(&dir)).unwrap();
        let mut invalid = table;
        invalid.entries[0].input_per_m = -1.0;
        assert!(save(&dir, &invalid).is_err());
        invalid.entries[0].input_per_m = f64::INFINITY;
        assert!(save(&dir, &invalid).is_err());
        assert_eq!(std::fs::read(pricing_path(&dir)).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_parsed_table_is_custom_even_when_one_entry_is_invalid() {
        let dir = tempdir();
        std::fs::write(
            pricing_path(&dir),
            r#"{"entries":[{"modelPattern":"","inputPerM":1,"outputPerM":1,"cacheWritePerM":1,"cacheReadPerM":1}],"updatedAt":null}"#,
        )
        .unwrap();
        assert!(has_custom_pricing(&dir));
        // `load_with_base` treats the file as a complete selection too: the
        // invalid entry is filtered, leaving an intentionally empty table.
        assert!(load_with_base(&dir, default_table()).entries.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn older_timestamped_remote_table_is_not_an_update() {
        let mut current = remote_table("gpt-6-astra", 10.0);
        current.updated_at = Some("2026-09-23T12:00:00Z".into());
        let mut older = remote_table("gpt-6-astra", 2.0);
        older.updated_at = Some("2026-09-22T12:00:00Z".into());
        assert!(!table_is_update(&older, &current));
        older.updated_at = Some("2026-09-24T12:00:00Z".into());
        assert!(table_is_update(&older, &current));
    }

    #[test]
    fn entry_order_is_a_pricing_revision() {
        let sol = remote_table("gpt-6-sol", 2.0)
            .entries
            .into_iter()
            .next()
            .unwrap();
        let astra = remote_table("gpt-6-astra", 10.0)
            .entries
            .into_iter()
            .next()
            .unwrap();
        let current = PricingTable {
            entries: vec![sol, astra],
            updated_at: None,
        };
        let mut reordered = current.clone();
        reordered.entries.reverse();
        assert!(table_is_update(&reordered, &current));
    }
}
