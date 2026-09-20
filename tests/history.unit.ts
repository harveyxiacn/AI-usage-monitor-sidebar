import { test, expect } from '@playwright/test';
import { csvCell, historyCsv, historyRange, localDateInput } from '../src/lib/history';
import type { HistoryRow } from '../src/lib/types';

test('custom ranges include the entire final local day and reject invalid dates', () => {
  const range = historyRange('custom', '2026-09-19', '2026-09-20')!;
  expect(localDateInput(range.from)).toBe('2026-09-19');
  expect(localDateInput(range.to - 1)).toBe('2026-09-20');
  expect(new Date(range.to).getHours()).toBe(0);
  expect(localDateInput(range.to)).toBe('2026-09-21');
  for (const [from, to] of [['', '2026-09-20'], ['2026-02-30', '2026-03-02'], ['2026-09-21', '2026-09-20']]) {
    expect(historyRange('custom', from, to)).toBeNull();
  }
});

test('presets contain seven calendar days including today', () => {
  const now = new Date(2026, 8, 20, 15, 30).getTime();
  const range = historyRange('7d', '', '', now)!;
  expect(localDateInput(range.from)).toBe('2026-09-14');
  expect(new Date(range.from).getHours()).toBe(0);
  expect(range.to).toBe(now);
});

test('calendar ranges survive daylight saving transitions', () => {
  const original = process.env.TZ;
  process.env.TZ = 'America/New_York';
  try {
    const spring = historyRange('custom', '2026-03-08', '2026-03-08')!;
    const fall = historyRange('custom', '2026-11-01', '2026-11-01')!;
    expect(spring.to - spring.from).toBe(23 * 3_600_000);
    expect(fall.to - fall.from).toBe(25 * 3_600_000);
  } finally {
    if (original === undefined) delete process.env.TZ;
    else process.env.TZ = original;
  }
});

test('CSV preserves delimiters, unknown cost and neutralizes formula cells', () => {
  expect(csvCell('hello,"world"\r\nnext')).toBe('"hello,""world""\r\nnext"');
  for (const value of ['=1+1', '+cmd', '-cmd', '@SUM(A1)', '\t=1+1']) {
    expect(csvCell(value)).toBe(`'${value}`);
  }
  expect(csvCell(-42)).toBe('-42');
  const row: HistoryRow = {
    bucketStart: '2026-09-20T00:00:00Z', provider: 'claude', model: '模型,example',
    inputTokens: 100, cacheWriteTokens: 20, cacheReadTokens: 30, outputTokens: 40,
    reasoningTokens: 10, totalTokens: 190, requests: 1, estimatedCostUsd: null,
  };
  const csv = historyCsv([row]);
  expect(csv).toContain('"模型,example",100,20,30,40,10,190,1,\r\n');
  expect(csv.split('\r\n')).toHaveLength(3);
});
