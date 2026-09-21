import { test, expect } from '@playwright/test';
import { forecastLine, forecastTickPercent } from '../src/lib/forecast';
import type { QuotaForecast, QuotaWindow } from '../src/lib/types';

const NOW = Date.parse('2026-09-15T00:00:00Z');
const MINUTE = 60_000;

function window(usedPercent: number, forecast?: Partial<QuotaForecast> | null): QuotaWindow {
  return {
    kind: 'five_hour',
    label: '5-hour',
    windowSeconds: 18_000,
    usedPercent,
    resetsAt: new Date(NOW + 2 * 60 * MINUTE).toISOString(),
    scope: null,
    isPrimary: true,
    forecast:
      forecast === undefined || forecast === null
        ? forecast ?? null
        : {
            projectedPercentAtReset: 120,
            exhaustsAt: null,
            ratePercentPerHour: 30,
            confidence: 'high',
            ...forecast,
          },
  };
}

test('a window without a forecast produces no line and no ring tick', () => {
  expect(forecastLine(window(40), 'used', NOW)).toBeNull();
  expect(forecastTickPercent(window(40))).toBeNull();
  expect(forecastLine(window(40, null), 'used', NOW)).toBeNull();
});

test('running out before the reset counts down in the warning tone', () => {
  const soon = window(82, { exhaustsAt: new Date(NOW + 40 * MINUTE).toISOString() });
  expect(forecastLine(soon, 'used', NOW)).toEqual({
    key: 'forecast.runsOutMin',
    params: { m: 40 },
    tone: 'warn',
  });

  // over an hour the line switches to h + zero-padded minutes, like formatReset
  const later = window(50, { exhaustsAt: new Date(NOW + 125 * MINUTE).toISOString() });
  expect(forecastLine(later, 'used', NOW)).toEqual({
    key: 'forecast.runsOutHourMin',
    params: { h: 2, m: '05' },
    tone: 'warn',
  });

  // the countdown follows the clock, and never goes negative
  expect(forecastLine(soon, 'used', NOW + 39 * MINUTE)?.params).toEqual({ m: 1 });
  expect(forecastLine(soon, 'used', NOW + 99 * MINUTE)?.params).toEqual({ m: 0 });

  // percentMode does not change a countdown
  expect(forecastLine(soon, 'remaining', NOW)?.key).toBe('forecast.runsOutMin');
});

test('a projection that stays under the limit states the pace in the chosen mode', () => {
  const w = window(31, { projectedPercentAtReset: 74.2 });
  expect(forecastLine(w, 'used', NOW)).toEqual({
    key: 'forecast.paceUsed',
    params: { p: 74 },
    tone: 'muted',
  });
  expect(forecastLine(w, 'remaining', NOW)).toEqual({
    key: 'forecast.paceLeft',
    params: { p: 26 },
    tone: 'muted',
  });
});

test('landing at or past the limit is a warning even without an exhaustion time', () => {
  // exactly 100 % at the reset: no countdown, but not a neutral statement either
  const exact = window(90, { projectedPercentAtReset: 100 });
  expect(forecastLine(exact, 'used', NOW)).toEqual({
    key: 'forecast.paceUsed',
    params: { p: 100 },
    tone: 'warn',
  });
  // the backend may project past 100 with no exhaustion time inside the window
  const over = window(90, { projectedPercentAtReset: 143.6 });
  expect(forecastLine(over, 'used', NOW)).toEqual({
    key: 'forecast.paceUsed',
    params: { p: 144 },
    tone: 'warn',
  });
  expect(forecastLine(over, 'remaining', NOW)?.params).toEqual({ p: 0 });
});

test('an unparseable exhaustion time falls back to the pace wording', () => {
  const w = window(50, { exhaustsAt: 'not a time', projectedPercentAtReset: 80 });
  expect(forecastLine(w, 'used', NOW)?.key).toBe('forecast.paceUsed');
});

test('the ring tick is capped at 100 and skips low-confidence guesses', () => {
  expect(forecastTickPercent(window(40, { projectedPercentAtReset: 82 }))).toBe(82);
  expect(forecastTickPercent(window(40, { projectedPercentAtReset: 520 }))).toBe(100);
  expect(forecastTickPercent(window(40, { projectedPercentAtReset: 82, confidence: 'medium' }))).toBe(82);
  expect(forecastTickPercent(window(40, { projectedPercentAtReset: 82, confidence: 'low' }))).toBeNull();
  // nothing to mark when the projection is not ahead of the arc itself
  expect(forecastTickPercent(window(82, { projectedPercentAtReset: 82 }))).toBeNull();
  expect(forecastTickPercent(window(100, { projectedPercentAtReset: 120 }))).toBeNull();
});
