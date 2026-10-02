//! Replays the recorded quota history of a **copy** of a real usage database
//! and scores the forecast against what actually happened. [BACKEND]
//!
//! For every window (provider + kind, model-scoped windows skipped) the history
//! is cut into periods at each reset; at every stored sample the forecast is
//! recomputed from the data available *at that moment* and compared with the
//! period's outcome. Run it twice in your head: "percent only" is what the app
//! did before the token fallback, "with tokens" is what it does now.
//!
//! ```text
//! cargo run --example forecast_backtest -- ~/.local/share/io.github.harveyxiacn.ai-usage-sidebar/usage.db
//! ```

use ai_usage_sidebar_lib::commands::forecast::{backtest, BacktestReport};
use ai_usage_sidebar_lib::commands::store;
use ai_usage_sidebar_lib::model::WindowKind;
use anyhow::{Context, Result};

fn fmt(v: Option<f64>, unit: &str) -> String {
    v.map_or("n/a".to_string(), |v| format!("{v:.1} {unit}"))
}

fn print(label: &str, r: &BacktestReport) {
    println!(
        "  {label:<13} {} periods ({} hit 100 %), {} points -> {} forecasts ({} from tokens)",
        r.periods, r.periods_hit_limit, r.points, r.predicted_points, r.token_points
    );
    println!(
        "  {:<13} projected-at-reset MAE {}, run-out MAE {}, false alarms {}, misses {}",
        "",
        fmt(r.projected_mae_points, "pts"),
        fmt(r.exhaust_mae_minutes, "min"),
        r.false_alarm_points,
        r.missed_points
    );
}

fn main() -> Result<()> {
    let source = std::env::args()
        .nth(1)
        .context("usage: forecast_backtest <usage.db>")?;
    // Never open the live file: the app holds it in WAL mode.
    let copy = std::env::temp_dir().join(format!("forecast-backtest-{}.db", std::process::id()));
    std::fs::copy(&source, &copy)?;
    for ext in ["-wal", "-shm"] {
        let _ = std::fs::copy(format!("{source}{ext}"), format!("{}{ext}", copy.display()));
    }
    let db = store::Db::open(&copy)?;

    for provider in ["claude", "codex", "copilot", "openrouter"] {
        let tokens = store::usage_token_events(&db, provider, 0)?;
        for (kind, seconds) in [
            (WindowKind::FiveHour, 18_000u64),
            (WindowKind::SevenDay, 604_800),
        ] {
            let samples = store::quota::window_samples(&db, provider, kind, None, 0)?;
            if samples.is_empty() {
                continue;
            }
            println!("{provider} {kind:?}: {} samples", samples.len());
            print(
                "percent only",
                &backtest(&samples, None, kind, Some(seconds)),
            );
            if !tokens.is_empty() {
                print(
                    "with tokens",
                    &backtest(&samples, Some(&tokens), kind, Some(seconds)),
                );
            }
        }
    }
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", copy.display()));
    }
    Ok(())
}
