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
//! * when the percentages are too few, too brief or flat, [`forecast_with_tokens`]
//!   estimates the pace from token usage events instead (one confidence step
//!   lower); `backtest` replays history to measure the error of either path.
//! * too few samples, too short a span, stale history, a flat or negative slope,
//!   a window that has already reset or is already full, or a projection that is
//!   within one percentage point of the current value all yield `None`.

use crate::commands::providers::{clamp_percent, rfc3339_from_unix_ms};
use crate::commands::store::{self, Db};
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
/// Start of the window's current period (unix ms) when both the reset time and
/// the window length are known.
pub fn period_start_ms(spec: &WindowSpec) -> Option<i64> {
    let secs = spec.window_seconds?;
    Some(spec.resets_at_ms? - (secs as i64).saturating_mul(1_000))
}

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

/// One token-usage event of the provider (`usage_events`), reduced to what the
/// token-based fallback needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TokenEvent {
    /// unix ms
    pub ts_ms: i64,
    pub tokens: i64,
}

/// Project `spec` forward from its stored `samples` (oldest first), using only
/// the quota percentages.
pub fn forecast(samples: &[Sample], spec: &WindowSpec, now_ms: i64) -> Option<QuotaForecast> {
    forecast_with_tokens(samples, spec, now_ms, None)
}

/// Like [`forecast`], with a fallback for the case the percentages cannot
/// answer: the provider reports whole percent only, so at a low burn rate the
/// value sits on a plateau for many minutes and the Theil–Sen slope is zero.
///
/// `tokens` are the provider's token events **since the start of the window's
/// current period** (oldest first), only for a window that is not scoped to a
/// model. When the percentage series is too short, too brief or flat, the burn
/// rate is estimated from the tokens spent inside the estimation horizon,
/// scaled by the tokens-per-percent ratio observed in this very period. Such a
/// forecast always carries a confidence one step lower than the same
/// sample/span evidence would earn from percentages.
pub fn forecast_with_tokens(
    samples: &[Sample],
    spec: &WindowSpec,
    now_ms: i64,
    tokens: Option<&[TokenEvent]>,
) -> Option<QuotaForecast> {
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

    let (rate, confidence) = match percent_estimate(series, horizon) {
        PercentEstimate::Rate(rate, confidence) => (rate, confidence),
        PercentEstimate::Falling => return None,
        PercentEstimate::Unusable => token_estimate(tokens?, current, now_ms, horizon)?,
    };

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
        confidence,
    })
}

/// What the percentage series alone can say.
enum PercentEstimate {
    /// percentage points per hour (> 0) and how far to trust it
    Rate(f64, ForecastConfidence),
    /// a negative slope: the provider corrected itself downwards; say nothing
    Falling,
    /// too few points, too short a span, or a flat series
    Unusable,
}

fn percent_estimate(series: &[Sample], horizon: i64) -> PercentEstimate {
    if series.len() < MIN_SAMPLES {
        return PercentEstimate::Unusable;
    }
    let span = series[series.len() - 1].ts_ms - series[0].ts_ms;
    if span < min_span_ms(horizon) {
        return PercentEstimate::Unusable;
    }
    let Some(slope) = theil_sen(&thin(series, MAX_SAMPLES)) else {
        return PercentEstimate::Unusable;
    };
    let rate = slope * HOUR_MS as f64;
    if !rate.is_finite() {
        return PercentEstimate::Unusable;
    }
    if rate < 0.0 {
        return PercentEstimate::Falling;
    }
    if rate == 0.0 {
        return PercentEstimate::Unusable;
    }
    PercentEstimate::Rate(rate, confidence_of(series.len(), span, horizon))
}

/// Tokens per 1 % must come from at least this much quota, or one rounding
/// step of the integer percentage is too large a share of the ratio.
const MIN_RATIO_PERCENT: f64 = 2.0;
/// Fewer recent events than this is a blip, not a pace.
const MIN_TOKEN_EVENTS: usize = 3;

/// Burn rate from token usage: the tokens spent inside the horizon, divided by
/// the tokens-per-percent ratio of the current period.
fn token_estimate(
    events: &[TokenEvent],
    current: f64,
    now_ms: i64,
    horizon: i64,
) -> Option<(f64, ForecastConfidence)> {
    if current < MIN_RATIO_PERCENT {
        return None;
    }
    let events: Vec<&TokenEvent> = events
        .iter()
        .filter(|e| e.ts_ms <= now_ms && e.tokens > 0)
        .collect();
    let period_tokens: i64 = events.iter().map(|e| e.tokens).sum();
    if period_tokens <= 0 {
        return None;
    }
    let tokens_per_percent = period_tokens as f64 / current;

    let recent: Vec<&&TokenEvent> = events
        .iter()
        .filter(|e| e.ts_ms > now_ms - horizon)
        .collect();
    if recent.len() < MIN_TOKEN_EVENTS {
        return None;
    }
    // Like a stale sample: no activity lately means no pace to extrapolate.
    if now_ms - recent[recent.len() - 1].ts_ms > stale_after_ms(horizon) {
        return None;
    }
    // Spread over the whole horizon (the conservative reading) unless the
    // activity only started inside it.
    let first = recent[0].ts_ms;
    let elapsed = (now_ms - first).clamp(min_span_ms(horizon), horizon);
    let recent_tokens: i64 = recent.iter().map(|e| e.tokens).sum();
    let rate = recent_tokens as f64 / tokens_per_percent / (elapsed as f64 / HOUR_MS as f64);
    if !rate.is_finite() || rate <= 0.0 {
        return None;
    }
    let mut confidence = downgrade(confidence_of(recent.len(), now_ms - first, horizon));
    if current < 5.0 {
        confidence = ForecastConfidence::Low; // ratio resting on a handful of points
    }
    Some((rate, confidence))
}

fn downgrade(c: ForecastConfidence) -> ForecastConfidence {
    match c {
        ForecastConfidence::High => ForecastConfidence::Medium,
        _ => ForecastConfidence::Low,
    }
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

// ---------- backtest ----------

/// What replaying recorded history says about the estimator's accuracy.
/// `examples/forecast_backtest.rs` prints it for a real database.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BacktestReport {
    /// distinct window periods found in the samples
    pub periods: usize,
    /// periods whose recorded history reached 100 %
    pub periods_hit_limit: usize,
    /// replayed sample points that were evaluated
    pub points: usize,
    /// points at which a forecast was produced
    pub predicted_points: usize,
    /// points whose forecast came from the token fallback
    pub token_points: usize,
    /// mean |projected % at reset − final %| (both capped at 100), in points
    pub projected_mae_points: Option<f64>,
    /// mean |predicted run-out − actual run-out| over points that predicted a
    /// run-out in a period that really ran out, in minutes
    pub exhaust_mae_minutes: Option<f64>,
    /// predicted a run-out in a period that never reached 100 %
    pub false_alarm_points: usize,
    /// no run-out predicted although the period reached 100 % later
    pub missed_points: usize,
}

/// Replay `samples` (oldest first, one window) and compare every forecast with
/// what happened afterwards. Periods are split like `since_last_reset` does
/// (new `resets_at` or a falling percentage); the last period is still running,
/// so it only counts when it already hit the limit. `tokens` switches the token
/// fallback on, exactly as `attach` does (events of the whole history, oldest
/// first).
pub fn backtest(
    samples: &[Sample],
    tokens: Option<&[TokenEvent]>,
    kind: WindowKind,
    window_seconds: Option<u64>,
) -> BacktestReport {
    let mut report = BacktestReport::default();
    let mut proj_err = (0.0f64, 0usize);
    let mut exh_err = (0.0f64, 0usize);

    // period boundaries: [start, end) index ranges
    let mut bounds: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    for i in 1..samples.len() {
        let (prev, cur) = (&samples[i - 1], &samples[i]);
        let new_reset = matches!((prev.resets_at_ms, cur.resets_at_ms),
            (Some(a), Some(b)) if (a - b).abs() > MINUTE_MS);
        if new_reset || cur.used_percent < prev.used_percent - RESET_DROP {
            bounds.push((start, i));
            start = i;
        }
    }
    if !samples.is_empty() {
        bounds.push((start, samples.len()));
    }
    let last_period = bounds.len().saturating_sub(1);

    for (pi, &(from, to)) in bounds.iter().enumerate() {
        let period = &samples[from..to];
        let Some(resets_at) = period.iter().rev().find_map(|s| s.resets_at_ms) else {
            continue;
        };
        report.periods += 1;
        let hit_ts = period
            .iter()
            .find(|s| s.used_percent >= 100.0)
            .map(|s| s.ts_ms);
        if hit_ts.is_some() {
            report.periods_hit_limit += 1;
        }
        if pi == last_period && hit_ts.is_none() {
            continue; // still running: its outcome is not known yet
        }
        let final_percent = period
            .iter()
            .map(|s| s.used_percent)
            .fold(0.0f64, f64::max)
            .min(100.0);

        for (offset, s) in period.iter().enumerate() {
            if s.used_percent >= 100.0 {
                break;
            }
            let spec = WindowSpec {
                kind,
                window_seconds,
                used_percent: s.used_percent,
                resets_at_ms: Some(resets_at),
            };
            report.points += 1;
            let history = &samples[..from + offset + 1];
            let token_slice: Vec<TokenEvent> = match (tokens, period_start_ms(&spec)) {
                (Some(all), Some(since)) => all
                    .iter()
                    .copied()
                    .filter(|e| e.ts_ms >= since && e.ts_ms <= s.ts_ms)
                    .collect(),
                _ => Vec::new(),
            };
            let by_tokens = tokens.is_some() && !token_slice.is_empty();
            let by_percent = forecast(history, &spec, s.ts_ms);
            let f = if by_percent.is_some() || !by_tokens {
                by_percent.clone()
            } else {
                forecast_with_tokens(history, &spec, s.ts_ms, Some(&token_slice))
            };
            let Some(f) = f else {
                if hit_ts.is_some() {
                    report.missed_points += 1;
                }
                continue;
            };
            report.predicted_points += 1;
            if by_percent.is_none() {
                report.token_points += 1;
            }
            proj_err.0 += (f.projected_percent_at_reset.min(100.0) - final_percent).abs();
            proj_err.1 += 1;
            match (f.exhausts_at.as_deref().and_then(parse_ms), hit_ts) {
                (Some(predicted), Some(actual)) => {
                    exh_err.0 += (predicted - actual).abs() as f64 / MINUTE_MS as f64;
                    exh_err.1 += 1;
                }
                (Some(_), None) => report.false_alarm_points += 1,
                (None, Some(_)) => report.missed_points += 1,
                (None, None) => {}
            }
        }
    }
    report.projected_mae_points = (proj_err.1 > 0).then(|| proj_err.0 / proj_err.1 as f64);
    report.exhaust_mae_minutes = (exh_err.1 > 0).then(|| exh_err.0 / exh_err.1 as f64);
    report
}

// ---------- snapshot integration ----------

/// Fill `forecast` on every window of `snapshot` from the stored samples.
///
/// Called by the scheduler on the blocking pool right after the new samples
/// were written, so the newest value is already in the database. Providers that
/// are not reporting live data are left alone: their windows are last-known
/// values, and extrapolating those would invent usage.
pub fn attach(db: &Db, snapshot: &mut AppSnapshot, now_ms: i64) {
    use crate::commands::store::quota::{window_samples_many, WindowQuery};
    // One pass to collect what to load, one query round under a single lock,
    // one pass to write the forecasts back.
    let mut targets: Vec<(usize, usize, WindowSpec)> = Vec::new();
    let mut queries: Vec<WindowQuery<'_>> = Vec::new();
    let keys: Vec<String> = snapshot.providers.iter().map(|p| p.key()).collect();
    for (pi, provider) in snapshot.providers.iter().enumerate() {
        if provider.status != ProviderStatus::Ok {
            continue;
        }
        for (wi, w) in provider.windows.iter().enumerate() {
            let spec = WindowSpec {
                kind: w.kind,
                window_seconds: w.window_seconds,
                used_percent: w.used_percent,
                resets_at_ms: w.resets_at.as_deref().and_then(parse_ms),
            };
            queries.push(WindowQuery {
                provider: &keys[pi],
                kind: w.kind,
                scope: w.scope.as_deref(),
                since: now_ms - horizon_ms(spec.kind, spec.window_seconds),
            });
            targets.push((pi, wi, spec));
        }
    }
    if targets.is_empty() {
        return;
    }
    let loaded = window_samples_many(db, &queries);
    drop(queries);
    match loaded {
        Ok(all) => {
            for ((pi, wi, spec), samples) in targets.into_iter().zip(all) {
                let provider = &snapshot.providers[pi];
                let scoped = provider.windows[wi].scope.is_some();
                let mut f = forecast(&samples, &spec, now_ms);
                // The percentages had nothing to say: try the token history of
                // the current period (never for a model-scoped window — the
                // provider's tokens are not that scope's tokens).
                // Extra accounts have no ingested token events: the provider's
                // own would belong to the primary account.
                if f.is_none() && !scoped && provider.account_id.is_none() {
                    if let Some(since) = period_start_ms(&spec) {
                        match store::usage_token_events(db, &provider.provider, since) {
                            Ok(events) if !events.is_empty() => {
                                f = forecast_with_tokens(&samples, &spec, now_ms, Some(&events));
                            }
                            Ok(_) => {}
                            Err(e) => log::debug!("forecast: cannot read token events: {e:#}"),
                        }
                    }
                }
                snapshot.providers[pi].windows[wi].forecast = f;
            }
        }
        Err(e) => log::debug!("forecast: cannot read quota samples: {e:#}"),
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

    // ---------- token-based fallback ----------

    fn ev(minutes_ago: i64, tokens: i64) -> TokenEvent {
        TokenEvent {
            ts_ms: T0 - minutes_ago * MINUTE_MS,
            tokens,
        }
    }

    /// 20 % used and flat for the whole horizon (integer percentages plateau),
    /// 5-hour window with 2 h to go.
    fn plateau() -> (Vec<Sample>, WindowSpec) {
        (
            series(&[20.0; 9], 5 * MINUTE_MS),
            spec(FIVE_HOUR, 20.0, 2 * HOUR_MS),
        )
    }

    #[test]
    fn a_plateau_falls_back_to_the_token_pace() {
        let (samples, spec) = plateau();
        assert!(
            forecast(&samples, &spec, T0).is_none(),
            "percent only: flat"
        );
        // 1.7 M tokens early in the period + 6 x 50 k inside the last 35 min:
        // 2 M tokens for 20 % -> 100 k per point; 300 k in 35 min -> 5.14 %/h.
        let mut events = vec![ev(120, 1_700_000)];
        events.extend([35, 28, 21, 14, 7, 1].map(|m| ev(m, 50_000)));
        let f = forecast_with_tokens(&samples, &spec, T0, Some(&events)).unwrap();
        assert_eq!(f.rate_percent_per_hour, 5.14);
        assert_eq!(f.projected_percent_at_reset, 30.29);
        assert_eq!(f.exhausts_at, None);
        // six events over 35 min would be Medium from percentages; one lower.
        assert_eq!(f.confidence, ForecastConfidence::Low);
    }

    #[test]
    fn percentages_win_when_they_can_answer() {
        let samples = series(&[0.0, 10.0, 20.0, 30.0, 40.0], 5 * MINUTE_MS);
        let spec = spec(FIVE_HOUR, 40.0, HOUR_MS);
        let events = vec![ev(30, 1), ev(20, 1), ev(10, 1)];
        assert_eq!(
            forecast_with_tokens(&samples, &spec, T0, Some(&events)),
            forecast(&samples, &spec, T0)
        );
    }

    #[test]
    fn token_confidence_is_one_step_below_the_percentage_rule() {
        let (samples, spec) = plateau();
        // nine events over 32 min covering > half the horizon: percentages
        // would call that High, tokens say Medium.
        let mut events = vec![ev(120, 1_700_000)];
        events.extend((0..9).map(|i| ev(32 - i * 4, 30_000)));
        let f = forecast_with_tokens(&samples, &spec, T0, Some(&events)).unwrap();
        assert_eq!(f.confidence, ForecastConfidence::Medium);
        // ... and a ratio resting on < 5 % of quota is always Low.
        let samples = series(&[3.0; 9], 5 * MINUTE_MS);
        let low = WindowSpec {
            used_percent: 3.0,
            ..spec
        };
        let mut events = vec![ev(120, 300_000)];
        events.extend((0..9).map(|i| ev(32 - i * 4, 30_000)));
        let f = forecast_with_tokens(&samples, &low, T0, Some(&events)).unwrap();
        assert_eq!(f.confidence, ForecastConfidence::Low);
    }

    #[test]
    fn a_token_burst_after_idle_is_not_over_extrapolated() {
        let (samples, spec) = plateau();
        // three 100 k events in the last 4 minutes, nothing before them but
        // the early bulk: the pace is spread over at least the 10-minute floor.
        let events = vec![
            ev(120, 1_700_000),
            ev(4, 100_000),
            ev(2, 100_000),
            ev(1, 100_000),
        ];
        let f = forecast_with_tokens(&samples, &spec, T0, Some(&events)).unwrap();
        // 300 k / 100 k per point / (10 min = 1/6 h) = 18 %/h, not 45 %/h.
        assert_eq!(f.rate_percent_per_hour, 18.0);
    }

    #[test]
    fn token_history_that_has_gone_quiet_gives_no_forecast() {
        let (samples, spec) = plateau();
        // all activity ended 25 minutes ago (> the 15-minute staleness bound)
        let events = vec![
            ev(120, 1_700_000),
            ev(40, 50_000),
            ev(30, 50_000),
            ev(25, 50_000),
        ];
        assert!(forecast_with_tokens(&samples, &spec, T0, Some(&events)).is_none());
        // fewer than three recent events is a blip
        let events = vec![ev(120, 1_700_000), ev(5, 50_000), ev(2, 50_000)];
        assert!(forecast_with_tokens(&samples, &spec, T0, Some(&events)).is_none());
        // no events at all / no tokens supplied
        assert!(forecast_with_tokens(&samples, &spec, T0, Some(&[])).is_none());
        assert!(forecast_with_tokens(&samples, &spec, T0, None).is_none());
    }

    #[test]
    fn the_ratio_is_taken_from_the_current_period_only() {
        // 80 -> 95 before the reset, then flat at 6 %: the caller hands over
        // only the new period's tokens (500 k for 6 % = 83 k per point).
        let samples = series(
            &[80.0, 90.0, 95.0, 6.0, 6.0, 6.0, 6.0, 6.0, 6.0],
            5 * MINUTE_MS,
        );
        let spec = spec(FIVE_HOUR, 6.0, 2 * HOUR_MS);
        let mut events = vec![ev(25, 200_000)];
        events.extend([20, 10, 2].map(|m| ev(m, 100_000)));
        let f = forecast_with_tokens(&samples, &spec, T0, Some(&events)).unwrap();
        // 500 k / (500 k / 6) / (25 min) = 14.4 %/h
        assert_eq!(f.rate_percent_per_hour, 14.4);
        // too little quota used to trust a ratio at all
        let tiny = WindowSpec {
            used_percent: 1.0,
            ..spec
        };
        let samples = series(&[1.0; 9], 5 * MINUTE_MS);
        assert!(forecast_with_tokens(&samples, &tiny, T0, Some(&events)).is_none());
    }

    // ---------- backtest ----------

    /// Samples every 5 minutes from `start` for `n` steps, `pct(i)` each.
    fn ramp(start: i64, n: i64, resets_at: i64, pct: impl Fn(i64) -> f64) -> Vec<Sample> {
        (0..n)
            .map(|i| Sample {
                ts_ms: start + i * 5 * MINUTE_MS,
                used_percent: pct(i),
                resets_at_ms: Some(resets_at),
            })
            .collect()
    }

    #[test]
    fn the_backtest_scores_run_out_predictions_against_what_happened() {
        let s = T0;
        let five_h = 5 * HOUR_MS;
        // P1: 2 points per step -> 100 % after 250 min (a real run-out).
        let mut samples = ramp(s, 51, s + five_h, |i| (2.0 * i as f64).min(100.0));
        // P2: 1 point per step, 59 % at the end - never close to the limit.
        samples.extend(ramp(s + five_h, 60, s + 2 * five_h, |i| i as f64));
        // P3: still running - must not be scored.
        samples.extend(ramp(s + 2 * five_h, 20, s + 3 * five_h, |i| i as f64));
        let r = backtest(&samples, None, WindowKind::FiveHour, Some(FIVE_HOUR));
        assert_eq!((r.periods, r.periods_hit_limit), (3, 1));
        assert!(r.predicted_points > 10, "{r:?}");
        let exhaust = r.exhaust_mae_minutes.expect("P1 ran out");
        assert!(
            exhaust < 2.0,
            "a straight ramp is predicted to the minute, got {exhaust}"
        );
        assert!(r.projected_mae_points.unwrap() < 5.0, "{r:?}");
        // P2 burns 12 %/h: it ends near 59 %, so never a run-out alarm.
        assert_eq!(r.false_alarm_points, 0, "{r:?}");
        assert_eq!(r.token_points, 0);
    }

    #[test]
    fn the_backtest_counts_token_fallback_points_on_a_plateau() {
        let s = T0;
        let five_h = 5 * HOUR_MS;
        // +1 point per 30 min: whole-percent plateaus the slope cannot see.
        let mut samples = ramp(s, 55, s + five_h, |i| 10.0 + (i / 6) as f64);
        samples.extend(ramp(s + five_h, 3, s + 2 * five_h, |i| i as f64));
        let events: Vec<TokenEvent> = (0..55)
            .map(|i| TokenEvent {
                ts_ms: s + i * 5 * MINUTE_MS,
                tokens: 10_000,
            })
            .collect();
        let without = backtest(&samples, None, WindowKind::FiveHour, Some(FIVE_HOUR));
        let with = backtest(
            &samples,
            Some(&events),
            WindowKind::FiveHour,
            Some(FIVE_HOUR),
        );
        assert_eq!(without.token_points, 0);
        assert!(with.token_points > 0, "{with:?}");
        assert!(with.predicted_points > without.predicted_points);
    }

    #[test]
    fn the_backtest_of_an_empty_history_is_empty() {
        assert_eq!(
            backtest(&[], None, WindowKind::FiveHour, Some(FIVE_HOUR)),
            BacktestReport::default()
        );
    }
}
