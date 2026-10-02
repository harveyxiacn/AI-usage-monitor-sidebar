import { expect, test } from '@playwright/test';
import { cacheHitRate, todaySummaries } from '../src/lib/today';
import type { HistoryResult, HistoryRow, TokenTotals } from '../src/lib/types';

const totals = (over: Partial<TokenTotals> = {}): TokenTotals => ({
  inputTokens: 0, cacheWriteTokens: 0, cacheReadTokens: 0, outputTokens: 0, reasoningTokens: 0,
  totalTokens: 0, requests: 0, estimatedCostUsd: null, ...over,
});

test('cache hit rate is cache reads over all input-side tokens', () => {
  expect(cacheHitRate(totals())).toBeNull();
  expect(cacheHitRate(totals({ inputTokens: 100, cacheReadTokens: 300, cacheWriteTokens: 100 }))).toBeCloseTo(0.6);
  expect(cacheHitRate(totals({ inputTokens: 10 }))).toBe(0);
});

test('hourly rows fill the right local-hour slots per provider', () => {
  const at = (hour: number) => new Date(2026, 8, 22, hour, 0, 0).toISOString();
  const row = (provider: string, hour: number, totalTokens: number): HistoryRow => ({
    ...totals({ totalTokens }), bucketStart: at(hour), provider, model: null, reasoningEffort: null, project: null,
  });
  const result: HistoryResult = {
    rows: [row('claude', 9, 100), row('claude', 9, 50), row('claude', 23, 7), row('codex', 0, 5)],
    totals: totals({ totalTokens: 162 }),
    byProvider: {
      claude: totals({ totalTokens: 157, inputTokens: 50, cacheReadTokens: 50, estimatedCostUsd: 1.5 }),
      codex: totals({ totalTokens: 5 }),
    },
    projects: [],
    costApproximate: false,
  };
  const map = todaySummaries(result);
  const claude = map.get('claude')!;
  expect(claude.hours).toHaveLength(24);
  expect(claude.hours[9]).toBe(150);
  expect(claude.hours[23]).toBe(7);
  expect(claude.cacheHitRate).toBeCloseTo(0.5);
  expect(map.get('codex')!.hours[0]).toBe(5);
  expect(map.get('codex')!.cacheHitRate).toBeNull();
  expect(map.has('copilot')).toBe(false);
});
