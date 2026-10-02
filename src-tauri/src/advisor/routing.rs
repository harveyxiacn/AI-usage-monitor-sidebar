//! Cross-provider routing advice: "Claude is about to run dry, Codex has room".
//! [BACKEND]
//!
//! [`advise`] is a pure function over the current [`AppSnapshot`] (quota
//! windows with their forecasts, see `forecast.rs`) and the settings. It reads
//! nothing else, sends nothing anywhere and stays silent (`None`) whenever the
//! evidence is thin. Everything it says is an *estimate* from the providers'
//! own percentages: one percent of Claude is not one percent of Codex, so the
//! advice names both numbers instead of pretending they are comparable.
//!
//! ## Rules (every threshold is a constant below)
//!
//! A *candidate* is a provider account that is enabled, `status == ok`, fetched
//! no more than [`MAX_AGE_MS`] ago and has at least one account-wide window
//! (no `scope`) with a known reset in the future. Per-model windows are
//! ignored, rate-limited / errored / signed-out / disabled accounts never take
//! part (neither as source nor as target).
//!
//! * **Source (constrained)**: a candidate with an account-wide window that is
//!   already at 100 % (confidence `high`, it is a fact), or whose forecast says
//!   it runs out *before its reset* within [`SOURCE_HORIZON_MS`] (3 h) with a
//!   confidence of at least `medium`. The window with the least time left is the
//!   binding one; the source with the least time left across candidates wins.
//! * **Target**: another candidate of a verified provider (`claude`, `codex`;
//!   Copilot and OpenRouter are experimental and are never recommended) where
//!   *every* account-wide window has at least [`MIN_TARGET_REMAINING`] (30 %)
//!   left and no forecast puts exhaustion earlier than
//!   `max(2 x source time left, 60 min)`. The one with the most headroom in its
//!   binding (least remaining) window wins; ties go to the longer run-out time,
//!   then to the key.
//! * **Confidence** = the lower of the source forecast's confidence and the
//!   lowest forecast confidence on the target's windows (a target with no
//!   forecast adds nothing: its headroom is the provider's own number). A
//!   `low` result is never shown: the function returns `None`.
//! * **Duration** ("for the next ~2 h") = the shorter of the time until the
//!   source resets and the target's projected run-out, capped at 24 h.
//! * **No switch needed** is only said when at least two candidates have data,
//!   none is constrained, and no forecast of `low` confidence points at a
//!   run-out inside the horizon (that would be "not sure", i.e. silence).
//!
//! Nothing here persists anything; the notification dedupe lives in
//! [`claim_notification`].

use std::collections::BTreeMap;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::commands::forecast::parse_ms;
use crate::commands::providers::clamp_percent;
use crate::model::{
    AppSnapshot, ForecastConfidence, ProviderQuota, ProviderStatus, QuotaWindow, Settings,
    WindowKind,
};

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;

/// A quota reading older than this is not a basis for advice.
pub const MAX_AGE_MS: i64 = 30 * MINUTE_MS;
/// A source must run dry within this long to be worth a switch.
pub const SOURCE_HORIZON_MS: i64 = 3 * HOUR_MS;
/// A target needs at least this much left (percent) in each account-wide window.
pub const MIN_TARGET_REMAINING: f64 = 30.0;
/// A target's own projected run-out must be at least this long away ...
pub const MIN_TARGET_SAFE_MS: i64 = HOUR_MS;
/// ... and at least this many times the source's remaining time.
pub const TARGET_SAFE_FACTOR: i64 = 2;
/// "Use X for the next …" never promises more than this.
pub const MAX_DURATION_MS: i64 = 24 * HOUR_MS;

/// Providers whose numbers are verified against a live account.
fn recommendable(provider: &str) -> bool {
    matches!(provider, "claude" | "codex")
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutingKind {
    /// Move the next work to `to`.
    Switch,
    /// Everything has room; says so (Overview card only).
    NoSwitch,
}

/// The binding account-wide window of one candidate, with the numbers the
/// advice is based on.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AdvisorWindow {
    /// `claude` or `claude@work`
    pub key: String,
    pub provider: String,
    pub display_name: String,
    pub kind: WindowKind,
    pub label: String,
    pub used_percent: f64,
    /// headroom until the next reset: `100 - used`
    pub remaining_percent: f64,
    pub resets_in_min: i64,
    /// unix ms of the reset (dedupe key of the notification)
    pub resets_at_ms: i64,
    /// Safe working time at the current pace, in minutes: `0` when the window
    /// is full, the time to the projected 100 % when it runs out before the
    /// reset, the time to the reset when the forecast says it will not, and
    /// `None` when there is no forecast.
    pub safe_minutes: Option<i64>,
    /// the forecast's projection at the reset (percent), when there is one
    pub projected_percent_at_reset: Option<f64>,
    pub confidence: Option<ForecastConfidence>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoutingAdvice {
    pub kind: RoutingKind,
    /// The constrained provider (`switch` only).
    pub from: Option<AdvisorWindow>,
    /// The recommended one (`switch` only).
    pub to: Option<AdvisorWindow>,
    /// "consider `to` for the next ~N minutes" (`switch` only)
    pub use_target_minutes: Option<i64>,
    pub confidence: ForecastConfidence,
    /// The binding window of every candidate that was considered, so the UI
    /// can show the numbers behind the sentence.
    pub basis: Vec<AdvisorWindow>,
}

/// One account-wide window, evaluated.
#[derive(Clone, Debug)]
struct Eval {
    window: AdvisorWindow,
    run_out_ms: Option<i64>,
    resets_in_ms: i64,
}

struct Candidate {
    key: String,
    provider: String,
    /// every evaluated account-wide window
    evals: Vec<Eval>,
}

fn rank(c: ForecastConfidence) -> u8 {
    match c {
        ForecastConfidence::Low => 0,
        ForecastConfidence::Medium => 1,
        ForecastConfidence::High => 2,
    }
}

fn min_conf(a: ForecastConfidence, b: ForecastConfidence) -> ForecastConfidence {
    if rank(a) <= rank(b) {
        a
    } else {
        b
    }
}

fn eval_window(q: &ProviderQuota, w: &QuotaWindow, now_ms: i64) -> Option<Eval> {
    if w.scope.is_some() || !w.used_percent.is_finite() {
        return None;
    }
    let resets_at = parse_ms(w.resets_at.as_deref()?)?;
    let resets_in_ms = resets_at - now_ms;
    if resets_in_ms <= 0 {
        return None; // resetting right now: the reading is about to change
    }
    let used = clamp_percent(w.used_percent);
    let forecast = w.forecast.as_ref();
    let run_out_ms = if used >= 100.0 {
        Some(0)
    } else {
        forecast
            .and_then(|f| f.exhausts_at.as_deref())
            .and_then(parse_ms)
            .map(|at| at - now_ms)
            .filter(|ms| *ms > 0 && *ms < resets_in_ms)
    };
    let safe_ms = if used >= 100.0 {
        Some(0)
    } else if forecast.is_some() {
        Some(run_out_ms.unwrap_or(resets_in_ms))
    } else {
        None
    };
    let confidence = if used >= 100.0 {
        Some(ForecastConfidence::High)
    } else {
        forecast.map(|f| f.confidence)
    };
    Some(Eval {
        window: AdvisorWindow {
            key: q.key(),
            provider: q.provider.clone(),
            display_name: q.display_name.clone(),
            kind: w.kind,
            label: w.label.clone(),
            used_percent: used,
            remaining_percent: 100.0 - used,
            resets_in_min: resets_in_ms / MINUTE_MS,
            resets_at_ms: resets_at,
            safe_minutes: safe_ms.map(|ms| ms / MINUTE_MS),
            projected_percent_at_reset: forecast.map(|f| f.projected_percent_at_reset),
            confidence,
        },
        run_out_ms,
        resets_in_ms,
    })
}

fn candidates(snapshot: &AppSnapshot, settings: &Settings, now_ms: i64) -> Vec<Candidate> {
    let mut out = Vec::new();
    for q in &snapshot.providers {
        if q.status != ProviderStatus::Ok {
            continue;
        }
        if !settings
            .providers
            .get(&q.provider)
            .is_some_and(|p| p.enabled)
        {
            continue;
        }
        match parse_ms(&q.fetched_at) {
            Some(at) if now_ms - at <= MAX_AGE_MS && at <= now_ms + MINUTE_MS => {}
            _ => continue,
        }
        let evals: Vec<Eval> = q
            .windows
            .iter()
            .filter_map(|w| eval_window(q, w, now_ms))
            .collect();
        if evals.is_empty() {
            continue;
        }
        out.push(Candidate {
            key: q.key(),
            provider: q.provider.clone(),
            evals,
        });
    }
    out
}

/// The window that limits a candidate as a *source*: full, or confidently
/// projected to run out soon. `None` when the candidate is not constrained.
fn source_window(c: &Candidate) -> Option<(&Eval, ForecastConfidence)> {
    c.evals
        .iter()
        .filter_map(|e| {
            let run_out = e.run_out_ms?;
            let conf = e.window.confidence?;
            let full = e.window.used_percent >= 100.0;
            if !full
                && (rank(conf) < rank(ForecastConfidence::Medium) || run_out > SOURCE_HORIZON_MS)
            {
                return None;
            }
            Some((e, conf))
        })
        .min_by_key(|(e, _)| e.run_out_ms)
}

/// A forecast of `low` confidence that points at a run-out inside the horizon:
/// we cannot tell whether a switch is needed, so we say nothing.
fn unsure_about(c: &Candidate) -> bool {
    c.evals.iter().any(|e| {
        e.run_out_ms.is_some_and(|ms| ms <= SOURCE_HORIZON_MS)
            && e.window.confidence == Some(ForecastConfidence::Low)
    })
}

/// The window that limits a candidate as a *target*: the one with the least
/// headroom.
fn binding_window(c: &Candidate) -> &Eval {
    c.evals
        .iter()
        .min_by(|a, b| {
            a.window
                .remaining_percent
                .total_cmp(&b.window.remaining_percent)
                .then_with(|| a.resets_in_ms.cmp(&b.resets_in_ms))
        })
        .expect("a candidate has at least one window")
}

/// Is `c` a usable target for a source with `source_left_ms` to go?
/// Returns its binding window, the earliest projected run-out over all its
/// windows and the lowest forecast confidence among them.
fn target_fit(
    c: &Candidate,
    source_left_ms: i64,
) -> Option<(&Eval, Option<i64>, Option<ForecastConfidence>)> {
    if !recommendable(&c.provider) {
        return None;
    }
    if c.evals
        .iter()
        .any(|e| e.window.remaining_percent < MIN_TARGET_REMAINING)
    {
        return None;
    }
    let need = (source_left_ms * TARGET_SAFE_FACTOR).max(MIN_TARGET_SAFE_MS);
    let run_out = c.evals.iter().filter_map(|e| e.run_out_ms).min();
    if run_out.is_some_and(|ms| ms < need) {
        return None;
    }
    let conf = c
        .evals
        .iter()
        .filter_map(|e| e.window.confidence)
        .reduce(min_conf);
    Some((binding_window(c), run_out, conf))
}

/// The one recommendation (or "no switch needed") for this snapshot; `None`
/// when there is nothing worth saying or too little data to say it.
pub fn advise(snapshot: &AppSnapshot, settings: &Settings, now_ms: i64) -> Option<RoutingAdvice> {
    let cands = candidates(snapshot, settings, now_ms);
    if cands.is_empty() {
        return None;
    }
    let basis: Vec<AdvisorWindow> = cands
        .iter()
        .map(|c| binding_window(c).window.clone())
        .collect();

    // the most pressed source
    let source = cands
        .iter()
        .filter_map(|c| source_window(c).map(|(e, conf)| (c, e, conf)))
        .min_by_key(|(c, e, _)| (e.run_out_ms, c.key.clone()));

    let Some((src_cand, src_eval, src_conf)) = source else {
        // nobody is constrained
        if cands.len() < 2 || cands.iter().any(unsure_about) {
            return None;
        }
        let confidence = cands
            .iter()
            .flat_map(|c| c.evals.iter().filter_map(|e| e.window.confidence))
            .reduce(min_conf)
            .unwrap_or(ForecastConfidence::Medium);
        return Some(RoutingAdvice {
            kind: RoutingKind::NoSwitch,
            from: None,
            to: None,
            use_target_minutes: None,
            confidence,
            basis,
        });
    };

    let source_left_ms = src_eval.run_out_ms.unwrap_or(0);
    let mut best: Option<(&Candidate, &Eval, Option<i64>, Option<ForecastConfidence>)> = None;
    for c in cands.iter().filter(|c| c.key != src_cand.key) {
        // another candidate that is itself running dry is no refuge
        if source_window(c).is_some() {
            continue;
        }
        let Some((win, run_out, conf)) = target_fit(c, source_left_ms) else {
            continue;
        };
        let better = match &best {
            None => true,
            Some((bc, bw, br, _)) => {
                let a = (
                    win.window.remaining_percent,
                    run_out.unwrap_or(i64::MAX),
                    std::cmp::Reverse(&c.key),
                );
                let b = (
                    bw.window.remaining_percent,
                    br.unwrap_or(i64::MAX),
                    std::cmp::Reverse(&bc.key),
                );
                a.0.total_cmp(&b.0)
                    .then(a.1.cmp(&b.1))
                    .then(a.2.cmp(&b.2))
                    .is_gt()
            }
        };
        if better {
            best = Some((c, win, run_out, conf));
        }
    }
    let (_, target, target_run_out, target_conf) = best?;

    let confidence = match target_conf {
        Some(t) => min_conf(src_conf, t),
        None => src_conf,
    };
    if confidence == ForecastConfidence::Low {
        return None;
    }
    let duration_ms = src_eval
        .resets_in_ms
        .min(target_run_out.unwrap_or(i64::MAX))
        .min(MAX_DURATION_MS);
    Some(RoutingAdvice {
        kind: RoutingKind::Switch,
        from: Some(src_eval.window.clone()),
        to: Some(target.window.clone()),
        use_target_minutes: Some(duration_ms / MINUTE_MS),
        confidence,
        basis,
    })
}

// ---------- notification ----------

/// `from|to` → the reset (unix ms) of the source window already announced.
static ANNOUNCED: Mutex<BTreeMap<String, i64>> = Mutex::new(BTreeMap::new());

/// `true` the first time this recommendation is seen within its source's
/// reset period, so the same advice never repeats every refresh. Entries are
/// forgotten once their reset has passed.
pub fn claim_notification(advice: &RoutingAdvice, now_ms: i64) -> bool {
    claim_in(&mut ANNOUNCED.lock(), advice, now_ms)
}

fn claim_in(seen: &mut BTreeMap<String, i64>, advice: &RoutingAdvice, now_ms: i64) -> bool {
    let (Some(from), Some(to)) = (&advice.from, &advice.to) else {
        return false;
    };
    seen.retain(|_, resets_at| *resets_at > now_ms);
    let key = format!("{}|{}", from.key, to.key);
    if seen.get(&key) == Some(&from.resets_at_ms) {
        return false;
    }
    seen.insert(key, from.resets_at_ms);
    true
}

/// "25 min", "1 h 30 min", "2 d 3 h": coarse on purpose.
pub fn format_minutes(minutes: i64, chinese: bool) -> String {
    let m = minutes.max(1);
    let (d, h, mm) = (m / 1440, (m % 1440) / 60, m % 60);
    match (chinese, m) {
        (false, m) if m < 60 => format!("{m} min"),
        (false, m) if m < 2880 => {
            if mm == 0 {
                format!("{} h", m / 60)
            } else {
                format!("{} h {mm} min", m / 60)
            }
        }
        (false, _) => format!("{d} d {h} h"),
        (true, m) if m < 60 => format!("{m} 分钟"),
        (true, m) if m < 2880 => {
            if mm == 0 {
                format!("{} 小时", m / 60)
            } else {
                format!("{} 小时 {mm} 分钟", m / 60)
            }
        }
        (true, _) => format!("{d} 天 {h} 小时"),
    }
}

fn window_text(w: &AdvisorWindow, chinese: bool) -> String {
    match (w.kind, chinese) {
        (WindowKind::FiveHour, false) => "5-hour".into(),
        (WindowKind::FiveHour, true) => "5 小时".into(),
        (WindowKind::SevenDay, false) => "weekly".into(),
        (WindowKind::SevenDay, true) => "每周".into(),
        (WindowKind::Other, _) => w.label.clone(),
    }
}

/// Notification title + body of a `switch` recommendation, in one language.
pub fn alert_text(advice: &RoutingAdvice, chinese: bool) -> Option<(String, String)> {
    let (from, to) = (advice.from.as_ref()?, advice.to.as_ref()?);
    let used = from.used_percent.round() as i64;
    let left = to.remaining_percent.round() as i64;
    let use_for = format_minutes(advice.use_target_minutes.unwrap_or(0), chinese);
    let pace = if from.used_percent >= 100.0 {
        None
    } else {
        from.safe_minutes.map(|m| format_minutes(m, chinese))
    };
    Some(if chinese {
        let state = match pace {
            Some(p) => format!("按当前速度约 {p}后用尽"),
            None => format!(
                "已用尽（约 {}后重置）",
                format_minutes(from.resets_in_min, true)
            ),
        };
        (
            format!("考虑改用 {}", to.display_name),
            format!(
                "{} {}限额已用 {used}%，{state}；{} {}还剩 {left}%，可先用约 {use_for}（估算）",
                from.display_name,
                window_text(from, true),
                to.display_name,
                window_text(to, true),
            ),
        )
    } else {
        let state = match pace {
            Some(p) => format!("on pace to run out in ~{p}"),
            None => format!(
                "full (resets in ~{})",
                format_minutes(from.resets_in_min, false)
            ),
        };
        (
            format!("Consider {} for now", to.display_name),
            format!(
                "{} {} is at {used}% and {state}; {} has {left}% of its {} left — about {use_for} of room (estimate)",
                from.display_name,
                window_text(from, false),
                to.display_name,
                window_text(to, false),
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::QuotaForecast;

    const NOW: i64 = 1_789_430_400_000; // 2026-09-15T00:00:00Z

    fn iso(ms: i64) -> String {
        crate::commands::providers::rfc3339_from_unix_ms(ms).unwrap()
    }

    fn window(
        kind: WindowKind,
        used: f64,
        resets_in_min: i64,
        forecast: Option<(Option<i64>, ForecastConfidence)>,
    ) -> QuotaWindow {
        QuotaWindow {
            kind,
            label: if kind == WindowKind::FiveHour {
                "5-hour"
            } else {
                "Weekly"
            }
            .into(),
            window_seconds: Some(if kind == WindowKind::FiveHour {
                18_000
            } else {
                604_800
            }),
            used_percent: used,
            resets_at: Some(iso(NOW + resets_in_min * MINUTE_MS)),
            scope: None,
            is_primary: kind == WindowKind::FiveHour,
            forecast: forecast.map(|(exhausts_in_min, confidence)| QuotaForecast {
                projected_percent_at_reset: if exhausts_in_min.is_some() {
                    150.0
                } else {
                    used
                },
                exhausts_at: exhausts_in_min.map(|m| iso(NOW + m * MINUTE_MS)),
                rate_percent_per_hour: 20.0,
                confidence,
            }),
        }
    }

    fn quota(provider: &str, windows: Vec<QuotaWindow>) -> ProviderQuota {
        let mut q = crate::commands::providers::empty_quota(
            provider,
            if provider == "claude" {
                "Claude"
            } else {
                "Codex"
            },
            ProviderStatus::Ok,
        );
        q.fetched_at = iso(NOW - MINUTE_MS);
        q.windows = windows;
        q
    }

    fn snap(providers: Vec<ProviderQuota>) -> AppSnapshot {
        AppSnapshot {
            providers,
            ..AppSnapshot::default()
        }
    }

    fn claude_hot() -> ProviderQuota {
        quota(
            "claude",
            vec![window(
                WindowKind::FiveHour,
                92.0,
                120,
                Some((Some(25), ForecastConfidence::High)),
            )],
        )
    }

    fn codex_roomy() -> ProviderQuota {
        quota(
            "codex",
            vec![
                window(WindowKind::FiveHour, 10.0, 200, None),
                window(WindowKind::SevenDay, 30.0, 5_000, None),
            ],
        )
    }

    fn advise_now(providers: Vec<ProviderQuota>) -> Option<RoutingAdvice> {
        advise(&snap(providers), &Settings::default(), NOW)
    }

    #[test]
    fn a_confident_run_out_with_a_roomy_alternative_recommends_a_switch() {
        let a = advise_now(vec![claude_hot(), codex_roomy()]).expect("advice");
        assert_eq!(a.kind, RoutingKind::Switch);
        let (from, to) = (a.from.as_ref().unwrap(), a.to.as_ref().unwrap());
        assert_eq!(from.key, "claude");
        assert_eq!(from.safe_minutes, Some(25));
        assert_eq!(from.remaining_percent, 8.0);
        assert_eq!(to.key, "codex");
        // codex's least-headroom window is the weekly one
        assert_eq!(to.kind, WindowKind::SevenDay);
        assert_eq!(to.remaining_percent, 70.0);
        // the source resets in 120 min; the target has no projected run-out
        assert_eq!(a.use_target_minutes, Some(120));
        assert_eq!(a.confidence, ForecastConfidence::High);
        assert_eq!(a.basis.len(), 2);
    }

    #[test]
    fn the_duration_is_capped_by_the_targets_own_run_out_and_by_24_hours() {
        let mut codex = codex_roomy();
        codex.windows[1].forecast = Some(QuotaForecast {
            projected_percent_at_reset: 120.0,
            exhausts_at: Some(iso(NOW + 200 * MINUTE_MS)),
            rate_percent_per_hour: 10.0,
            confidence: ForecastConfidence::Medium,
        });
        // the source resets in 4 h, the target (not itself in trouble: its
        // run-out is beyond the 3 h horizon) lasts 200 min
        let mut hot = claude_hot();
        hot.windows[0].resets_at = Some(iso(NOW + 240 * MINUTE_MS));
        let a = advise_now(vec![hot, codex]).unwrap();
        assert_eq!(a.use_target_minutes, Some(200));
        assert_eq!(a.confidence, ForecastConfidence::Medium, "lowest of both");

        // a source that is full for days is advised only for 24 h at a time
        let full = quota(
            "claude",
            vec![window(WindowKind::SevenDay, 100.0, 4 * 1440, None)],
        );
        let a = advise_now(vec![full, codex_roomy()]).unwrap();
        assert_eq!(a.use_target_minutes, Some(1440));
        assert_eq!(
            a.confidence,
            ForecastConfidence::High,
            "a full window is a fact"
        );
        assert_eq!(a.from.unwrap().safe_minutes, Some(0));
    }

    #[test]
    fn a_target_with_too_little_room_is_never_recommended() {
        // weekly nearly gone even though the 5-hour is fresh
        let tight = quota(
            "codex",
            vec![
                window(WindowKind::FiveHour, 5.0, 200, None),
                window(WindowKind::SevenDay, 75.0, 5_000, None),
            ],
        );
        assert_eq!(advise_now(vec![claude_hot(), tight]), None);
        // roomy, but itself on pace to run out soon: no refuge
        let mut shaky = codex_roomy();
        shaky.windows[0].forecast = Some(QuotaForecast {
            projected_percent_at_reset: 200.0,
            exhausts_at: Some(iso(NOW + 40 * MINUTE_MS)),
            rate_percent_per_hour: 30.0,
            confidence: ForecastConfidence::High,
        });
        assert_eq!(advise_now(vec![claude_hot(), shaky]), None);
    }

    #[test]
    fn disabled_errored_stale_and_unsigned_providers_are_never_involved() {
        for status in [
            ProviderStatus::NotLoggedIn,
            ProviderStatus::TokenExpired,
            ProviderStatus::RateLimited,
            ProviderStatus::Error,
            ProviderStatus::Disabled,
        ] {
            let mut codex = codex_roomy();
            codex.status = status;
            assert_eq!(advise_now(vec![claude_hot(), codex]), None, "{status:?}");
        }
        // disabled in the settings
        let mut s = Settings::default();
        s.providers.get_mut("codex").unwrap().enabled = false;
        assert_eq!(
            advise(&snap(vec![claude_hot(), codex_roomy()]), &s, NOW),
            None
        );
        // an old reading
        let mut codex = codex_roomy();
        codex.fetched_at = iso(NOW - 2 * HOUR_MS);
        assert_eq!(advise_now(vec![claude_hot(), codex]), None);
        // an errored source cannot be advised about either
        let mut hot = claude_hot();
        hot.status = ProviderStatus::Error;
        assert_eq!(advise_now(vec![hot, codex_roomy()]), None);
    }

    #[test]
    fn experimental_providers_are_not_recommended() {
        let mut s = Settings::default();
        s.providers.get_mut("copilot").unwrap().enabled = true;
        let mut other = codex_roomy();
        other.provider = "copilot".into();
        assert_eq!(advise(&snap(vec![claude_hot(), other]), &s, NOW), None);
    }

    #[test]
    fn a_low_confidence_forecast_never_triggers_a_switch() {
        let mut hot = claude_hot();
        hot.windows[0].forecast.as_mut().unwrap().confidence = ForecastConfidence::Low;
        assert_eq!(advise_now(vec![hot, codex_roomy()]), None);
        // and a medium source with a low-confidence target forecast is low overall
        let mut codex = codex_roomy();
        codex.windows[0].forecast = Some(QuotaForecast {
            projected_percent_at_reset: 20.0,
            exhausts_at: None,
            rate_percent_per_hour: 1.0,
            confidence: ForecastConfidence::Low,
        });
        assert_eq!(advise_now(vec![claude_hot(), codex]), None);
    }

    #[test]
    fn run_outs_beyond_the_horizon_or_after_the_reset_are_not_constrained() {
        // runs out in 4 h (> 3 h horizon): no switch, and both are fine
        let slow = quota(
            "claude",
            vec![window(
                WindowKind::FiveHour,
                60.0,
                600,
                Some((Some(240), ForecastConfidence::High)),
            )],
        );
        let a = advise_now(vec![slow, codex_roomy()]).unwrap();
        assert_eq!(a.kind, RoutingKind::NoSwitch);
        // a forecast that never reaches 100 % before the reset
        let calm = quota(
            "claude",
            vec![window(
                WindowKind::FiveHour,
                50.0,
                120,
                Some((None, ForecastConfidence::High)),
            )],
        );
        let a = advise_now(vec![calm, codex_roomy()]).unwrap();
        assert_eq!(a.kind, RoutingKind::NoSwitch);
        assert_eq!(a.confidence, ForecastConfidence::High);
        assert!(a.from.is_none() && a.to.is_none());
    }

    #[test]
    fn no_switch_needs_two_candidates_and_no_unsure_forecast() {
        assert_eq!(
            advise_now(vec![codex_roomy()]),
            None,
            "one provider: nothing to compare"
        );
        assert_eq!(advise_now(vec![]), None);
        let mut unsure = claude_hot();
        unsure.windows[0].forecast.as_mut().unwrap().confidence = ForecastConfidence::Low;
        // a hot, low-confidence reading means "not sure", not "no switch needed"
        assert_eq!(advise_now(vec![unsure, codex_roomy()]), None);
    }

    #[test]
    fn scoped_windows_and_resetting_windows_are_ignored() {
        let mut hot = claude_hot();
        hot.windows[0].scope = Some("Fable".into());
        // only a scoped window: the account has no candidate window at all
        assert_eq!(advise_now(vec![hot, codex_roomy()]), None);
        let mut resetting = claude_hot();
        resetting.windows[0].resets_at = Some(iso(NOW - 1_000));
        assert_eq!(advise_now(vec![resetting, codex_roomy()]), None);
    }

    #[test]
    fn the_best_target_has_the_most_headroom_and_extra_accounts_count() {
        let mut work = quota(
            "claude",
            vec![window(WindowKind::FiveHour, 20.0, 200, None)],
        );
        work.account_id = Some("work".into());
        work.display_name = "Claude · Work".into();
        let a = advise_now(vec![claude_hot(), codex_roomy(), work]).unwrap();
        // work has 80 % left, codex's weekly only 70 %
        assert_eq!(a.to.unwrap().key, "claude@work");
        // the same-provider account is a real alternative; the source is not
        assert_eq!(a.from.unwrap().key, "claude");
    }

    #[test]
    fn two_constrained_providers_leave_no_refuge() {
        let hot_codex = quota(
            "codex",
            vec![window(
                WindowKind::FiveHour,
                95.0,
                100,
                Some((Some(15), ForecastConfidence::High)),
            )],
        );
        assert_eq!(advise_now(vec![claude_hot(), hot_codex]), None);
    }

    #[test]
    fn notifications_are_claimed_once_per_reset_period() {
        let a = advise_now(vec![claude_hot(), codex_roomy()]).unwrap();
        let mut seen = BTreeMap::new();
        assert!(claim_in(&mut seen, &a, NOW));
        assert!(!claim_in(&mut seen, &a, NOW + MINUTE_MS));
        // the next reset period of the source announces again
        let mut later = a.clone();
        later.from.as_mut().unwrap().resets_at_ms += 5 * HOUR_MS;
        assert!(claim_in(&mut seen, &later, NOW + 3 * HOUR_MS));
        // a no-switch advice is never claimed
        let none = RoutingAdvice {
            kind: RoutingKind::NoSwitch,
            from: None,
            to: None,
            use_target_minutes: None,
            confidence: ForecastConfidence::High,
            basis: vec![],
        };
        assert!(!claim_in(&mut seen, &none, NOW));
    }

    #[test]
    fn texts_state_the_numbers_in_both_languages() {
        let a = advise_now(vec![claude_hot(), codex_roomy()]).unwrap();
        let (title, body) = alert_text(&a, false).unwrap();
        assert_eq!(title, "Consider Codex for now");
        assert!(body.contains("Claude 5-hour is at 92%"), "{body}");
        assert!(body.contains("run out in ~25 min"), "{body}");
        assert!(body.contains("Codex has 70% of its weekly left"), "{body}");
        assert!(body.contains("2 h"), "{body}");
        assert!(body.contains("estimate"));
        let (title, body) = alert_text(&a, true).unwrap();
        assert!(title.contains("Codex"));
        assert!(
            body.contains("92%") && body.contains("70%") && body.contains("估算"),
            "{body}"
        );
        let full = quota(
            "claude",
            vec![window(WindowKind::FiveHour, 100.0, 90, None)],
        );
        let a = advise_now(vec![full, codex_roomy()]).unwrap();
        assert!(alert_text(&a, false)
            .unwrap()
            .1
            .contains("full (resets in ~1 h 30 min)"));
    }

    #[test]
    fn minutes_format_coarsely() {
        assert_eq!(format_minutes(0, false), "1 min");
        assert_eq!(format_minutes(25, false), "25 min");
        assert_eq!(format_minutes(120, false), "2 h");
        assert_eq!(format_minutes(135, false), "2 h 15 min");
        assert_eq!(format_minutes(3000, false), "2 d 2 h");
        assert_eq!(format_minutes(135, true), "2 小时 15 分钟");
    }
}
