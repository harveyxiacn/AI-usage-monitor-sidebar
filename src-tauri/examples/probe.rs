//! End-to-end probe for the backend data layer. [BACKEND]
//!
//! Runs **without** a Tauri app: it fetches both providers over the network,
//! ingests the local session logs into a throw-away SQLite database and prints
//! a 30-day history. Handy for verifying the provider mapping against real
//! accounts.
//!
//! ```text
//! cargo run --example probe
//! ```
//!
//! Secrets are never printed: quotas carry no tokens, and account e-mails /
//! names are redacted before the JSON is dumped.

use ai_usage_sidebar_lib::commands::{ingest, providers, store};
use ai_usage_sidebar_lib::model::{Bucket, HistoryQuery, ProviderQuota};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let work_dir =
        std::env::temp_dir().join(format!("ai-usage-sidebar-probe-{}", std::process::id()));
    std::fs::create_dir_all(&work_dir)?;
    println!("probe workspace: {}", work_dir.display());

    // ---------- 1. providers ----------
    let ctx = providers::ProviderCtx::with_data_dir(&work_dir);
    let http = providers::http_client(&ctx.user_agent);
    println!("\n=== providers ===");
    for p in providers::all_providers(&ctx) {
        let info = p.info();
        println!(
            "\n--- {} ({}) logged_in={} plan={:?}\n    credentials: {}\n    logs:        {}",
            info.display_name,
            info.id,
            info.logged_in,
            info.plan_label,
            info.credential_path.as_deref().unwrap_or("-"),
            info.log_path.as_deref().unwrap_or("-"),
        );
        let quota = redact(p.fetch(&http).await);
        println!("{}", serde_json::to_string_pretty(&quota)?);
    }

    // ---------- 2. ingestion ----------
    println!("\n=== ingestion ===");
    for root in ingest::roots() {
        println!("  root {} ({})", root.path.display(), root.provider);
    }
    let db = store::Db::open(&work_dir.join("usage.db"))?;
    let stats = ingest::run(&db, true);
    println!("{}", serde_json::to_string_pretty(&stats)?);
    println!("stored events: {}", store::usage::count_events(&db)?);

    // ---------- 3. history ----------
    println!("\n=== history (last 30 days, by provider) ===");
    let now = chrono::Utc::now();
    let query = HistoryQuery {
        from: (now - chrono::Duration::days(30)).to_rfc3339(),
        to: now.to_rfc3339(),
        bucket: Bucket::Day,
        group_by_model: false,
        provider: None,
    };
    let pricing = ai_usage_sidebar_lib::commands::pricing::default_table();
    let history = store::query_history(&db, &query, &pricing)?;
    println!(
        "rows: {} (daily buckets per provider)\ntotals: {}\nbyProvider: {}",
        history.rows.len(),
        serde_json::to_string_pretty(&history.totals)?,
        serde_json::to_string_pretty(&history.by_provider)?,
    );

    let by_model = HistoryQuery {
        group_by_model: true,
        bucket: Bucket::Month,
        ..query
    };
    let models = store::query_history(&db, &by_model, &pricing)?;
    let mut rows = models.rows;
    rows.sort_by_key(|r| -r.totals.total_tokens);
    println!("\ntop models (30 days):");
    for row in rows.iter().take(8) {
        println!(
            "  {:<10} {:<22} {:>14} tokens  {:>8} req  {}",
            row.provider,
            row.model.as_deref().unwrap_or("-"),
            row.totals.total_tokens,
            row.totals.requests,
            row.totals
                .estimated_cost_usd
                .map(|c| format!("${c:.2}"))
                .unwrap_or_else(|| "n/a".into()),
        );
    }

    println!(
        "\ndone. remove {} when you are finished.",
        work_dir.display()
    );
    Ok(())
}

/// Blank out anything that identifies the account.
fn redact(mut quota: ProviderQuota) -> ProviderQuota {
    if let Some(account) = quota.account.as_mut() {
        account.email = account.email.as_deref().map(mask);
        account.name = account.name.as_deref().map(mask);
    }
    quota
}

fn mask(value: &str) -> String {
    let keep: String = value.chars().take(3).collect();
    format!("{keep}…<redacted>")
}
