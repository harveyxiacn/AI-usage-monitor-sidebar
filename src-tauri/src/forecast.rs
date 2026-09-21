//! "At this pace, when do I hit the limit?" — burn-rate projection. [BACKEND]
//!
//! Input is the `quota_samples` history the scheduler already records (see
//! `store/quota.rs`); output is an optional [`QuotaForecast`] per quota window,
//! which travels on the wire as `QuotaWindow.forecast`.
//!
//! [`forecast`] is pure, cheap and runs on every refresh. It is deliberately
//! conservative — a confident "you run out in 20 minutes" that is wrong is much
//! worse than no line at all:
//!
//! * samples from before the window's last reset are dropped. A reset shows up
//!   as a *drop* in `used_percent` going forward in time, or as a sample taken
//!   against a different `resets_at`.
//! * only the recent tail counts: `window_seconds / 7`, clamped to 30 min … 24 h
//!   (≈ 43 min for a 5-hour window, 24 h for a weekly one).
//! * the slope is a Theil–Sen estimator (the median of all pairwise slopes),
//!   which shrugs off a single burst and damps — rather than ignores — an idle
//!   plateau in the middle or at the end of the series.
//! * too few samples, too short a span, stale history, a flat or negative slope,
//!   a window that has already reset or is already full, or a projection that is
//!   within one percentage point of the current value all yield `None`.

use crate::commands::providers::{clamp_percent, rfc3339_from_unix_ms};
use crate::commands::store::Db;
use crate::model::{AppSnapshot, ForecastConfidence, ProviderStatus, QuotaForecast, WindowKind};

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;

/// Shortest estimation horizon — below this a 5-hour window is all noise.
const MIN_HORIZON_MS: i64 = 30 * MINUTE_MS;
/// Longest one: even a weekly window is judged on its most recent day.
const MAX_HORIZON_MS: i64 = 24 * HOUR_MS;
/// Fewer points than this is not a trend.
const MIN_SAMPLES: usize = 4;
/// Theil–Sen is O(n²); beyond this the extra points buy nothing.
const MAX_SAMPLES: usize = 60;
/// A backwards step larger than this (percentage points) marks a window reset.
const RESET_DROP: f64 = 0.5;
/// Projections closer than this to the current value are not worth showing.
const MIN_PROJECTED_GAIN: f64 = 1.0;
/// Runaway projections are meaningless; "≥ 999 %" is as useful as "8000 %".
const MAX_PROJECTED: f64 = 999.0;

/// One stored `quota_samples` row, reduced to what the estimator needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// unix ms
    pub ts_ms: i64,
    pub used_percent: f64,
    /// unix ms of the reset this sample was taken against
    pub resets_at_ms: Option<i64>,
}

/// The live window the forecast is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowSpec {
    pub kind: WindowKind,
    pub window_seconds: Option<u64>,
    pub used_percent: f64,
    /// unix ms
    pub resets_at_ms: Option<i64>,
}

/// How far back the estimator looks for a window of this length.
pub fn horizon_ms(kind: WindowKind, window_seconds: Option<u64>) -> i64 {
    let secs = window_seconds.unwrap_or(match kind {
        WindowKind::FiveHour => 5 * 3_600,
        WindowKind::SevenDay => 7 * 86_400,
        WindowKind::Other => 6 * 3_600,
    });
    let raw = (secs as i64).saturating_mul(1_000) / 7;
    raw.clamp(MIN_HORIZON_MS, MAX_HORIZON_MS)
}

/// History older than this means the provider stopped reporting — no forecast.
fn stale_after_ms(horizon: i64) -> i64 {
    (horizon / 4).clamp(15 * MINUTE_MS, 2 * HOUR_MS)
}

/// The samples must cover at least this much time before a slope means anything.
fn min_span_ms(horizon: i64) -> i64 {
    (horizon / 8).max(10 * MINUTE_MS)
}

/// Project `spec` forward from its stored `samples` (oldest first).
pub fn forecast(samples: &[Sample], spec: &WindowSpec, now_ms: i64) -> Option<QuotaForecast> {
    let resets_at = spec.resets_at_ms?;
    if resets_at <= now_ms {
        return None; // the window is resetting right now; wait for fresh data
    }
    let current = clamp_percent(spec.used_percent);
    if current >= 100.0 {
        return None; // already full — there is nothing left to predict
    }

    let horizon = horizon_ms(spec.kind, spec.window_seconds);
    // Freshness is judged on what the provider actually gave us, before any
    // reset filtering: a window nobody refreshed cannot be extrapolated.
    if now_ms - samples.last()?.ts_ms > stale_after_ms(horizon) {
        return None;
    }

    let mut series: Vec<Sample> = samples
        .iter()
        .copied()
        .filter(|s| s.ts_ms > now_ms - horizon && s.ts_ms <= now_ms)
        .collect();
    // Sample writes are throttled to one row per 5 minutes, so the live value
    // is regularly newer (and always at least as true) as the newest row.
    if series.last().is_none_or(|s| s.ts_ms < now_ms) {
        series.push(Sample {
            ts_ms: now_ms,
            used_percent: current,
            resets_at_ms: Some(resets_at),
        });
    }

    let series = since_last_reset(&series, Some(resets_at));
    if series.len() < MIN_SAMPLES {
        return None;
    }
    let span = series[series.len() - 1].ts_ms - series[0].ts_ms;
    if span < min_span_ms(horizon) {
        return None;
    }

    let rate = theil_sen(&thin(series, MAX_SAMPLES))? * HOUR_MS as f64;
    if !rate.is_finite() || rate <= 0.0 {
        return None;
    }

    let hours_left = (resets_at - now_ms) as f64 / HOUR_MS as f64;
    let gain = rate * hours_left;
    if !gain.is_finite() || gain < MIN_PROJECTED_GAIN {
        return None;
    }
    let projected = (current + gain).min(MAX_PROJECTED);

    // Only a projection that actually crosses the cap *before* the reset gets a
    // time; "100 % exactly at the reset" is not running out early.
    let exhausts_at = if projected > 100.0 {
        let ms = now_ms + (((100.0 - current) / rate) * HOUR_MS as f64).round() as i64;
        (ms < resets_at).then_some(ms)
    } else {
        None
    };

    Some(QuotaForecast {
        projected_percent_at_reset: round2(projected),
        exhausts_at: exhausts_at.and_then(rfc3339_from_unix_ms),
        rate_percent_per_hour: round2(rate),
        confidence: confidence_of(series.len(), span, horizon),
    })
}

/// The tail of `samples` that belongs to the window's current period.
///
/// Walks newest → oldest and stops at the first sample that cannot be part of
/// it: one taken against a different `resets_at`, or one whose percentage is
/// *higher* than the sample that follows it in time (the window reset in
/// between and started filling again).
fn since_last_reset(samples: &[Sample], resets_at_ms: Option<i64>) -> &[Sample] {
    let mut start = samples.len();
    let mut newer_percent = f64::INFINITY;
    for (i, s) in samples.iter().enumerate().rev() {
        let other_period = matches!((s.resets_at_ms, resets_at_ms), (Some(a), Some(b)) if a != b);
        if other_period || s.used_percent > newer_percent + RESET_DROP {
            break;
        }
        newer_percent = s.used_percent;
        start = i;
    }
    &samples[start..]
}

/// At most `max` evenly spaced samples, first and last always kept.
fn thin(series: &[Sample], max: usize) -> Vec<Sample> {
    if series.len() <= max || max < 2 {
        return series.to_vec();
    }
    let last = series.len() - 1;
    (0..max).map(|i| series[i * last / (max - 1)]).collect()
}

/// Theil–Sen slope in percentage points per millisecond: the median of the
/// slopes of every pair of samples. Robust to bursts, plateaus and outliers.
fn theil_sen(series: &[Sample]) -> Option<f64> {
    let mut slopes = Vec::with_capacity(series.len().saturating_sub(1) * series.len() / 2);
    for (i, a) in series.iter().enumerate() {
        for b in &series[i + 1..] {
            let dt = (b.ts_ms - a.ts_ms) as f64;
            if dt > 0.0 {
                slopes.push((b.used_percent - a.used_percent) / dt);
            }
        }
    }
    if slopes.is_empty() {
        return None;
    }
    slopes.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let mid = slopes.len() / 2;
    Some(if slopes.len() % 2 == 0 {
        (slopes[mid - 1] + slopes[mid]) / 2.0
    } else {
        slopes[mid]
    })
}

/// Confidence from how many samples there are and how much of the estimation
/// horizon they cover.
fn confidence_of(count: usize, span: i64, horizon: i64) -> ForecastConfidence {
    let covered = span as f64 / horizon as f64;
    if count >= 8 && covered >= 0.5 {
        ForecastConfidence::High
    } else if count >= 5 && covered >= 0.25 {
        ForecastConfidence::Medium
    } else {
        ForecastConfidence::Low
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// RFC 3339 → unix ms. Shared with the scheduler's predictive notification.
pub fn parse_ms(rfc3339: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .ok()
        .map(|d| d.timestamp_millis())
}

// ---------- snapshot integration ----------

/// Fill `forecast` on every window of `snapshot` from the stored samples.
///
/// Called by the scheduler on the blocking pool right after the new samples
/// were written, so the newest value is already in the database. Providers that
/// are not reporting live data are left alone: their windows are last-known
/// values, and extrapolating those would invent usage.
pub fn attach(db: &Db, snapshot: &mut AppSnapshot, now_ms: i64) {
    for provider in &mut snapshot.providers {
        if provider.status != ProviderStatus::Ok {
            continue;
        }
        for w in &mut provider.windows {
            let spec = WindowSpec {
                kind: w.kind,
                window_seconds: w.window_seconds,
                used_percent: w.used_percent,
                resets_at_ms: w.resets_at.as_deref().and_then(parse_ms),
            };
            let since = now_ms - horizon_ms(spec.kind, spec.window_seconds);
            match crate::commands::store::quota::window_samples(
                db,
                &provider.provider,
                w.kind,
                w.scope.as_deref(),
                since,
            ) {
                Ok(samples) => w.forecast = forecast(&samples, &spec, now_ms),
                Err(e) => log::debug!(
                    "forecast: cannot read samples for {} {}: {e:#}",
                    provider.provider,
                    w.label
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_789_430_400_000; // 2026-09-15T00:00:00Z
    const FIVE_HOUR: u64 = 18_000;
    const WEEKLY: u64 = 604_800;

    fn spec(secs: u64, used: f64, resets_in_ms: i64) -> WindowSpec {
        WindowSpec {
            kind: WindowKind::from_seconds(secs),
            window_seconds: Some(secs),
            used_percent: used,
            resets_at_ms: Some(T0 + resets_in_ms),
        }
    }

    /// Samples every `step_ms`, ending at `T0`, taking percentages from `pcts`.
    fn series(pcts: &[f64], step_ms: i64) -> Vec<Sample> {
        let n = pcts.len() as i64;
        pcts.iter()
            .enumerate()
            .map(|(i, &p)| Sample {
                ts_ms: T0 - (n - 1 - i as i64) * step_ms,
                used_percent: p,
                resets_at_ms: None,
            })
            .collect()
    }

    #[test]
    fn steady_burn_projects_the_same_slope_forward() {
        // 10 percentage points every 5 min = 120 %/h, 40 % used, 1 h to go.
        let samples = series(&[0.0, 10.0, 20.0, 30.0, 40.0], 5 * MINUTE_MS);
        let f = forecast(&samples, &spec(FIVE_HOUR, 40.0, HOUR_MS), T0).unwrap();
        assert_eq!(f.rate_percent_per_hour, 120.0);
        assert_eq!(f.projected_percent_at_reset, 160.0);
        // 60 % left at 120 %/h → half an hour.
        assert_eq!(f.exhausts_at.as_deref(), Some("2026-09-15T00:30:00Z"));
        assert_eq!(f.confidence, ForecastConfidence::Medium);
    }

    #[test]
    fn a_projection_below_one_hundred_has_no_exhaustion_time() {
        // 1 point every 5 min = 12 %/h, 20 % used, 2 h to go → 44 %.
        let samples = series(&[16.0, 17.0, 18.0, 19.0, 20.0], 5 * MINUTE_MS);
        let f = forecast(&samples, &spec(FIVE_HOUR, 20.0, 2 * HOUR_MS), T0).unwrap();
        assert_eq!(f.rate_percent_per_hour, 12.0);
        assert_eq!(f.projected_percent_at_reset, 44.0);
        assert_eq!(f.exhausts_at, None);
    }

    #[test]
    fn a_projection_beyond_the_reset_has_no_exhaustion_time() {
        // 12 %/h with 80 % used and 1 h to go: 92 % at the reset, never 100 %.
        let samples = series(&[76.0, 77.0, 78.0, 79.0, 80.0], 5 * MINUTE_MS);
        let f = forecast(&samples, &spec(FIVE_HOUR, 80.0, HOUR_MS), T0).unwrap();
        assert_eq!(f.projected_percent_at_reset, 92.0);
        assert_eq!(f.exhausts_at, None);
    }

    #[test]
    fn bursty_usage_reads_as_its_average_pace() {
        // Three 12-point bursts vs. a perfectly even ramp over the same 40 min.
        // Between two consecutive samples the bursty series alternates between
        // 0 %/h and 144 %/h; the median of the pairwise slopes lands near the
        // 36 %/h the even ramp reports.
        let steady = series(
            &[0.0, 3.0, 6.0, 9.0, 12.0, 15.0, 18.0, 21.0, 24.0],
            5 * MINUTE_MS,
        );
        let bursty = series(
            &[0.0, 0.0, 0.0, 12.0, 12.0, 12.0, 24.0, 24.0, 24.0],
            5 * MINUTE_MS,
        );
        let steady = forecast(&steady, &spec(FIVE_HOUR, 24.0, 2 * HOUR_MS), T0).unwrap();
        let bursty = forecast(&bursty, &spec(FIVE_HOUR, 24.0, 2 * HOUR_MS), T0).unwrap();
        assert_eq!(steady.rate_percent_per_hour, 36.0);
        assert!(
            (bursty.rate_percent_per_hour - 36.0).abs() < 12.0,
            "bursty pace {} should stay near the 36 %/h average",
            bursty.rate_percent_per_hour
        );
    }

    #[test]
    fn an_idle_plateau_in_the_middle_does_not_break_the_trend() {
        // Work, a 15-minute break, work again — still a usable pace.
        let samples = series(
            &[0.0, 8.0, 16.0, 16.0, 16.0, 16.0, 24.0, 32.0, 40.0],
            5 * MINUTE_MS,
        );
        let f = forecast(&samples, &spec(FIVE_HOUR, 40.0, HOUR_MS), T0).unwrap();
        assert!(
            f.rate_percent_per_hour > 30.0,
            "a plateau between two bursts must not zero the rate, got {}",
            f.rate_percent_per_hour
        );
    }

    #[test]
    fn a_long_idle_tail_stops_the_forecast() {
        // 27 points burnt in 10 min, then half an hour of nothing: most of the
        // recent history is flat, so "at this pace" no longer means anything.
        let samples = series(
            &[13.0, 27.0, 40.0, 40.0, 40.0, 40.0, 40.0, 40.0, 40.0],
            5 * MINUTE_MS,
        );
        assert!(forecast(&samples, &spec(FIVE_HOUR, 40.0, 2 * HOUR_MS), T0).is_none());
    }

    #[test]
    fn a_fully_idle_window_has_no_forecast() {
        let samples = series(&[42.0; 10], 5 * MINUTE_MS);
        assert!(forecast(&samples, &spec(FIVE_HOUR, 42.0, 2 * HOUR_MS), T0).is_none());
    }

    #[test]
    fn a_falling_series_has_no_forecast() {
        // Providers do correct percentages downwards occasionally.
        let samples = series(&[30.0, 29.8, 29.6, 29.4, 29.2], 5 * MINUTE_MS);
        assert!(forecast(&samples, &spec(FIVE_HOUR, 29.2, 2 * HOUR_MS), T0).is_none());
    }

    #[test]
    fn a_negligible_projection_is_not_reported() {
        // 0.6 %/h with 10 min left is 0.1 points — below the 1-point floor.
        let samples = series(&[9.5, 9.55, 9.6, 9.65, 9.7], 5 * MINUTE_MS);
        assert!(forecast(&samples, &spec(FIVE_HOUR, 9.7, 10 * MINUTE_MS), T0).is_none());
    }

    #[test]
    fn samples_from_before_a_reset_are_not_used() {
        // 80 → 95 in the old period, then 2 → 14 in the new one. Regressing
        // across the reset would read as a *negative* slope.
        let samples = series(
            &[80.0, 90.0, 95.0, 2.0, 5.0, 8.0, 11.0, 14.0],
            5 * MINUTE_MS,
        );
        let f = forecast(&samples, &spec(FIVE_HOUR, 14.0, HOUR_MS), T0).unwrap();
        assert_eq!(f.rate_percent_per_hour, 36.0);
        assert_eq!(f.projected_percent_at_reset, 50.0);
    }

    #[test]
    fn samples_taken_against_another_reset_time_are_not_used() {
        let mut samples = series(
            &[80.0, 90.0, 95.0, 2.0, 5.0, 8.0, 11.0, 14.0],
            5 * MINUTE_MS,
        );
        let old = Some(T0 - 2 * HOUR_MS);
        let new = Some(T0 + HOUR_MS);
        for (i, s) in samples.iter_mut().enumerate() {
            s.resets_at_ms = if i < 3 { old } else { new };
        }
        let spec = WindowSpec {
            resets_at_ms: new,
            ..spec(FIVE_HOUR, 14.0, HOUR_MS)
        };
        let f = forecast(&samples, &spec, T0).unwrap();
        assert_eq!(f.rate_percent_per_hour, 36.0);

        // The same series with only pre-reset rows left has nothing usable.
        for s in samples.iter_mut() {
            s.resets_at_ms = old;
        }
        assert!(forecast(&samples, &spec, T0).is_none());
    }

    #[test]
    fn too_few_samples_give_no_forecast() {
        let samples = series(&[0.0, 20.0, 40.0], 5 * MINUTE_MS);
        // Three rows plus the live point is exactly the minimum…
        assert!(forecast(&samples, &spec(FIVE_HOUR, 60.0, HOUR_MS), T0 + MINUTE_MS).is_some());
        // …two are not.
        let samples = series(&[0.0, 20.0], 5 * MINUTE_MS);
        assert!(forecast(&samples, &spec(FIVE_HOUR, 40.0, HOUR_MS), T0 + MINUTE_MS).is_none());
    }

    #[test]
    fn too_short_a_span_gives_no_forecast() {
        // Four samples but only 6 min of history for a 5-hour window (min 10).
        let samples = series(&[0.0, 5.0, 10.0, 15.0], 2 * MINUTE_MS);
        assert!(forecast(&samples, &spec(FIVE_HOUR, 15.0, HOUR_MS), T0).is_none());
        // The same four samples spread over 40 min are fine.
        let samples = series(&[0.0, 5.0, 10.0, 15.0], 10 * MINUTE_MS);
        assert!(forecast(&samples, &spec(FIVE_HOUR, 15.0, HOUR_MS), T0).is_some());
    }

    #[test]
    fn stale_history_gives_no_forecast() {
        let samples = series(&[0.0, 10.0, 20.0, 30.0, 40.0], 5 * MINUTE_MS);
        // 14 min after the newest row is still fresh, 20 min is not.
        assert!(forecast(
            &samples,
            &spec(FIVE_HOUR, 40.0, HOUR_MS),
            T0 + 14 * MINUTE_MS
        )
        .is_some());
        assert!(forecast(
            &samples,
            &spec(FIVE_HOUR, 40.0, HOUR_MS),
            T0 + 20 * MINUTE_MS
        )
        .is_none());
        assert!(forecast(&[], &spec(FIVE_HOUR, 40.0, HOUR_MS), T0).is_none());
    }

    #[test]
    fn a_window_without_or_past_its_reset_has_no_forecast() {
        let samples = series(&[0.0, 10.0, 20.0, 30.0, 40.0], 5 * MINUTE_MS);
        let mut s = spec(FIVE_HOUR, 40.0, HOUR_MS);
        s.resets_at_ms = None;
        assert!(forecast(&samples, &s, T0).is_none());
        s.resets_at_ms = Some(T0 - MINUTE_MS);
        assert!(forecast(&samples, &s, T0).is_none());
    }

    #[test]
    fn a_full_window_has_no_forecast() {
        let samples = series(&[60.0, 70.0, 80.0, 90.0, 100.0], 5 * MINUTE_MS);
        assert!(forecast(&samples, &spec(FIVE_HOUR, 100.0, HOUR_MS), T0).is_none());
    }

    #[test]
    fn a_runaway_projection_is_capped_but_still_exceeds_one_hundred() {
        // 120 %/h with 4 h to go = 520 %; the wire value is clamped at 999.
        let samples = series(&[0.0, 10.0, 20.0, 30.0, 40.0], 5 * MINUTE_MS);
        let f = forecast(&samples, &spec(FIVE_HOUR, 40.0, 4 * HOUR_MS), T0).unwrap();
        assert_eq!(f.projected_percent_at_reset, 520.0);
        let f = forecast(&samples, &spec(FIVE_HOUR, 40.0, 40 * HOUR_MS), T0).unwrap();
        assert_eq!(f.projected_percent_at_reset, MAX_PROJECTED);
        assert!(f.exhausts_at.is_some());
    }

    #[test]
    fn the_horizon_follows_the_window_length() {
        assert_eq!(horizon_ms(WindowKind::FiveHour, Some(FIVE_HOUR)), 2_571_428);
        assert_eq!(
            horizon_ms(WindowKind::SevenDay, Some(WEEKLY)),
            MAX_HORIZON_MS
        );
        // A one-minute window still gets the 30-minute floor.
        assert_eq!(horizon_ms(WindowKind::Other, Some(60)), MIN_HORIZON_MS);
        // Unknown lengths fall back to the kind.
        assert_eq!(
            horizon_ms(WindowKind::SevenDay, None),
            horizon_ms(WindowKind::SevenDay, Some(WEEKLY))
        );
    }

    #[test]
    fn a_weekly_window_ignores_five_hour_scale_history() {
        // Half a day of steady 1 %/h with 3 days to go → 31 % + 72 % = 103 %.
        let pcts: Vec<f64> = (0..25).map(|i| 10.0 + i as f64 * 0.5).collect();
        let samples = series(&pcts, 30 * MINUTE_MS);
        let f = forecast(&samples, &spec(WEEKLY, 22.0, 3 * 24 * HOUR_MS), T0).unwrap();
        assert_eq!(f.rate_percent_per_hour, 1.0);
        assert_eq!(f.projected_percent_at_reset, 94.0);
        // 25 samples covering half of the 24 h horizon.
        assert_eq!(f.confidence, ForecastConfidence::High);

        // The same 5-hour-scale burst inside a weekly window is only 2 h of the
        // 24 h horizon, which is too little to extrapolate a week from.
        let samples = series(&[0.0, 10.0, 20.0, 30.0, 40.0], 30 * MINUTE_MS);
        assert!(forecast(&samples, &spec(WEEKLY, 40.0, 3 * 24 * HOUR_MS), T0).is_none());
    }

    #[test]
    fn confidence_grows_with_samples_and_covered_span() {
        let horizon = horizon_ms(WindowKind::FiveHour, Some(FIVE_HOUR));
        assert_eq!(confidence_of(4, horizon, horizon), ForecastConfidence::Low);
        assert_eq!(
            confidence_of(12, horizon / 8, horizon),
            ForecastConfidence::Low
        );
        assert_eq!(
            confidence_of(5, horizon / 4, horizon),
            ForecastConfidence::Medium
        );
        assert_eq!(
            confidence_of(8, horizon / 2, horizon),
            ForecastConfidence::High
        );

        // End to end: a full 5-hour horizon sampled every 5 min is "high".
        let pcts: Vec<f64> = (0..9).map(|i| i as f64 * 3.0).collect();
        let samples = series(&pcts, 5 * MINUTE_MS);
        let f = forecast(&samples, &spec(FIVE_HOUR, 24.0, 2 * HOUR_MS), T0).unwrap();
        assert_eq!(f.confidence, ForecastConfidence::High);
    }

    #[test]
    fn thinning_keeps_the_ends_and_the_shape() {
        let samples = series(&(0..200).map(|i| i as f64 * 0.5).collect::<Vec<_>>(), 1_000);
        let thinned = thin(&samples, MAX_SAMPLES);
        assert_eq!(thinned.len(), MAX_SAMPLES);
        assert_eq!(thinned[0], samples[0]);
        assert_eq!(thinned[MAX_SAMPLES - 1], samples[samples.len() - 1]);
        assert_eq!(thin(&samples[..3], MAX_SAMPLES).len(), 3);
    }
}
