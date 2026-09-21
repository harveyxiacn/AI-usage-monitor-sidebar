// Presentation rules for QuotaWindow.forecast. [FRONTEND]
//
// Deliberately i18n-free (like history.ts) so `tests/forecast.unit.ts` can
// import it in plain Node; `format.ts` turns the descriptor below into the
// localized line and `Ring.svelte` consumes the tick position.
import type { PercentMode, QuotaWindow } from './types';

const MINUTE = 60_000;

/** i18n keys the forecast line can use; all live under `forecast.*`. */
export type ForecastKey =
  | 'forecast.runsOutMin'
  | 'forecast.runsOutHourMin'
  | 'forecast.paceUsed'
  | 'forecast.paceLeft';

export interface ForecastLine {
  key: ForecastKey;
  params: Record<string, string | number>;
  /** 'warn' = the window is on pace to hit the limit; 'muted' = just a heads-up */
  tone: 'warn' | 'muted';
}

/**
 * One compact line per window, or null when the backend sent no forecast.
 *
 * Running out before the reset is the urgent case and gets the countdown;
 * anything else states where the pace lands at the reset, worded to match
 * `percentMode` like every other percentage in the UI.
 */
export function forecastLine(
  w: QuotaWindow,
  mode: PercentMode = 'used',
  now: number = Date.now()
): ForecastLine | null {
  const f = w.forecast;
  if (!f) return null;

  const exhausts = f.exhaustsAt ? Date.parse(f.exhaustsAt) : NaN;
  if (Number.isFinite(exhausts)) {
    const minutes = Math.max(0, Math.round((exhausts - now) / MINUTE));
    return minutes < 60
      ? { key: 'forecast.runsOutMin', params: { m: minutes }, tone: 'warn' }
      : {
          key: 'forecast.runsOutHourMin',
          params: { h: Math.floor(minutes / 60), m: String(minutes % 60).padStart(2, '0') },
          tone: 'warn',
        };
  }

  const projected = Math.round(Math.max(0, f.projectedPercentAtReset));
  // Reaching exactly 100 % at the reset is still worth the warning colour.
  const tone = projected >= 100 ? 'warn' : 'muted';
  return mode === 'remaining'
    ? { key: 'forecast.paceLeft', params: { p: Math.max(0, 100 - projected) }, tone }
    : { key: 'forecast.paceUsed', params: { p: projected }, tone };
}

/**
 * Where to put the projected-at-reset tick on a ring arc, 0..100 — or null
 * when the ring should stay clean: no forecast, a guess we call "low", or a
 * projection that is not ahead of where the arc already is.
 */
export function forecastTickPercent(w: QuotaWindow): number | null {
  const f = w.forecast;
  if (!f || f.confidence === 'low') return null;
  const projected = Math.min(100, Math.max(0, f.projectedPercentAtReset));
  return projected > Math.min(100, Math.max(0, w.usedPercent)) ? projected : null;
}
