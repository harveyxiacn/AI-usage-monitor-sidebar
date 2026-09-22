import { expect, test } from '@playwright/test';
import { buildQuotaHistory, type QuotaHistoryEvent } from '../src/lib/quota-history';
import type { QuotaSample } from '../src/lib/types';

function sample(minute: number, usedPercent: number, overrides: Partial<QuotaSample> = {}): QuotaSample {
  return {
    provider: 'codex', kind: 'five_hour', scope: null, usedPercent,
    resetsAt: '2026-09-22T01:00:00Z', plan: 'Pro',
    ts: new Date(Date.UTC(2026, 8, 22, 0, minute)).toISOString(), ...overrides,
  };
}

test('empty histories have no fabricated series or observations', () => {
  expect(buildQuotaHistory([])).toEqual([]);
});

test('a reset divides observed increases without estimating missing consumption', () => {
  const samples = [sample(0, 20), sample(30, 30),
    sample(61, 2, { resetsAt: '2026-09-22T06:00:00Z' }),
    sample(65, 7, { resetsAt: '2026-09-22T06:00:00Z' })];
  const [series] = buildQuotaHistory(samples);
  expect(series.entries.map(({ event, change }) => [event, change])).toEqual([
    ['initial', null], ['increase', 10], ['reset', -28], ['increase', 5],
  ]);
  expect(series.firstUsedPercent).toBe(20);
  expect(series.lastUsedPercent).toBe(7);
  expect(series.observedIncrease).toBe(15);
  expect(series.resetCount).toBe(1);
  expect(series.planChangeCount).toBe(0);
  expect(series.entries[0].previous).toBeNull();
  expect(series.entries[2].previous).toEqual(samples[1]);
});

test('same-cycle decreases are retained without inventing a reset', () => {
  const [series] = buildQuotaHistory([sample(0, 30), sample(1, 25), sample(2, 28), sample(3, 28)]);
  expect(series.entries.map(({ event, change }) => [event, change])).toEqual([
    ['initial', null], ['decrease', -5], ['increase', 3], ['unchanged', 0],
  ]);
  expect(series.observedIncrease).toBe(3);
  expect(series.resetCount).toBe(0);
});

const resetCases: Array<{
  label: string; minute: number; percent: number; oldReset: string | null;
  newReset: string | null; event: QuotaHistoryEvent;
}> = [
  { label: 'deadline advances before the old deadline with no decrease', minute: 30, percent: 20, oldReset: '2026-09-22T01:00:00Z', newReset: '2026-09-22T06:00:00Z', event: 'unchanged' },
  { label: 'deadline advances before the old deadline with increasing usage', minute: 30, percent: 25, oldReset: '2026-09-22T01:00:00Z', newReset: '2026-09-22T06:00:00Z', event: 'increase' },
  { label: 'deadline advances early with decreasing usage', minute: 30, percent: 2, oldReset: '2026-09-22T01:00:00Z', newReset: '2026-09-22T06:00:00Z', event: 'reset' },
  { label: 'equal percentage exactly at the old deadline', minute: 60, percent: 20, oldReset: '2026-09-22T01:00:00Z', newReset: '2026-09-22T06:00:00Z', event: 'reset' },
  { label: 'higher percentage after the old deadline', minute: 61, percent: 50, oldReset: '2026-09-22T01:00:00Z', newReset: '2026-09-22T06:00:00Z', event: 'reset' },
  { label: 'deadline moves backward', minute: 61, percent: 2, oldReset: '2026-09-22T01:00:00Z', newReset: '2026-09-22T00:30:00Z', event: 'decrease' },
  { label: 'equivalent offset spelling is not a new deadline', minute: 61, percent: 2, oldReset: '2026-09-22T01:00:00Z', newReset: '2026-09-22T09:00:00+08:00', event: 'decrease' },
  { label: 'missing previous deadline', minute: 61, percent: 2, oldReset: null, newReset: '2026-09-22T06:00:00Z', event: 'decrease' },
  { label: 'missing new deadline', minute: 61, percent: 2, oldReset: '2026-09-22T01:00:00Z', newReset: null, event: 'decrease' },
  { label: 'invalid previous deadline', minute: 61, percent: 2, oldReset: 'invalid', newReset: '2026-09-22T06:00:00Z', event: 'decrease' },
  { label: 'invalid new deadline', minute: 61, percent: 2, oldReset: '2026-09-22T01:00:00Z', newReset: 'invalid', event: 'decrease' },
];
for (const row of resetCases) {
  test(`reset evidence: ${row.label}`, () => {
    const [series] = buildQuotaHistory([
      sample(0, 20, { resetsAt: row.oldReset }),
      sample(row.minute, row.percent, { resetsAt: row.newReset }),
    ]);
    expect(series.entries[1].event).toBe(row.event);
    expect(series.entries[1].change).toBe(row.percent - 20);
    expect(series.resetCount).toBe(row.event === 'reset' ? 1 : 0);
    expect(series.observedIncrease).toBe(row.event === 'increase' ? row.percent - 20 : 0);
  });
}

test('plan transitions, including missing metadata, take precedence over resets and increases', () => {
  const [series] = buildQuotaHistory([
    sample(0, 20, { plan: null }), sample(1, 50),
    sample(61, 1, { plan: 'Plus', resetsAt: '2026-09-22T06:00:00Z' }),
    sample(62, 90, { plan: null, resetsAt: '2026-09-22T06:00:00Z' }),
    sample(63, 92, { plan: null, resetsAt: '2026-09-22T06:00:00Z' }),
  ]);
  expect(series.entries.map((entry) => entry.event)).toEqual([
    'initial', 'plan_change', 'plan_change', 'plan_change', 'increase',
  ]);
  expect(series.planChangeCount).toBe(3);
  expect(series.resetCount).toBe(0);
  expect(series.observedIncrease).toBe(2);
});

test('provider, quota kind and exact scope form separate deterministically ordered series', () => {
  const scopes = [null, '', 'Fable', 'Opus'];
  const rows = scopes.flatMap((scope, i) => [sample(2, i + 10, { scope }), sample(1, i, { scope })]);
  rows.push(sample(1, 90, { provider: 'claude' }), sample(1, 80, { kind: 'seven_day' }));
  const result = buildQuotaHistory(rows);
  const expectedKeys = [
    ...scopes.map((scope) => JSON.stringify(['codex', 'five_hour', scope])),
    JSON.stringify(['claude', 'five_hour', null]), JSON.stringify(['codex', 'seven_day', null]),
  ].sort((a, b) => a.localeCompare(b));
  expect(result.map((series) => series.key)).toEqual(expectedKeys);
  expect(result.filter((series) => series.samples.length === 2).map((series) => series.observedIncrease)).toEqual([10, 10, 10, 10]);
  expect(result.find((series) => series.provider === 'claude')!.lastUsedPercent).toBe(90);
  expect(result.find((series) => series.kind === 'seven_day')!.lastUsedPercent).toBe(80);
});

test('last input observation wins same-instant duplicates without merging groups or milliseconds', () => {
  const [series, other] = buildQuotaHistory([
    sample(2, 9), sample(0, 10), sample(0, 20, { ts: '2026-09-22T08:00:00+08:00' }),
    sample(0, 30, { ts: '2026-09-22T00:00:00.001Z' }), sample(0, 77, { scope: 'Fable' }),
    sample(0, NaN),
  ]).sort((a, b) => a.scope === null ? -1 : b.scope === null ? 1 : 0);
  expect(series.samples.map((row) => row.usedPercent)).toEqual([20, 30, 9]);
  expect(series.entries.map((entry) => entry.change)).toEqual([null, 10, -21]);
  expect(series.observedIncrease).toBe(10);
  expect(other.samples.map((row) => row.usedPercent)).toEqual([77]);
});

test('invalid input observations are excluded while endpoints and fractional percentages remain exact', () => {
  const invalidPercentages = [NaN, Infinity, -Infinity, -1, 101];
  const [series] = buildQuotaHistory([
    ...invalidPercentages.map((value, i) => sample(i, value)),
    sample(7, 20, { ts: 'invalid' }), sample(8, 20, { ts: '' }),
    sample(9, 0), sample(10, 0.125), sample(11, 100),
  ]);
  expect(series.samples.map((row) => row.usedPercent)).toEqual([0, 0.125, 100]);
  expect(series.entries.map((entry) => entry.change)).toEqual([null, 0.125, 99.875]);
  expect(series.observedIncrease).toBe(100);
  expect(buildQuotaHistory([sample(0, -1), sample(0, 1, { ts: 'invalid' })])).toEqual([]);
});

test('analysis accepts frozen inputs and never changes their order or fields', () => {
  const input = Object.freeze([Object.freeze(sample(2, 30)), Object.freeze(sample(0, 10)), Object.freeze(sample(1, 20))]);
  const before = JSON.stringify(input);
  const [series] = buildQuotaHistory(input);
  expect(series.samples.map((row) => row.usedPercent)).toEqual([10, 20, 30]);
  expect(series.observedIncrease).toBe(20);
  expect(JSON.stringify(input)).toBe(before);
  expect(input.map((row) => row.usedPercent)).toEqual([30, 10, 20]);
});
