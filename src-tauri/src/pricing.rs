//! Default price list + cost estimation. [BACKEND owns this file]
//!
//! Prices are USD per 1M tokens (input / output / cache write / cache read) and
//! mirror the public API prices. Subscription users do not actually pay per
//! token — the estimate is a *comparison indicator* only (ARCHITECTURE §9).

use crate::model::{PricingEntry, PricingTable, TokenTotals};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub const PRICING_FILE: &str = "pricing.json";

/// `(model prefix, input, output, cache write, cache read)` — USD / 1M tokens.
const DEFAULTS: &[(&str, f64, f64, f64, f64)] = &[
    // --- Anthropic ---
    ("claude-opus-4", 15.0, 75.0, 18.75, 1.5),
    ("claude-opus-4-1", 15.0, 75.0, 18.75, 1.5),
    ("claude-opus-4-5", 5.0, 25.0, 6.25, 0.5),
    ("claude-opus-4-6", 5.0, 25.0, 6.25, 0.5),
    ("claude-opus-5", 5.0, 25.0, 6.25, 0.5),
    ("claude-sonnet-4", 3.0, 15.0, 3.75, 0.3),
    ("claude-sonnet-4-5", 3.0, 15.0, 3.75, 0.3),
    ("claude-sonnet-5", 3.0, 15.0, 3.75, 0.3),
    ("claude-haiku-4-5", 1.0, 5.0, 1.25, 0.1),
    ("claude-fable-5", 15.0, 75.0, 18.75, 1.5),
    ("claude-mythos-5", 15.0, 75.0, 18.75, 1.5),
    // --- OpenAI ---
    ("gpt-5", 1.25, 10.0, 1.25, 0.125),
    ("gpt-5-codex", 1.25, 10.0, 1.25, 0.125),
    ("gpt-5.1", 1.25, 10.0, 1.25, 0.125),
    ("gpt-5.1-codex", 1.25, 10.0, 1.25, 0.125),
    ("gpt-5.2", 1.75, 14.0, 1.75, 0.175),
    ("gpt-5.3-codex", 1.75, 14.0, 1.75, 0.175),
    ("gpt-5.4", 2.5, 15.0, 2.5, 0.25),
    ("gpt-5.5", 2.5, 15.0, 2.5, 0.25),
    ("gpt-6", 2.5, 15.0, 2.5, 0.25),
    ("gpt-6-astra", 2.5, 15.0, 2.5, 0.25),
    ("gpt-5-mini", 0.25, 2.0, 0.25, 0.025),
    ("gpt-5-nano", 0.05, 0.4, 0.05, 0.005),
    ("codex-mini", 1.5, 6.0, 1.5, 0.375),
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

/// Defaults merged with the user's `pricing.json` (same pattern → override).
pub fn load(config_dir: &Path) -> PricingTable {
    let mut table = default_table();
    let path = pricing_path(config_dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return table;
    };
    let overrides: PricingTable = match serde_json::from_str(&text) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("ignoring invalid {}: {}", path.display(), e);
            return table;
        }
    };
    for entry in overrides.entries {
        match table
            .entries
            .iter_mut()
            .find(|e| e.model_pattern.eq_ignore_ascii_case(&entry.model_pattern))
        {
            Some(existing) => *existing = entry,
            None => table.entries.push(entry),
        }
    }
    table.updated_at = overrides.updated_at;
    table
}

/// Persist the user's table atomically and return the merged result.
pub fn save(config_dir: &Path, table: &PricingTable) -> Result<PricingTable> {
    let mut to_write = table.clone();
    to_write.updated_at = Some(crate::commands::providers::now_rfc3339());
    std::fs::create_dir_all(config_dir).ok();
    crate::commands::settings::write_atomic(
        &pricing_path(config_dir),
        &serde_json::to_vec_pretty(&to_write).context("serialize pricing table")?,
    )?;
    Ok(load(config_dir))
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

/// Longest-prefix match on the (normalised) model name.
pub fn find_entry<'a>(table: &'a PricingTable, model: &str) -> Option<&'a PricingEntry> {
    let name = normalize_model(model);
    table
        .entries
        .iter()
        .filter(|e| {
            let p = e.model_pattern.to_ascii_lowercase();
            !p.is_empty() && name.starts_with(&p)
        })
        .max_by_key(|e| e.model_pattern.len())
}

/// Estimated USD cost of `totals` for `model`, or `None` for unknown models.
///
/// Reasoning tokens are already part of `output_tokens` for both providers and
/// are therefore not charged twice.
pub fn estimate_cost(table: &PricingTable, model: &str, totals: &TokenTotals) -> Option<f64> {
    let e = find_entry(table, model)?;
    let m = 1_000_000.0;
    Some(
        totals.input_tokens as f64 * e.input_per_m / m
            + totals.output_tokens as f64 * e.output_per_m / m
            + totals.cache_write_tokens as f64 * e.cache_write_per_m / m
            + totals.cache_read_tokens as f64 * e.cache_read_per_m / m,
    )
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
    fn unknown_models_have_no_price() {
        let t = default_table();
        assert!(find_entry(&t, "llama-9").is_none());
        assert!(estimate_cost(&t, "llama-9", &totals(1000, 1000, 0, 0)).is_none());
        assert!(estimate_cost(&t, "", &totals(1, 1, 1, 1)).is_none());
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
    fn user_overrides_merge_over_the_defaults() {
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
        assert_eq!(
            find_entry(&merged, "gpt-5-codex").unwrap().input_per_m,
            1.25
        );
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
}
