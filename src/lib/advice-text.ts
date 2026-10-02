// Wording of the routing advice. The decision itself is made in the backend
// (src-tauri/src/advisor/routing.rs); the numbers come with it, this file only
// puts them into a sentence. [FRONTEND]
import { durationParts } from './advice';
import { t, tDyn } from './i18n/i18n.svelte';
import type { AdvisorWindow, ForecastConfidence } from './types';
import type { SwitchAdvice } from './advice';

/** "25 min", "2 h", "1 h 30 min", "2 d 6 h" — already rounded to an "about" figure. */
export function adviceDuration(minutes: number): string {
  const { days, hours, minutes: m } = durationParts(minutes);
  if (days > 0) return hours > 0 ? t('advisor.dur.dh', { d: days, h: hours }) : t('advisor.dur.d', { d: days });
  if (hours > 0) return m > 0 ? t('advisor.dur.hm', { h: hours, m }) : t('advisor.dur.h', { h: hours });
  return t('advisor.dur.min', { n: Math.max(5, m) });
}

export function adviceWindowName(w: AdvisorWindow): string {
  if (w.kind === 'five_hour') return t('window.fiveHour');
  if (w.kind === 'seven_day') return t('window.weekly');
  return w.label;
}

export function confidenceName(level: ForecastConfidence): string {
  return tDyn(`advisor.confidence.${level}`);
}

function pace(from: AdvisorWindow): string {
  return from.usedPercent >= 100 || from.safeMinutes === null
    ? t('advisor.routing.pace.full', { time: adviceDuration(from.resetsInMin) })
    : t('advisor.routing.pace.runsOut', { time: adviceDuration(from.safeMinutes) });
}

/** The full sentence of the Overview card. */
export function routingSentence(a: SwitchAdvice): string {
  return t('advisor.routing.switch', {
    from: a.from.displayName,
    fromWindow: adviceWindowName(a.from),
    used: Math.round(a.from.usedPercent),
    pace: pace(a.from),
    to: a.to.displayName,
    left: Math.round(a.to.remainingPercent),
    toWindow: adviceWindowName(a.to),
    duration: adviceDuration(a.useTargetMinutes ?? a.from.resetsInMin),
  });
}

/** The one-liner of the popover footer. */
export function routingHint(a: SwitchAdvice): string {
  return t('advisor.routing.hint', {
    to: a.to.displayName,
    duration: adviceDuration(a.useTargetMinutes ?? a.from.resetsInMin),
    from: a.from.displayName,
    fromWindow: adviceWindowName(a.from),
    used: Math.round(a.from.usedPercent),
  });
}

/** One row of the "Basis" list: the binding window of a considered provider. */
export function basisRow(w: AdvisorWindow): string {
  return t('advisor.routing.basisRow', {
    provider: w.displayName,
    window: adviceWindowName(w),
    used: Math.round(w.usedPercent),
    left: Math.round(w.remainingPercent),
    reset: adviceDuration(w.resetsInMin),
    safe: w.safeMinutes !== null && w.usedPercent < 100
      ? t('advisor.routing.basisSafe', { time: adviceDuration(w.safeMinutes) })
      : '',
  });
}
