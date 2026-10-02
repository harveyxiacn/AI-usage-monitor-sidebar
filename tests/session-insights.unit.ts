import { test, expect } from '@playwright/test';
import { binPosition, bubbleRadius, buildInsights, filtersFromParams, formatRate, formatUsdCompact, insightFlags, logHistogram, percentile, sessionFilters } from '../src/lib/session-insights';
import { sessionMockInvoke } from '../src/lib/session-mock';
import type { SessionInsights, SessionSummary } from '../src/lib/session-types';

const row = (over: Partial<SessionSummary>): SessionSummary => ({
  provider: 'codex', sessionId: 's', project: '/p', title: 't', titleSource: 'native', firstTs: '2026-01-01T00:00:00Z', lastTs: '2026-01-01T01:00:00Z',
  durationMs: 1, models: [], inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0, reasoningTokens: 0, totalTokens: 10, requests: 1,
  estimatedCostUsd: 1, userTurns: 2, toolCalls: 10, toolFailures: 0, repeatedToolCalls: 0, activeDurationMs: 60000, transcriptAvailable: true, parentSessionId: null,
  ...over,
} as SessionSummary);

test('percentiles interpolate and handle empty samples', () => {
  expect(percentile([], 0.5)).toBeNull();
  expect(percentile([1, 2, 3, 4], 0.5)).toBe(2.5);
  expect(percentile([1, 2, 3, 4, 5], 0.9)).toBeCloseTo(4.6);
});

test('log histogram bins are contiguous, count positives and keep zeros apart', () => {
  const h = logHistogram([0, 0.011, 0.5, 0.9, 12]);
  expect(h.zeroCount).toBe(1);
  expect(h.sample).toBe(5);
  expect(h.bins.reduce((a, b) => a + b.count, 0)).toBe(4);
  for (let i = 1; i < h.bins.length; i++) expect(h.bins[i].from).toBeCloseTo(h.bins[i - 1].to, 9);
  expect(h.bins[0].from).toBeLessThanOrEqual(0.011);
  expect(h.bins.at(-1)!.to).toBeGreaterThan(12);
  expect(logHistogram([]).bins).toEqual([]);
});

test('marker positions are monotonic and clamped to the bin axis', () => {
  const h = logHistogram([0.01, 0.1, 1, 10]);
  const a = binPosition(0.05, h.bins)!, b = binPosition(0.5, h.bins)!;
  expect(a).toBeLessThan(b);
  expect(binPosition(1e-9, h.bins)).toBe(0);
  expect(binPosition(1e9, h.bins)).toBe(h.bins.length);
  expect(binPosition(null, h.bins)).toBeNull();
  expect(binPosition(0, h.bins)).toBeNull();
});

test('flags need both an absolute count and a share of calls', () => {
  expect(insightFlags({ toolCalls: 10, toolFailures: 3, repeatedToolCalls: 0 })).toEqual(['failures']);
  expect(insightFlags({ toolCalls: 100, toolFailures: 3, repeatedToolCalls: 0 })).toEqual([]);
  expect(insightFlags({ toolCalls: 10, toolFailures: 0, repeatedToolCalls: 5 })).toEqual(['repeats']);
  expect(insightFlags({ toolCalls: 10, toolFailures: 2, repeatedToolCalls: 4 })).toEqual([]);
});

test('buildInsights aggregates medians, rates and ranked lists', () => {
  const rows = [
    row({ sessionId: 'a', estimatedCostUsd: 0.1, userTurns: 1, toolCalls: 4, toolFailures: 0 }),
    row({ sessionId: 'b', estimatedCostUsd: 5, userTurns: 9, toolCalls: 10, toolFailures: 5, repeatedToolCalls: 6, activeDurationMs: 600000 }),
    row({ sessionId: 'c', estimatedCostUsd: undefined, userTurns: 3, toolCalls: 2, toolFailures: 2 }),
  ];
  const r = buildInsights(rows, []);
  expect(r.kpis.sessions).toBe(3);
  expect(r.kpis.pricedSessions).toBe(2);
  expect(r.kpis.medianCostUsd).toBeCloseTo(2.55);
  expect(r.kpis.medianTurns).toBe(3);
  expect(r.kpis.failureRate).toBeCloseTo(7 / 16);
  expect(r.topCost.map(s => s.sessionId)).toEqual(['b', 'a']);
  // a session with fewer than three calls has no meaningful rate
  expect(r.topFailures.map(s => s.sessionId)).toEqual(['b']);
  expect(r.topRepeats.map(s => s.sessionId)).toEqual(['b']);
  expect(r.points.find(s => s.sessionId === 'b')!.flags).toEqual(['failures', 'repeats']);
  expect(r.truncated).toBe(false);
  expect(buildInsights(rows, [], 5).truncated).toBe(true);
});

test('formatting helpers stay readable at extremes', () => {
  expect(formatUsdCompact(0.004)).toBe('$0.0040');
  expect(formatUsdCompact(0.25)).toBe('$0.250');
  expect(formatUsdCompact(12.345)).toBe('$12.35');
  expect(formatUsdCompact(1234)).toBe('$1234');
  expect(formatRate(null)).toBe('—');
  expect(formatRate(0.04)).toBe('4.0%');
  expect(formatRate(0.4)).toBe('40%');
  expect(bubbleRadius(0)).toBeLessThan(bubbleRadius(5));
  expect(bubbleRadius(1000)).toBe(bubbleRadius(12));
});

test('History range and URL params produce the same Sessions filters', () => {
  const q = sessionFilters({ from: Date.UTC(2026, 0, 1), to: Date.UTC(2026, 0, 8) }, 'claude', '/work');
  expect(q).toMatchObject({ provider: 'claude', project: '/work', from: '2026-01-01T00:00:00.000Z', to: '2026-01-08T00:00:00.000Z', offset: 0 });
  expect(sessionFilters(null, '', null)).toMatchObject({ provider: null, project: null, from: null, to: null });
  const parsed = filtersFromParams(new URLSearchParams({ provider: 'codex', project: '/x', from: q.from!, to: '2026-02-03' }));
  expect(parsed).toMatchObject({ provider: 'codex', project: '/x', from: q.from });
  expect(Date.parse(parsed!.to!)).toBe(new Date('2026-02-03T00:00:00').getTime());
  expect(filtersFromParams(new URLSearchParams({ provider: 'nope', from: 'garbage' }))).toBeNull();
});

test('the preview mock serves insights that respect filters', async () => {
  const all = await sessionMockInvoke('get_session_insights', { query: {} }) as SessionInsights;
  expect(all.kpis.sessions).toBe(83);
  expect(all.points.length).toBe(83);
  expect(all.costHistogram.bins.length).toBeGreaterThan(1);
  expect(all.tools.length).toBeGreaterThan(0);
  const claude = await sessionMockInvoke('get_session_insights', { query: { provider: 'claude' } }) as SessionInsights;
  expect(claude.points.every(p => p.provider === 'claude')).toBe(true);
  expect(claude.kpis.sessions).toBeLessThan(all.kpis.sessions);
});
