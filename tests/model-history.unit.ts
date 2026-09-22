import { test, expect } from '@playwright/test';
import { historyCsv, historySeries, modelVariantLabel, sessionModelVariants, sessionsCsv } from '../src/lib/history';
import type { HistoryRow, SessionRow } from '../src/lib/types';

const row = (reasoningEffort: string | null | undefined, totalTokens: number, model = 'gpt-6-astra'): HistoryRow => ({
  bucketStart: '2026-09-22T00:00:00Z', provider: 'codex', model, reasoningEffort, project: '/work',
  inputTokens: totalTokens, cacheReadTokens: 0, cacheWriteTokens: 0, outputTokens: 0,
  reasoningTokens: 0, totalTokens, requests: 1, estimatedCostUsd: totalTokens / 10,
});

const session = (): SessionRow => ({
  ...row(null, 43), sessionId: 'session-a', project: '/work',
  firstTs: '2026-09-22T00:00:00Z', lastTs: '2026-09-22T01:00:00Z', durationMs: 3_600_000,
  models: ['gpt-6-astra', 'gpt-5.6-sol'],
});

test('model labels retain exact API names and recorded effort without inferred aliases', () => {
  expect(modelVariantLabel('gpt-6-astra', 'medium')).toBe('gpt-6-astra · medium');
  expect(modelVariantLabel('gpt-6-astra', 'ultra')).toBe('gpt-6-astra · ultra');
  expect(modelVariantLabel('gpt-5.6-sol', 'xhigh')).toBe('gpt-5.6-sol · xhigh');
  expect(modelVariantLabel('claude-opus-5-20260514', 'high')).toBe('claude-opus-5-20260514 · high');
  expect(modelVariantLabel('claude-fable-5', null, '未知模型', '推理强度未记录')).toBe('claude-fable-5 · 推理强度未记录');
  expect(modelVariantLabel('gpt-6-astra', undefined, 'Unknown', 'Effort not recorded')).toBe('gpt-6-astra · Effort not recorded');
  expect(modelVariantLabel(null, null, '未知模型', '推理强度未记录')).toBe('未知模型 · 推理强度未记录');
});

test('model grouping keeps medium, ultra and missing effort separate with exact token totals', () => {
  const rows = [row('medium', 10), row('ultra', 20), row(null, 5), row(undefined, 8), row('xhigh', 7, 'gpt-5.6-sol')];
  const before = structuredClone(rows);
  const { series } = historySeries(rows, true, false, 'tokens');
  expect(series.map((s) => [s.model, s.reasoningEffort, s.values])).toEqual([
    ['gpt-5.6-sol', 'xhigh', [7]],
    ['gpt-6-astra', 'medium', [10]],
    ['gpt-6-astra', 'ultra', [20]],
    ['gpt-6-astra', null, [13]],
  ]);
  expect(new Set(series.map((s) => s.key)).size).toBe(4);
  expect(rows).toEqual(before);
});

test('provider grouping collapses effort without changing tokens or raw-model pricing totals', () => {
  const rows = [row('medium', 10), row('ultra', 20), row(null, 5), row(undefined, 8)];
  const tokens = historySeries(rows, false, false, 'tokens').series;
  expect(tokens.map((s) => [s.model, s.reasoningEffort, s.values])).toEqual([[null, null, [43]]]);
  expect(historySeries(rows, false, false, 'cost').series[0].values[0]).toBeCloseTo(4.3, 8);
});

test('effort identities cannot collide with model names or exact project paths', () => {
  const rows = [
    { ...row('ultra', 1, 'astra · medium'), project: '/a' },
    { ...row('medium · ultra', 2, 'astra'), project: '/a' },
    { ...row('ultra', 4, 'astra · medium'), project: '/b' },
  ];
  const { series } = historySeries(rows, true, true, 'tokens');
  expect(new Set(series.map((s) => s.key)).size).toBe(3);
  expect(series.map((s) => s.values[0]).sort()).toEqual([1, 2, 4]);
});

test('sessions show every observed model and effort switch without adding fictitious unknown effort', () => {
  const data = session();
  data.modelVariants = [
    { model: 'gpt-6-astra', reasoningEffort: 'ultra' },
    { model: 'gpt-6-astra', reasoningEffort: 'medium' },
    { model: 'gpt-6-astra', reasoningEffort: 'ultra' },
    { model: 'gpt-5.6-sol', reasoningEffort: 'xhigh' },
  ];
  expect(sessionModelVariants(data)).toEqual([
    { model: 'gpt-5.6-sol', reasoningEffort: 'xhigh' },
    { model: 'gpt-6-astra', reasoningEffort: 'medium' },
    { model: 'gpt-6-astra', reasoningEffort: 'ultra' },
  ]);
});

test('legacy sessions and partially populated metadata keep raw model names with unknown effort', () => {
  const data = session();
  expect(sessionModelVariants(data)).toEqual([
    { model: 'gpt-5.6-sol', reasoningEffort: null },
    { model: 'gpt-6-astra', reasoningEffort: null },
  ]);
  data.modelVariants = [{ model: 'gpt-6-astra', reasoningEffort: 'medium' }, { model: 'gpt-6-astra', reasoningEffort: null }];
  expect(sessionModelVariants(data)).toEqual([
    { model: 'gpt-5.6-sol', reasoningEffort: null },
    { model: 'gpt-6-astra', reasoningEffort: null },
    { model: 'gpt-6-astra', reasoningEffort: 'medium' },
  ]);
});

test('bucket CSV exports raw effort, separates missing values, and escapes formula prefixes', () => {
  const csv = historyCsv([row('medium', 10), row(null, 20), row('=1+1', 30)]);
  expect(csv.split('\r\n')[0]).toContain('provider,model,reasoning_effort,project');
  expect(csv).toContain(',codex,gpt-6-astra,medium,/work,10,');
  expect(csv).toContain(',codex,gpt-6-astra,,/work,20,');
  expect(csv).toContain(",codex,gpt-6-astra,'=1+1,/work,30,");
  expect(csv).not.toContain('Effort not recorded');
});

test('session CSV retains raw models and exports all variants as reversible JSON', () => {
  const data = session();
  data.modelVariants = [
    { model: 'gpt-6-astra', reasoningEffort: 'medium' },
    { model: 'gpt-6-astra', reasoningEffort: 'ultra' },
    { model: 'gpt-5.6-sol', reasoningEffort: 'xhigh' },
  ];
  const [header, line] = sessionsCsv([data]).split('\r\n');
  expect(header).toContain('models,model_variants,input_tokens');
  expect(line).toContain(',gpt-6-astra gpt-5.6-sol,');
  // Read the quoted JSON cell independently of the exporter.
  const quoted = line.match(/,"(\[.*\])",43,/)![1].replace(/""/g, '"');
  expect(JSON.parse(quoted)).toEqual([
    { model: 'gpt-5.6-sol', reasoningEffort: 'xhigh' },
    { model: 'gpt-6-astra', reasoningEffort: 'medium' },
    { model: 'gpt-6-astra', reasoningEffort: 'ultra' },
  ]);
});
