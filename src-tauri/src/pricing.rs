//! Default price list + cost estimation. [BACKEND owns this file]
//!
//! Prices are USD per 1M tokens (input / output / cache write / cache read) and
//! mirror the public API prices. Subscription users do not actually pay per
//! token — the estimate is a *comparison indicator* only (ARCHITECTURE §9).

use crate::model::{PricingEntry, PricingTable, TokenTotals};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub const PRICING_FILE: &str = "pricing.json";

/// `(model id, input, output, cache write, cache read)` — USD / 1M tokens.
/// Sources checked 2026-09-20:
/// https://platform.claude.com/docs/en/about-claude/pricing
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
        updated_at: None,
    }
}

pub fn pricing_path(config_dir: &Path) -> PathBuf {
    config_dir.join(PRICING_FILE)
}

/// The saved table is the user's complete selection, including deletions.
/// Defaults are used only when no readable, valid pricing file exists.
pub fn load(config_dir: &Path) -> PricingTable {
    load_with_base(config_dir, default_table())
}

/// Like `load`, but with an explicit fallback table — the bundled defaults,
/// or the cached remote table when the user configured `pricingUrl`.
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

// ---------- opt-in remote price list ----------
//
// Off by default and never contacted unless the user puts an https URL in
// `Settings.pricingUrl`: the README promises no third-party service.

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

/// Whether the remote table should be downloaded now (pure).
///
/// Nothing is ever fetched without a URL. Otherwise: a manual "refresh
/// prices now" always fetches, a changed URL invalidates the cache, and the
/// background loop fetches at most once a day.
pub fn should_fetch_remote(
    url: &str,
    cache: Option<&RemoteCache>,
    now_ms: i64,
    manual: bool,
) -> bool {
    if url.trim().is_empty() {
        return false;
    }
    if manual {
        return true;
    }
    match cache {
        None => true,
        Some(c) if c.url != url.trim() => true,
        Some(c) => now_ms.saturating_sub(c.fetched_at_ms) >= REMOTE_MIN_INTERVAL_MS,
    }
}

/// The cached remote table, but only if it was fetched from `url`.
pub fn cached_remote(data_dir: &Path, url: &str) -> Option<RemoteCache> {
    let text = std::fs::read_to_string(remote_cache_path(data_dir)).ok()?;
    let cache: RemoteCache = serde_json::from_str(&text).ok()?;
    (cache.url == url.trim()).then_some(cache)
}

/// The table the app starts from: the cached remote list when the user
/// enabled one, otherwise the bundled defaults. Any problem falls back.
pub fn base_table(data_dir: &Path, url: &str) -> PricingTable {
    match cached_remote(data_dir, url) {
        Some(cache) => match validate_remote(&cache.table) {
            Ok(table) => table,
            Err(e) => {
                log::warn!(
                    "cached remote pricing table is unusable ({e:#}), using the built-in one"
                );
                default_table()
            }
        },
        None => default_table(),
    }
}

/// Download, validate and cache the table at `url`. Returns the new table,
/// or `None` when nothing needed to be fetched. Never touches the network
/// unless `should_fetch_remote` says so.
pub async fn refresh_remote(
    http: &reqwest::Client,
    data_dir: &Path,
    url: &str,
    manual: bool,
) -> Result<Option<PricingTable>> {
    let existing = cached_remote(data_dir, url);
    if !should_fetch_remote(
        url,
        existing.as_ref(),
        crate::commands::store::now_ms(),
        manual,
    ) {
        return Ok(None);
    }
    let parsed = validate_url(url)?;
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
    let table = validate_remote(&parsed)?;

    let cache = RemoteCache {
        url: url.trim().to_string(),
        fetched_at_ms: crate::commands::store::now_ms(),
        table: table.clone(),
    };
    crate::commands::settings::write_atomic(
        &remote_cache_path(data_dir),
        &serde_json::to_vec_pretty(&cache).context("serialize the pricing cache")?,
    )?;
    log::info!(
        "pricing table updated from {url} ({} models)",
        table.entries.len()
    );
    Ok(Some(table))
}

/// Keep the effective table in step with an opt-in remote list: one check a
/// minute after start, then hourly. `refresh_remote` itself rate-limits the
/// download to once a day and does nothing at all without a `pricingUrl`.
pub fn start_remote_refresh(app: tauri::AppHandle) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        loop {
            let (http, url, data_dir, config_dir) = {
                let state = app.state::<crate::state::AppState>();
                let settings = state.settings.read();
                (
                    state.http.clone(),
                    settings.pricing_url.clone(),
                    state.data_dir.clone(),
                    state.config_dir.clone(),
                )
            };
            match refresh_remote(&http, &data_dir, &url, false).await {
                Ok(Some(_)) => {
                    let merged = load_with_base(&config_dir, base_table(&data_dir, &url));
                    *app.state::<crate::state::AppState>().pricing.write() = merged;
                }
                Ok(None) => {}
                Err(e) => log::warn!("pricing refresh failed, keeping the current table: {e:#}"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(3_600)).await;
        }
    });
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
    fn the_remote_table_is_opt_in_and_fetched_at_most_daily() {
        let now = 1_800_000_000_000i64;
        let url = "https://example.com/pricing.json";
        let fresh = RemoteCache {
            url: url.into(),
            fetched_at_ms: now - 3_600_000,
            table: remote_table("gpt-5", 1.0),
        };
        // No URL = no request, ever, not even a manual one.
        assert!(!should_fetch_remote("", None, now, false));
        assert!(!should_fetch_remote("   ", None, now, true));
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
}
