import { test, expect } from '@playwright/test';
import {
  OTHER_KEY,
  bucketDateRange,
  buildComposition,
  buildKpis,
  buildMix,
  cacheHitRate,
  countSessionsByProject,
  deltaPercent,
  foldSeries,
  localDays,
  movingAverage,
  peakWindow,
  previousPeriod,
  rangeDays,
  rankProjects,
  sharePercent,
  splitPeriods,
  sumTotals,
} from '../src/lib/analytics';
import { historyRange, metricOf } from '../src/lib/history';
import type { CalendarDay, CalendarSlot, HistoryRow, TokenTotals } from '../src/lib/types';

/** Run `body` with a fixed local time zone, whatever the host is set to. */
function inTimeZone(tz: string, body: () => void) {
  const original = process.env.TZ;
  process.env.TZ = tz;
  try {
    body();
  } finally {
    if (original === undefined) delete process.env.TZ;
    else process.env.TZ = original;
  }
}

function totals(over: Partial<TokenTotals> = {}): TokenTotals {
  const base: TokenTotals = {
    inputTokens: 0, cacheWriteTokens: 0, cacheReadTokens: 0, outputTokens: 0, reasoningTokens: 0,
    totalTokens: 0, requests: 0, estimatedCostUsd: 0, knownCostUsd: 0, unpricedRequests: 0,
  };
  const merged = { ...base, ...over };
  if (over.totalTokens === undefined) merged.totalTokens = merged.inputTokens + merged.cacheWriteTokens + merged.cacheReadTokens + merged.outputTokens;
  return merged;
}

function day(date: string, over: Partial<TokenTotals> = {}): CalendarDay {
  return { date, ...totals({ requests: 1, ...over }) };
}

function row(over: Partial<HistoryRow> & { bucketStart: string }): HistoryRow {
  return { provider: 'claude', model: 'm', reasoningEffort: null, project: null, ...totals(over), ...over } as HistoryRow;
}

test('cache hit rate is cache read over every prompt-side token', () => {
  expect(cacheHitRate(totals({ inputTokens: 100, cacheReadTokens: 300, cacheWriteTokens: 100 }))).toBeCloseTo(0.6);
  expect(cacheHitRate(totals({ outputTokens: 50 }))).toBeNull();
  expect(cacheHitRate(totals({ cacheReadTokens: 10 }))).toBe(1);
});

test('summing totals keeps "cost unknown" sticky but remembers the priced part', () => {
  const sum = sumTotals([
    totals({ inputTokens: 10, requests: 1, estimatedCostUsd: 1, knownCostUsd: 1 }),
    totals({ inputTokens: 5, requests: 2, estimatedCostUsd: null, knownCostUsd: 0.5, unpricedRequests: 2 }),
  ]);
  expect(sum.inputTokens).toBe(15);
  expect(sum.requests).toBe(3);
  expect(sum.estimatedCostUsd).toBeNull();
  expect(sum.knownCostUsd).toBeCloseTo(1.5);
  expect(sum.unpricedRequests).toBe(2);
  expect(sumTotals([]).knownCostUsd).toBeNull();
  expect(sumTotals([totals({ estimatedCostUsd: 2, knownCostUsd: undefined })]).knownCostUsd).toBe(2);
});

test('delta percent needs a non-zero baseline', () => {
  expect(deltaPercent(150, 100)).toBe(50);
  expect(deltaPercent(50, 100)).toBe(-50);
  expect(deltaPercent(5, 0)).toBeNull();
  expect(deltaPercent(Number.NaN, 4)).toBeNull();
});

test('the previous period has the same number of calendar days and ends where the range starts', () => {
  inTimeZone('America/New_York', () => {
    // 7d preset: 2026-09-24 .. 2026-09-30 (today, partial)
    const now = new Date('2026-09-30T15:30:00').getTime();
    const range = historyRange('7d', '', '', now)!;
    expect(rangeDays(range)).toBe(7);
    const before = previousPeriod(range);
    expect(before.to).toBe(range.from);
    expect(new Date(before.from).getDate()).toBe(17);
    expect(rangeDays({ from: before.from, to: before.to })).toBe(7);
  });
});

test('the previous period stays whole across a DST change', () => {
  inTimeZone('America/New_York', () => {
    // 2026-11-01 has 25 hours (fall back)
    const range = historyRange('custom', '2026-11-01', '2026-11-03', 0)!;
    expect(rangeDays(range)).toBe(3);
    const before = previousPeriod(range);
    expect(new Date(before.from).getHours()).toBe(0);
    expect(new Date(before.from).getDate()).toBe(29);
    expect(localDays(range)).toEqual(['2026-11-01', '2026-11-02', '2026-11-03']);
    expect(localDays(before)).toEqual(['2026-10-29', '2026-10-30', '2026-10-31']);
  });
  inTimeZone('UTC', () => {
    expect(rangeDays({ from: Date.UTC(2026, 0, 5, 12), to: Date.UTC(2026, 0, 5, 13) })).toBe(1);
  });
});

test('calendar rows split into the current range and the period before it', () => {
  inTimeZone('UTC', () => {
    const range = historyRange('custom', '2026-09-08', '2026-09-10', 0)!;
    const days = [day('2026-09-06'), day('2026-09-08'), day('2026-09-10'), day('2026-09-11')];
    const { current, previous } = splitPeriods(days, range);
    expect(current.map((d) => d.date)).toEqual(['2026-09-08', '2026-09-10']);
    expect(previous.map((d) => d.date)).toEqual(['2026-09-06']);
  });
});

test('KPIs carry values, deltas against the previous period and one spark point per day', () => {
  const dayList = ['2026-09-08', '2026-09-09', '2026-09-10'];
  const current = [
    day('2026-09-08', { inputTokens: 100, cacheReadTokens: 300, outputTokens: 100, requests: 4, estimatedCostUsd: 2, knownCostUsd: 2 }),
    day('2026-09-10', { inputTokens: 100, cacheReadTokens: 100, outputTokens: 100, requests: 6, estimatedCostUsd: 1, knownCostUsd: 1 }),
  ];
  const previous = [day('2026-09-06', { inputTokens: 100, cacheReadTokens: 100, outputTokens: 0, requests: 5, estimatedCostUsd: 1.5, knownCostUsd: 1.5 })];
  const set = buildKpis(current, previous, dayList);
  const by = Object.fromEntries(set.items.map((k) => [k.id, k]));
  expect(by.tokens.value).toBe(800);
  expect(by.tokens.delta).toBeCloseTo(300); // 800 vs 200
  expect(by.tokens.spark).toEqual([500, 0, 300]);
  expect(by.cost.value).toBe(3);
  expect(by.cost.delta).toBeCloseTo(100);
  expect(by.requests.value).toBe(10);
  expect(by.activeDays.value).toBe(2);
  expect(by.activeDays.spark).toEqual([1, 1, 2]);
  expect(by.cacheHit.value).toBeCloseTo(400 / 600);
  expect(by.cacheHit.deltaUnit).toBe('points');
  expect(by.cacheHit.delta).toBeCloseTo((400 / 600 - 0.5) * 100);
  expect(by.dailyAverage.value).toBeCloseTo(800 / 3);
  expect(set.days).toBe(3);
});

test('KPIs have no delta without earlier data and no cost delta for partial prices', () => {
  const dayList = ['2026-09-08'];
  const set = buildKpis(
    [day('2026-09-08', { inputTokens: 10, estimatedCostUsd: null, knownCostUsd: 0.2, unpricedRequests: 1 })],
    [day('2026-09-01', { inputTokens: 10, estimatedCostUsd: 1 })],
    dayList
  );
  const by = Object.fromEntries(set.items.map((k) => [k.id, k]));
  expect(by.cost.delta).toBeNull();
  expect(by.cost.value).toBe(0.2);
  const none = buildKpis([], [], dayList);
  expect(none.items.every((k) => k.delta === null || k.id === 'activeDays' || k.id === 'tokens' || k.id === 'requests' || k.id === 'dailyAverage')).toBe(true);
  expect(none.items.find((k) => k.id === 'tokens')!.delta).toBeNull();
  expect(none.items.find((k) => k.id === 'cacheHit')!.value).toBeNull();
});

test('moving average trails a window', () => {
  expect(movingAverage([2, 4, 6], 2)).toEqual([2, 3, 5]);
});

test('composition carves reasoning out of output so segments sum to the total', () => {
  const rows = [
    row({ bucketStart: '2026-09-02T00:00:00Z', inputTokens: 10, cacheReadTokens: 60, cacheWriteTokens: 10, outputTokens: 20, reasoningTokens: 5 }),
    row({ bucketStart: '2026-09-01T00:00:00Z', inputTokens: 10, outputTokens: 10, reasoningTokens: 99 }),
    row({ bucketStart: '2026-09-02T00:00:00Z', provider: 'codex', inputTokens: 10, cacheReadTokens: 40, outputTokens: 0 }),
  ];
  const [first, second] = buildComposition(rows);
  expect(first.bucketStart).toBe('2026-09-01T00:00:00Z');
  expect(first.reasoning).toBe(10); // clamped to the output counter
  expect(first.output).toBe(0);
  expect(second.input).toBe(20);
  expect(second.cacheRead).toBe(100);
  expect(second.reasoning).toBe(5);
  expect(second.output).toBe(15);
  expect(second.total).toBe(second.input + second.cacheRead + second.cacheWrite + second.output + second.reasoning);
  expect(second.hitRate).toBeCloseTo(100 / 130);
});

test('folding keeps the biggest series and merges the rest, keeping null sticky', () => {
  const series = [
    { key: 'a', values: [1, 1] as Array<number | null> },
    { key: 'b', values: [10, 10] as Array<number | null> },
    { key: 'c', values: [5, null] as Array<number | null> },
    { key: 'd', values: [2, 2] as Array<number | null> },
  ];
  const { series: out, folded } = foldSeries(series, 2, (values, count) => ({ key: OTHER_KEY, values, count } as (typeof series)[number]));
  expect(folded).toBe(2);
  expect(out.map((s) => s.key)).toEqual(['b', 'c', OTHER_KEY]);
  // top two by sum are b (20) and c (5); a and d merge
  expect(out[2].values).toEqual([3, 3]);
  const withNull = foldSeries(series, 1, (values) => ({ key: OTHER_KEY, values } as (typeof series)[number]));
  expect(withNull.series.map((s) => s.key)).toEqual(['b', OTHER_KEY]);
  expect(withNull.series[1].values).toEqual([8, null]);
  // nothing to fold
  const same = foldSeries(series, 8, () => { throw new Error('unused'); });
  expect(same.series).toHaveLength(4);
  expect(same.folded).toBe(0);
});

test('share percent is zero without a total', () => {
  expect(sharePercent(25, 100)).toBe(25);
  expect(sharePercent(5, 0)).toBe(0);
  expect(sharePercent(null, 10)).toBe(0);
});

test('a clicked bucket becomes the date range it covers', () => {
  inTimeZone('Asia/Singapore', () => {
    expect(bucketDateRange('2026-09-21T00:00:00+08:00', 'day')).toEqual({ from: '2026-09-21', to: '2026-09-21' });
    expect(bucketDateRange('2026-09-21T00:00:00+08:00', 'week')).toEqual({ from: '2026-09-21', to: '2026-09-27' });
    expect(bucketDateRange('2026-02-01T00:00:00+08:00', 'month')).toEqual({ from: '2026-02-01', to: '2026-02-28' });
    expect(bucketDateRange('2026-09-21T10:00:00+08:00', 'hour')).toBeNull();
    expect(bucketDateRange('not a date', 'day')).toBeNull();
  });
});

test('model mix merges effort variants, folds the tail and reports effort shares', () => {
  const rows = [
    row({ bucketStart: '2026-09-01T00:00:00Z', model: 'big', reasoningEffort: 'high', totalTokens: 60 }),
    row({ bucketStart: '2026-09-01T00:00:00Z', model: 'big', reasoningEffort: 'low', totalTokens: 20 }),
    row({ bucketStart: '2026-09-02T00:00:00Z', model: 'big', reasoningEffort: null, totalTokens: 10 }),
    row({ bucketStart: '2026-09-02T00:00:00Z', model: 'x', totalTokens: 6 }),
    row({ bucketStart: '2026-09-02T00:00:00Z', model: 'y', totalTokens: 4 }),
    row({ bucketStart: '2026-09-02T00:00:00Z', model: null, totalTokens: 1 }),
  ];
  const mix = buildMix(rows, 2, (n) => `Other (${n})`, 'Unknown model');
  expect(mix.buckets).toHaveLength(2);
  expect(mix.series.map((s) => s.label)).toEqual(['big', 'x', 'Other (2)']);
  expect(mix.series[0].values).toEqual([80, 10]);
  expect(mix.series[2].values).toEqual([0, 5]);
  expect(mix.total).toBe(101);
  expect(mix.efforts[0]).toMatchObject({ effort: 'high', tokens: 60 });
  expect(mix.efforts.find((e) => e.effort === '')!.tokens).toBe(21);
  expect(mix.efforts.reduce((s, e) => s + e.share, 0)).toBeCloseTo(100);
});

test('project ranking sorts by tokens, caps at ten and attaches cache hit and sessions', () => {
  const rows: HistoryRow[] = [];
  for (let i = 0; i < 12; i += 1) {
    rows.push(row({ bucketStart: '2026-09-01T00:00:00Z', project: `/p/${String(i).padStart(2, '0')}`, inputTokens: 10, cacheReadTokens: 10 * (i + 1) }));
    rows.push(row({ bucketStart: '2026-08-01T00:00:00Z', project: `/p/${String(i).padStart(2, '0')}`, inputTokens: 10, cacheReadTokens: 0 }));
  }
  const counts = countSessionsByProject([{ project: '/p/11' }, { project: '/p/11' }, { project: '/p/03' }]);
  const ranks = rankProjects(rows, counts, 10);
  expect(ranks).toHaveLength(10);
  expect(ranks[0].project).toBe('/p/11');
  expect(ranks[0].sessions).toBe(2);
  expect(ranks[0].totals.totalTokens).toBe(10 + 120 + 10);
  expect(ranks[0].hitRate).toBeCloseTo(120 / 140);
  expect(ranks.at(-1)!.project).toBe('/p/02');
  expect(ranks.find((r) => r.project === '/p/03')!.sessions).toBe(1);
});

test('the peak window is the busiest three hours, all weekdays folded', () => {
  const slot = (weekday: number, hour: number, totalTokens: number): CalendarSlot => ({ weekday, hour, ...totals({ totalTokens, requests: 1 }) });
  const slots = [slot(0, 9, 10), slot(1, 14, 30), slot(2, 15, 40), slot(2, 16, 20), slot(3, 22, 5)];
  const peak = peakWindow(slots, (s) => s.totalTokens)!;
  expect(peak.startHour).toBe(14);
  expect(peak.endHour).toBe(17);
  expect(peak.share).toBeCloseTo((90 / 105) * 100);
  expect(peak.weekday).toBe(2);
  expect(peakWindow([], (s) => s.totalTokens)).toBeNull();
  expect(peakWindow([slot(0, 1, 0)], (s) => s.totalTokens)).toBeNull();
});

test('heatmap metrics: requests and cache hit rate', () => {
  const t = totals({ inputTokens: 100, cacheReadTokens: 300, requests: 7 });
  expect(metricOf(t, 'requests')).toBe(7);
  expect(metricOf(t, 'cache')).toBeCloseTo(0.75);
  expect(metricOf(totals({ outputTokens: 4 }), 'cache')).toBeNull();
  expect(metricOf({ totalTokens: 5, estimatedCostUsd: 1 }, 'requests')).toBe(0);
});
