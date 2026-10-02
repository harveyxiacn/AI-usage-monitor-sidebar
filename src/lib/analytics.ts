// Pure derivations behind the History → Usage analytics views. [FRONTEND]
//
// No DOM, no Svelte, no Chart.js: everything here is a function of the rows the
// backend already returns, so it can be unit-tested with plain objects.
import { localDateInput, type HistoryRange } from './history';
import type { Bucket, CalendarDay, CalendarSlot, HistoryRow, TokenTotals } from './types';

type Counters = Pick<TokenTotals, 'inputTokens' | 'cacheReadTokens' | 'cacheWriteTokens'>;

export function emptyTotals(): TokenTotals {
  return {
    inputTokens: 0, cacheWriteTokens: 0, cacheReadTokens: 0, outputTokens: 0, reasoningTokens: 0,
    totalTokens: 0, requests: 0, estimatedCostUsd: 0, knownCostUsd: null, unpricedRequests: 0,
  };
}

/** Share of prompt-side tokens served from the cache; null when there are none. */
export function cacheHitRate(totals: Counters): number | null {
  const denominator = totals.inputTokens + totals.cacheReadTokens + totals.cacheWriteTokens;
  return denominator > 0 ? totals.cacheReadTokens / denominator : null;
}

/**
 * Add several totals together. "Cost unknown" stays sticky (contract §9): one
 * unpriced part makes the estimate null, while `knownCostUsd` keeps the sum of
 * what *is* priced.
 */
export function sumTotals(list: readonly TokenTotals[]): TokenTotals {
  const sum = emptyTotals();
  let anyKnown = false;
  let known = 0;
  for (const item of list) {
    sum.inputTokens += item.inputTokens;
    sum.cacheWriteTokens += item.cacheWriteTokens;
    sum.cacheReadTokens += item.cacheReadTokens;
    sum.outputTokens += item.outputTokens;
    sum.reasoningTokens += item.reasoningTokens;
    sum.totalTokens += item.totalTokens;
    sum.requests += item.requests;
    sum.unpricedRequests = (sum.unpricedRequests ?? 0) + (item.unpricedRequests ?? 0);
    sum.estimatedCostUsd = sum.estimatedCostUsd == null || item.estimatedCostUsd == null ? null : sum.estimatedCostUsd + item.estimatedCostUsd;
    const itemKnown = item.knownCostUsd ?? item.estimatedCostUsd;
    if (itemKnown != null) {
      anyKnown = true;
      known += itemKnown;
    }
  }
  sum.knownCostUsd = anyKnown ? known : null;
  return sum;
}

/** Relative change in percent; null when there is no baseline to compare with. */
export function deltaPercent(current: number, previous: number): number | null {
  if (!Number.isFinite(current) || !Number.isFinite(previous) || previous === 0) return null;
  return ((current - previous) / previous) * 100;
}

// ------------------------------------------------------------ KPI row ------

/** Key of the series that folds the long tail (cannot clash with JSON keys). */
export const OTHER_KEY = '__other__';

const DAY = 86_400_000;

/** Number of local calendar days touched by `[from, to)`. */
export function rangeDays(range: HistoryRange): number {
  const first = new Date(range.from);
  const last = new Date(Math.max(range.from, range.to - 1));
  const utc = (d: Date) => Date.UTC(d.getFullYear(), d.getMonth(), d.getDate());
  return Math.max(1, Math.round((utc(last) - utc(first)) / DAY) + 1);
}

/**
 * The period of equal length (in calendar days) directly before `range`.
 * Calendar arithmetic keeps 23/25-hour DST days whole.
 */
export function previousPeriod(range: HistoryRange): HistoryRange {
  const from = new Date(range.from);
  from.setHours(0, 0, 0, 0);
  const to = new Date(from);
  from.setDate(from.getDate() - rangeDays(range));
  return { from: from.getTime(), to: to.getTime() };
}

/** Every local `YYYY-MM-DD` of `[from, to)`, in order. */
export function localDays(range: HistoryRange): string[] {
  const days: string[] = [];
  const cursor = new Date(range.from);
  cursor.setHours(0, 0, 0, 0);
  const count = rangeDays(range);
  for (let i = 0; i < count; i += 1) {
    days.push(localDateInput(cursor.getTime()));
    cursor.setDate(cursor.getDate() + 1);
    cursor.setHours(0, 0, 0, 0);
  }
  return days;
}

export type KpiId = 'tokens' | 'cost' | 'requests' | 'activeDays' | 'cacheHit' | 'dailyAverage';

export interface Kpi {
  id: KpiId;
  /** current value; `cost` carries the full totals via `totals` for partial labels */
  value: number | null;
  /** change versus the previous period: percent, except `cacheHit` (percentage points) */
  delta: number | null;
  deltaUnit: 'percent' | 'points';
  /** one point per local day of the current range */
  spark: number[];
}

export interface KpiSet {
  items: Kpi[];
  totals: TokenTotals;
  days: number;
}

function activeDay(day: CalendarDay): boolean {
  return day.requests > 0 || day.totalTokens > 0;
}

/** Trailing moving average, used as the "daily average" trend. */
export function movingAverage(values: readonly number[], window = 7): number[] {
  return values.map((_, index) => {
    const slice = values.slice(Math.max(0, index - window + 1), index + 1);
    return slice.reduce((a, b) => a + b, 0) / slice.length;
  });
}

/**
 * KPI tiles from the calendar's daily rows. `current` and `previous` are the
 * rows inside / before the range; `dayList` is the range's local days so quiet
 * days still occupy a sparkline slot.
 */
export function buildKpis(current: readonly CalendarDay[], previous: readonly CalendarDay[], dayList: readonly string[]): KpiSet {
  const totals = sumTotals(current);
  const before = sumTotals(previous);
  const byDate = new Map(current.map((day) => [day.date, day]));
  const rows = dayList.map((date) => byDate.get(date) ?? null);
  const tokensSpark = rows.map((day) => day?.totalTokens ?? 0);
  const activeCount = current.filter(activeDay).length;
  const beforeActive = previous.filter(activeDay).length;
  const days = Math.max(1, dayList.length);
  const average = totals.totalTokens / days;
  const beforeAverage = before.totalTokens / days;
  const hit = cacheHitRate(totals);
  const beforeHit = cacheHitRate(before);
  let cumulative = 0;
  const costBoth = totals.estimatedCostUsd != null && before.estimatedCostUsd != null;

  const items: Kpi[] = [
    { id: 'tokens', value: totals.totalTokens, delta: deltaPercent(totals.totalTokens, before.totalTokens), deltaUnit: 'percent', spark: tokensSpark },
    {
      id: 'cost',
      value: totals.estimatedCostUsd ?? totals.knownCostUsd ?? null,
      delta: costBoth ? deltaPercent(totals.estimatedCostUsd!, before.estimatedCostUsd!) : null,
      deltaUnit: 'percent',
      spark: rows.map((day) => day?.estimatedCostUsd ?? day?.knownCostUsd ?? 0),
    },
    { id: 'requests', value: totals.requests, delta: deltaPercent(totals.requests, before.requests), deltaUnit: 'percent', spark: rows.map((day) => day?.requests ?? 0) },
    {
      id: 'activeDays',
      value: activeCount,
      delta: deltaPercent(activeCount, beforeActive),
      deltaUnit: 'percent',
      spark: rows.map((day) => (cumulative += day && activeDay(day) ? 1 : 0)),
    },
    {
      id: 'cacheHit',
      value: hit,
      delta: hit != null && beforeHit != null ? (hit - beforeHit) * 100 : null,
      deltaUnit: 'points',
      spark: rows.map((day) => (day ? cacheHitRate(day) ?? 0 : 0)),
    },
    { id: 'dailyAverage', value: average, delta: deltaPercent(average, beforeAverage), deltaUnit: 'percent', spark: movingAverage(tokensSpark) },
  ];
  return { items, totals, days };
}

/** Split a combined calendar response at the start of the current range. */
export function splitPeriods(days: readonly CalendarDay[], range: HistoryRange): { current: CalendarDay[]; previous: CalendarDay[] } {
  const boundary = localDateInput(range.from);
  const last = localDateInput(range.to - 1);
  return {
    current: days.filter((day) => day.date >= boundary && day.date <= last),
    previous: days.filter((day) => day.date < boundary),
  };
}

// ------------------------------------------------ token composition -------

export interface CompositionBucket {
  bucketStart: string;
  input: number;
  cacheRead: number;
  cacheWrite: number;
  /** output tokens that are not reasoning tokens */
  output: number;
  reasoning: number;
  total: number;
  hitRate: number | null;
}

/**
 * One entry per bucket with the token mix summed across every series.
 * Reasoning tokens are a subset of the output counter, so they are carved out
 * of it: the five segments add up to `total` and nothing is counted twice.
 */
export function buildComposition(rows: readonly HistoryRow[]): CompositionBucket[] {
  const map = new Map<string, TokenTotals>();
  for (const row of rows) {
    const entry = map.get(row.bucketStart) ?? emptyTotals();
    entry.inputTokens += row.inputTokens;
    entry.cacheReadTokens += row.cacheReadTokens;
    entry.cacheWriteTokens += row.cacheWriteTokens;
    entry.outputTokens += row.outputTokens;
    entry.reasoningTokens += row.reasoningTokens;
    map.set(row.bucketStart, entry);
  }
  return [...map.entries()]
    .sort(([a], [b]) => Date.parse(a) - Date.parse(b))
    .map(([bucketStart, t]) => {
      const reasoning = Math.min(t.reasoningTokens, t.outputTokens);
      const output = t.outputTokens - reasoning;
      return {
        bucketStart,
        input: t.inputTokens,
        cacheRead: t.cacheReadTokens,
        cacheWrite: t.cacheWriteTokens,
        output,
        reasoning,
        total: t.inputTokens + t.cacheReadTokens + t.cacheWriteTokens + t.outputTokens,
        hitRate: cacheHitRate(t),
      };
    });
}

// ------------------------------------------------- top-N folding ----------

export interface FoldableSeries {
  key: string;
  values: Array<number | null>;
}

/**
 * Keep the `limit` series with the largest total and merge the rest into one
 * "Other" series (null stays sticky, like every cost sum). Returns the input
 * untouched when it already fits. `other` is the merged series' key.
 */
export function foldSeries<T extends FoldableSeries>(
  series: readonly T[],
  limit: number,
  makeOther: (values: Array<number | null>, count: number) => T
): { series: T[]; folded: number } {
  if (series.length <= limit) return { series: [...series], folded: 0 };
  const weight = (s: T) => s.values.reduce<number>((sum, v) => sum + (v ?? 0), 0);
  const ranked = [...series].sort((a, b) => weight(b) - weight(a) || a.key.localeCompare(b.key));
  const kept = new Set(ranked.slice(0, limit).map((s) => s.key));
  const rest = series.filter((s) => !kept.has(s.key));
  const length = Math.max(0, ...series.map((s) => s.values.length));
  const merged: Array<number | null> = new Array(length).fill(0);
  for (const s of rest) {
    s.values.forEach((value, index) => {
      merged[index] = value == null || merged[index] == null ? null : merged[index]! + value;
    });
  }
  return { series: [...series.filter((s) => kept.has(s.key)), makeOther(merged, rest.length)], folded: rest.length };
}

/** Share (0..100) of `value` in `total`; 0 when there is nothing to share. */
export function sharePercent(value: number | null | undefined, total: number): number {
  return value != null && total > 0 ? (value / total) * 100 : 0;
}

// ---------------------------------------------- bucket → range (click) ----

/**
 * The local date range a clicked chart bucket stands for, or null when the
 * bucket (an hour) is already finer than the day-based range controls.
 */
export function bucketDateRange(bucketStart: string, bucket: Bucket): { from: string; to: string } | null {
  const start = new Date(bucketStart);
  if (!Number.isFinite(start.getTime()) || bucket === 'hour') return null;
  const first = new Date(start);
  first.setHours(0, 0, 0, 0);
  const last = new Date(first);
  if (bucket === 'week') last.setDate(last.getDate() + 6);
  else if (bucket === 'month') {
    last.setMonth(last.getMonth() + 1);
    last.setDate(0);
  }
  return { from: localDateInput(first.getTime()), to: localDateInput(last.getTime()) };
}

// ------------------------------------------------ model / effort mix ------

export interface MixSeries {
  key: string;
  label: string;
  /** one value per bucket, in tokens */
  values: number[];
  total: number;
}

export interface MixResult {
  buckets: string[];
  series: MixSeries[];
  /** reasoning effort → tokens; the unrecorded effort is under the key "" */
  efforts: Array<{ effort: string; tokens: number; share: number }>;
  total: number;
}

/**
 * Tokens per model per bucket (variants of one model merged) with the `limit`
 * biggest models kept and the remainder folded into `otherLabel`, plus the
 * overall reasoning-effort split.
 */
export function buildMix(rows: readonly HistoryRow[], limit: number, otherLabel: (count: number) => string, unknownModel: string): MixResult {
  const buckets = [...new Set(rows.map((r) => r.bucketStart))].sort((a, b) => Date.parse(a) - Date.parse(b));
  const index = new Map(buckets.map((b, i) => [b, i]));
  const models = new Map<string, number[]>();
  const effortTokens = new Map<string, number>();
  let total = 0;
  for (const row of rows) {
    const key = row.model ?? '';
    const values = models.get(key) ?? new Array<number>(buckets.length).fill(0);
    values[index.get(row.bucketStart)!] += row.totalTokens;
    models.set(key, values);
    const effort = row.reasoningEffort ?? '';
    effortTokens.set(effort, (effortTokens.get(effort) ?? 0) + row.totalTokens);
    total += row.totalTokens;
  }
  const all: Array<MixSeries> = [...models.entries()].map(([key, values]) => ({
    key,
    label: key || unknownModel,
    values,
    total: values.reduce((a, b) => a + b, 0),
  }));
  const folded = foldSeries(
    all.map((s) => ({ ...s, values: s.values as Array<number | null> })),
    limit,
    (values, count) => ({ key: '\u0000other', label: otherLabel(count), values, total: values.reduce<number>((a, b) => a + (b ?? 0), 0) })
  );
  const series = folded.series
    .map((s) => ({ ...s, values: s.values.map((v) => v ?? 0) }))
    .sort((a, b) => (a.key === '\u0000other' ? 1 : b.key === '\u0000other' ? -1 : b.total - a.total || a.label.localeCompare(b.label)));
  const efforts = [...effortTokens.entries()]
    .map(([effort, tokens]) => ({ effort, tokens, share: sharePercent(tokens, total) }))
    .sort((a, b) => b.tokens - a.tokens || a.effort.localeCompare(b.effort));
  return { buckets, series, efforts, total };
}

// ------------------------------------------------- project ranking --------

export interface ProjectRank {
  project: string;
  totals: TokenTotals;
  hitRate: number | null;
  sessions: number;
}

/** Top `limit` projects (exact cwd) by total tokens, ties broken by path. */
export function rankProjects(rows: readonly HistoryRow[], sessionCounts: ReadonlyMap<string, number>, limit = 10): ProjectRank[] {
  const by = new Map<string, TokenTotals[]>();
  for (const row of rows) {
    const key = row.project ?? '';
    const list = by.get(key) ?? [];
    list.push(row);
    by.set(key, list);
  }
  return [...by.entries()]
    .map(([project, list]) => {
      const totals = sumTotals(list);
      return { project, totals, hitRate: cacheHitRate(totals), sessions: sessionCounts.get(project) ?? 0 };
    })
    .sort((a, b) => b.totals.totalTokens - a.totals.totalTokens || a.project.localeCompare(b.project))
    .slice(0, limit);
}

/** Sessions per project from the session drill-down rows. */
export function countSessionsByProject(rows: ReadonlyArray<{ project: string }>): Map<string, number> {
  const counts = new Map<string, number>();
  for (const row of rows) counts.set(row.project, (counts.get(row.project) ?? 0) + 1);
  return counts;
}

// ------------------------------------------------------ heatmap insight ---

export interface PeakWindow {
  /** first hour of the window, 0..23 */
  startHour: number;
  /** exclusive end hour, 1..24 */
  endHour: number;
  /** share of the metric inside the window, 0..100 */
  share: number;
  /** 0 = Monday … 6 = Sunday, the busiest weekday */
  weekday: number;
}

/**
 * The busiest `width`-hour window of the day (all weekdays folded together)
 * and the busiest weekday. Null without any activity.
 */
export function peakWindow(slots: readonly CalendarSlot[], value: (slot: CalendarSlot) => number, width = 3): PeakWindow | null {
  const hours = new Array<number>(24).fill(0);
  const weekdays = new Array<number>(7).fill(0);
  let total = 0;
  for (const slot of slots) {
    const v = value(slot);
    if (!(v > 0) || slot.hour < 0 || slot.hour > 23 || slot.weekday < 0 || slot.weekday > 6) continue;
    hours[slot.hour] += v;
    weekdays[slot.weekday] += v;
    total += v;
  }
  if (total <= 0) return null;
  let best = 0;
  let bestSum = -1;
  for (let start = 0; start + width <= 24; start += 1) {
    let sum = 0;
    for (let h = start; h < start + width; h += 1) sum += hours[h];
    if (sum > bestSum) {
      bestSum = sum;
      best = start;
    }
  }
  const weekday = weekdays.indexOf(Math.max(...weekdays));
  return { startHour: best, endHour: best + width, share: (bestSum / total) * 100, weekday };
}
