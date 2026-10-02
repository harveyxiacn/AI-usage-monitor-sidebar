import { expect, test } from '@playwright/test';
import { buildCycles, forecastSegment, limitReachedCount, tokensPerPercent, windowLengthMs, withResetGaps, type CycleUsage } from '../src/lib/quota-cycles';
import { buildQuotaHistory } from '../src/lib/quota-history';
import { planPriceHint, projectToMonthEnd, subscriptionRoi, subscriptionTotal } from '../src/lib/subscription';
import type { QuotaSample, TokenTotals } from '../src/lib/types';

const H = 3_600_000;
const T0 = Date.UTC(2026, 8, 20, 0, 0, 0);
const iso = (ms: number) => new Date(ms).toISOString();

function sample(ts: number, usedPercent: number, resetsAt: number | null, overrides: Partial<QuotaSample> = {}): QuotaSample {
  return {
    provider: 'claude', kind: 'five_hour', scope: null, usedPercent,
    resetsAt: resetsAt === null ? null : iso(resetsAt), plan: 'max', ts: iso(ts), ...overrides,
  };
}

const totals = (totalTokens: number, cost: number | null): TokenTotals => ({
  inputTokens: totalTokens, cacheWriteTokens: 0, cacheReadTokens: 0, outputTokens: 0, reasoningTokens: 0,
  totalTokens, requests: 1, estimatedCostUsd: cost,
});

function threeCycles() {
  // cycle A ends T0+5h (peak 40), B ends T0+10h (peak 100), C ends T0+15h and is still running
  return buildQuotaHistory([
    sample(T0 + 1 * H, 10, T0 + 5 * H), sample(T0 + 4.8 * H, 40, T0 + 5 * H),
    sample(T0 + 6 * H, 20, T0 + 10 * H + 1500), sample(T0 + 9.9 * H, 100, T0 + 10 * H),
    sample(T0 + 11 * H, 5, T0 + 15 * H), sample(T0 + 12 * H, 12, T0 + 15 * H),
  ])[0];
}

test('samples split into cycles by reset deadline; deadline jitter stays one cycle', () => {
  const cycles = buildCycles(threeCycles(), T0 + 12.5 * H);
  expect(cycles.map((c) => [c.peak, c.completed])).toEqual([[40, true], [100, true], [12, false]]);
  expect(cycles[0].start).toBe(T0);
  expect(cycles[0].end).toBe(T0 + 5 * H);
  expect(limitReachedCount(cycles)).toBe(1);
});

test('weekly cycles start 7 days before the deadline and other windows have no cycles', () => {
  expect(windowLengthMs('seven_day')).toBe(7 * 24 * H);
  expect(windowLengthMs('other')).toBeNull();
  const [series] = buildQuotaHistory([sample(T0, 3, T0 + 100 * H, { kind: 'other' })]);
  expect(buildCycles(series, T0 + H)).toEqual([]);
});

test('a completed cycle with no samples near its end is flagged unreliable', () => {
  const [series] = buildQuotaHistory([sample(T0 + 0.5 * H, 30, T0 + 5 * H), sample(T0 + 1 * H, 31, T0 + 5 * H)]);
  const [cycle] = buildCycles(series, T0 + 6 * H);
  expect(cycle.completed).toBe(true);
  expect(cycle.reliable).toBe(false);
});

test('tokens per percent is the median of complete, reliable cycles', () => {
  const cycles = buildCycles(threeCycles(), T0 + 12.5 * H);
  // A: 40 % / 400k tokens -> 10k per %, B: 100 % / 2M -> 20k per %; running cycle ignored
  const usages: CycleUsage[] = [
    { cycle: cycles[0], totals: totals(400_000, 4) },
    { cycle: cycles[1], totals: totals(2_000_000, 20) },
    { cycle: cycles[2], totals: totals(9_999_999, 99) },
  ];
  const r = tokensPerPercent(usages)!;
  expect(r.cycles).toBe(2);
  expect(r.tokens).toBe(15_000);
  expect(r.costUsd).toBeCloseTo(0.15);
  expect(tokensPerPercent([{ cycle: cycles[0], totals: null }])).toBeNull();
  // an unpriced cycle removes the cost but keeps the token ratio
  const unpriced = tokensPerPercent([{ cycle: cycles[0], totals: totals(400_000, null) }])!;
  expect(unpriced.costUsd).toBeNull();
  expect(unpriced.tokens).toBe(10_000);
});

test('the forecast segment runs to exhaustion or to the reset and is hidden when stale or low-confidence', () => {
  const last = { ts: T0, usedPercent: 40 };
  const reset = iso(T0 + 5 * H);
  const base = { projectedPercentAtReset: 80, exhaustsAt: null, ratePercentPerHour: 8, confidence: 'high' as const };
  expect(forecastSegment(last, reset, base, T0)).toEqual([{ x: T0, y: 40 }, { x: T0 + 5 * H, y: 80 }]);
  const exhausts = iso(T0 + 2 * H);
  expect(forecastSegment(last, reset, { ...base, projectedPercentAtReset: 150, exhaustsAt: exhausts }, T0)![1]).toEqual({ x: T0 + 2 * H, y: 100 });
  expect(forecastSegment(last, reset, { ...base, projectedPercentAtReset: 150 }, T0)![1].y).toBe(100);
  expect(forecastSegment(last, reset, { ...base, confidence: 'low' }, T0)).toBeNull();
  expect(forecastSegment(last, reset, base, T0 + 6 * H)).toBeNull();
  expect(forecastSegment(last, reset, null, T0)).toBeNull();
});

test('reset gaps insert a null point so the line breaks instead of dropping', () => {
  const out = withResetGaps([{ x: 1, y: 5 }, { x: 10, y: 90 }, { x: 20, y: 2 }], new Set([2]));
  expect(out).toEqual([{ x: 1, y: 5 }, { x: 10, y: 90 }, { x: 19, y: null }, { x: 20, y: 2 }]);
});

test('plan hints are provider-aware and unknown plans give none', () => {
  expect(planPriceHint('claude', 'Claude Max 20x')).toBe(200);
  expect(planPriceHint('claude', 'Claude Max 5x')).toBe(100);
  expect(planPriceHint('claude', 'pro')).toBe(20);
  expect(planPriceHint('codex', 'plus')).toBe(20);
  expect(planPriceHint('codex', 'pro')).toBe(200);
  expect(planPriceHint('codex', null)).toBeNull();
  expect(planPriceHint('copilot', 'pro')).toBeNull();
});

test('subscription ROI compares the estimate with the price and projects the month end', () => {
  expect(subscriptionRoi(500, 0, 0.5)).toBeNull();
  const roi = subscriptionRoi(300, 100, 0.5)!;
  expect(roi.multiple).toBe(3);
  expect(roi.projectedUsd).toBe(600);
  expect(roi.projectedMultiple).toBe(6);
  expect(subscriptionRoi(5, 100, 0.001)!.projectedUsd).toBeNull();
  expect(subscriptionTotal({ claude: 100, codex: 20 }, null)).toBe(120);
  expect(subscriptionTotal({ claude: 100, codex: 20 }, 'codex')).toBe(20);
  expect(subscriptionTotal({ claude: -1 }, null)).toBe(0);
});

test('month-end projection continues the daily pace after the last elapsed day', () => {
  const series = [{ date: 'a', cumulativeUsd: 10 }, { date: 'b', cumulativeUsd: 30 }];
  expect(projectToMonthEnd(series, 5)).toEqual([null, 30, 45, 60, 75]);
  expect(projectToMonthEnd(series, 2)).toEqual([null, null]);
  expect(projectToMonthEnd([], 30)).toEqual([]);
});
