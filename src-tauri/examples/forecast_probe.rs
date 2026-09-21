//! Runs the forecast estimator against a **copy** of a real usage database.
//! [BACKEND]
//!
//! The estimator is unit tested on synthetic series only; this shows what it
//! says about real polling data (irregular spacing, idle nights, resets).
//!
//! ```text
//! cargo run --example forecast_probe -- ~/.local/share/io.github.harveyxiacn.ai-usage-sidebar/usage.db
//! ```

use ai_usage_sidebar_lib::commands::{forecast, store};
use ai_usage_sidebar_lib::model::WindowKind;
use anyhow::{Context, Result};

fn main() -> Result<()> {
    let source = std::env::args()
        .nth(1)
        .context("usage: forecast_probe <usage.db>")?;
    // Never open the live file: the app holds it in WAL mode.
    let copy = std::env::temp_dir().join(format!("forecast-probe-{}.db", std::process::id()));
    std::fs::copy(&source, &copy)?;
    for ext in ["-wal", "-shm"] {
        let _ = std::fs::copy(format!("{source}{ext}"), format!("{}{ext}", copy.display()));
    }
    let db = store::Db::open(&copy)?;
    let now = store::now_ms();

    for (provider, kind, seconds) in [
        ("claude", WindowKind::FiveHour, 18_000),
        ("claude", WindowKind::SevenDay, 604_800),
        ("codex", WindowKind::SevenDay, 604_800),
    ] {
        let since = now - forecast::horizon_ms(kind, Some(seconds)) * 2;
        let samples = store::quota::window_samples(&db, provider, kind, None, since)?;
        let Some(last) = samples.last().copied() else {
            println!("{provider} {kind:?}: no samples");
            continue;
        };
        let spec = forecast::WindowSpec {
            kind,
            window_seconds: Some(seconds),
            used_percent: last.used_percent,
            resets_at_ms: last.resets_at_ms,
        };
        println!(
            "{provider} {kind:?}: {} samples in range, now {:.0} %, resets in {:.0} min",
            samples.len(),
            last.used_percent,
            last.resets_at_ms
                .map_or(f64::NAN, |r| (r - now) as f64 / 60_000.0),
        );
        match forecast::forecast(&samples, &spec, now) {
            Some(f) => println!(
                "    {:.1} %/h, projected {:.0} % at reset, exhausts {:?}, confidence {:?}",
                f.rate_percent_per_hour, f.projected_percent_at_reset, f.exhausts_at, f.confidence
            ),
            None => println!("    no forecast"),
        }
    }
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", copy.display()));
    }
    Ok(())
}
