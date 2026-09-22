import { expect, test } from '@playwright/test';
import { mockInvoke } from '../src/lib/mock';
import type { HistoryQuery, HistoryResult, QuotaSample, SessionsResult } from '../src/lib/types';

const now = Date.now();
const query: HistoryQuery = {
  from: new Date(now - 7 * 86_400_000).toISOString(), to: new Date(now + 1).toISOString(),
  bucket: 'day', provider: 'codex', project: null, groupByModel: true, groupByProject: false,
};

test('preview history keeps model efforts distinct and exercises family pricing', async () => {
  const grouped = await mockInvoke<HistoryResult>('get_usage_history', { query });
  expect(grouped.rows.length).toBeGreaterThan(0);
  expect(new Set(grouped.rows.map((row) => row.provider))).toEqual(new Set(['codex']));
  const variants = new Set(grouped.rows.map((row) => `${row.model}:${row.reasoningEffort}`));
  for (const variant of ['gpt-6-astra:medium', 'gpt-6-astra:ultra', 'gpt-5.6-sol:xhigh', 'gpt-6-astra:null']) {
    expect(variants.has(variant), variant).toBe(true);
  }
  const spark = grouped.rows.filter((row) => row.model === 'gpt-5.3-codex-spark');
  expect(spark.length).toBeGreaterThan(0);
  expect(spark.every((row) => row.estimatedCostUsd !== null && row.totalTokens > 0)).toBe(true);
  expect(grouped.costApproximate).toBe(true);
});

test('model effort grouping preserves exact token and request totals', async () => {
  const grouped = await mockInvoke<HistoryResult>('get_usage_history', { query });
  const ungrouped = await mockInvoke<HistoryResult>('get_usage_history', { query: { ...query, groupByModel: false } });
  expect(ungrouped.totals).toEqual(grouped.totals);
  expect(ungrouped.rows.every((row) => row.model === null && row.reasoningEffort === null)).toBe(true);
  for (const key of ['totalTokens', 'inputTokens', 'outputTokens', 'requests'] as const) {
    expect(grouped.rows.reduce((sum, row) => sum + row[key], 0)).toBe(ungrouped.totals[key]);
  }
});

test('session variants retain known and unrecorded effort while agreeing with history', async () => {
  const history = await mockInvoke<HistoryResult>('get_usage_history', { query });
  const sessions = await mockInvoke<SessionsResult>('get_usage_sessions', { query: { ...query, limit: 1000 } });
  expect(sessions.rows.length).toBeGreaterThan(0);
  expect(sessions.truncated).toBe(false);
  expect(sessions.totals.totalTokens).toBe(history.totals.totalTokens);
  expect(sessions.totals.requests).toBe(history.totals.requests);
  for (const row of sessions.rows) {
    expect(row.provider).toBe('codex');
    expect(new Set(row.modelVariants!.map((variant) => variant.model))).toEqual(new Set(row.models));
  }
  const efforts = new Set(sessions.rows.flatMap((row) => row.modelVariants!.map((variant) => variant.reasoningEffort)));
  expect(efforts).toEqual(new Set(['medium', 'ultra', 'xhigh', null]));
});

test('quota preview filters the provider while retaining separate quota windows', async () => {
  const rows = await mockInvoke<QuotaSample[]>('get_quota_history', { query });
  expect(rows.length).toBeGreaterThan(0);
  expect(new Set(rows.map((row) => row.provider))).toEqual(new Set(['codex']));
  expect(new Set(rows.map((row) => row.kind))).toEqual(new Set(['five_hour', 'seven_day']));
  expect(rows.every((row) => row.usedPercent >= 0 && row.usedPercent <= 100)).toBe(true);
  expect(rows.every((row) => Date.parse(row.ts) >= Date.parse(query.from) && Date.parse(row.ts) < Date.parse(query.to))).toBe(true);
});
