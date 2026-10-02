// Quota cycles: split a window's samples by reset deadline, relate each
// completed cycle to the tokens consumed in it, and draw the forecast.
// Pure (no i18n) so `tests/quota-cycles.unit.ts` can import it in Node. [FRONTEND]
import type { QuotaHistorySeries } from './quota-history';
import type { QuotaForecast, TokenTotals, WindowKind } from './types';

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** Deadlines of one cycle jitter by seconds between polls; anything closer is the same cycle. */
const SAME_DEADLINE_MS = 5 * MINUTE;
/** A cycle whose last sample is further than this share of the window from its end under-reports its peak. */
const COVERAGE_SLACK = 0.1;
/** Peaks below this are noise when dividing tokens by percent. */
const MIN_PEAK_FOR_RATIO = 5;
/** Median over at most this many of the most recent reliable cycles. */
const RECENT_CYCLES = 8;

/** Window length by kind, or null when the window has no fixed length. */
export function windowLengthMs(kind: WindowKind): number | null {
  return kind === 'five_hour' ? 5 * HOUR : kind === 'seven_day' ? 7 * DAY : null;
}

export interface QuotaCycle {
  /** resets_at minus the window length (unix ms) */
  start: number;
  /** the reset deadline (unix ms) */
  end: number;
  /** highest used percent observed in the cycle */
  peak: number;
  samples: number;
  /** end <= now */
  completed: boolean;
  /** peak reached 100 % */
  limitReached: boolean;
  /** samples reach the end of the cycle, so `peak` is the real peak (always true for a running cycle) */
  reliable: boolean;
}

/**
 * Cycles of one window series, oldest first. Samples without a deadline are
 * skipped; a series whose kind has no fixed length yields no cycles.
 */
export function buildCycles(series: QuotaHistorySeries, now: number): QuotaCycle[] {
  const length = windowLengthMs(series.kind);
  if (length === null) return [];
  const groups: Array<{ end: number; peak: number; samples: number; lastTs: number }> = [];
  for (const sample of series.samples) {
    if (sample.resetsAt === null) continue;
    const end = Date.parse(sample.resetsAt);
    const ts = Date.parse(sample.ts);
    if (!Number.isFinite(end) || !Number.isFinite(ts)) continue;
    const last = groups[groups.length - 1];
    if (last && Math.abs(end - last.end) < SAME_DEADLINE_MS) {
      last.end = Math.max(last.end, end);
      last.peak = Math.max(last.peak, sample.usedPercent);
      last.samples++;
      last.lastTs = ts;
    } else {
      groups.push({ end, peak: sample.usedPercent, samples: 1, lastTs: ts });
    }
  }
  return groups.map((g) => {
    const completed = g.end <= now;
    return {
      start: g.end - length,
      end: g.end,
      peak: g.peak,
      samples: g.samples,
      completed,
      limitReached: g.peak >= 100,
      reliable: !completed || g.peak >= 100 || g.end - g.lastTs <= length * COVERAGE_SLACK,
    };
  });
}

export const limitReachedCount = (cycles: readonly QuotaCycle[]) =>
  cycles.filter((c) => c.limitReached).length;

export interface CycleUsage {
  cycle: QuotaCycle;
  /** null while the backend answer is pending or failed */
  totals: TokenTotals | null;
}

export interface TokensPerPercent {
  tokens: number;
  /** null when any contributing cycle had an unpriced request */
  costUsd: number | null;
  /** how many cycles the median covers */
  cycles: number;
}

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

/** Tokens (and cost) one percentage point of quota cost, median over recent completed, reliable cycles. */
export function tokensPerPercent(usages: readonly CycleUsage[]): TokensPerPercent | null {
  const usable = usages
    .filter(({ cycle, totals }) => cycle.completed && cycle.reliable && cycle.peak >= MIN_PEAK_FOR_RATIO &&
      totals !== null && totals.totalTokens > 0)
    .slice(-RECENT_CYCLES);
  if (usable.length === 0) return null;
  const tokens = median(usable.map(({ cycle, totals }) => totals!.totalTokens / cycle.peak));
  const costs = usable.map(({ cycle, totals }) =>
    totals!.estimatedCostUsd == null ? null : totals!.estimatedCostUsd / cycle.peak);
  const priced = costs.every((c) => c !== null);
  return { tokens, costUsd: priced ? median(costs as number[]) : null, cycles: usable.length };
}

/**
 * The dashed "at this pace" line: from the last sample to either the moment
 * 100 % is reached or the reset (at the projected percent). Null when the
 * cycle is over, there is no forecast, or the guess is "low".
 */
export function forecastSegment(
  last: { ts: number; usedPercent: number } | null,
  resetsAt: string | null,
  forecast: QuotaForecast | null | undefined,
  now: number,
): Array<{ x: number; y: number }> | null {
  if (!last || !forecast || forecast.confidence === 'low') return null;
  const reset = resetsAt ? Date.parse(resetsAt) : NaN;
  if (Number.isFinite(reset) && reset <= now) return null;
  const exhausts = forecast.exhaustsAt ? Date.parse(forecast.exhaustsAt) : NaN;
  let end: { x: number; y: number } | null = null;
  if (Number.isFinite(exhausts) && exhausts > last.ts) end = { x: exhausts, y: 100 };
  else if (Number.isFinite(reset) && reset > last.ts) {
    end = { x: reset, y: Math.min(100, Math.max(last.usedPercent, forecast.projectedPercentAtReset)) };
  }
  return end ? [{ x: last.ts, y: last.usedPercent }, end] : null;
}

/** Insert a null point before each listed index so the line breaks there instead of crossing a reset. */
export function withResetGaps(
  points: ReadonlyArray<{ x: number; y: number }>,
  breakBefore: ReadonlySet<number>,
): Array<{ x: number; y: number | null }> {
  const out: Array<{ x: number; y: number | null }> = [];
  points.forEach((p, i) => {
    if (breakBefore.has(i) && i > 0) out.push({ x: p.x - 1, y: null });
    out.push(p);
  });
  return out;
}
