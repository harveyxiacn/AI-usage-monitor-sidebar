import { expect, test } from '@playwright/test';
import { advisePlan, planTier, type PlanInput } from '../src/lib/plan-advisor';
import type { QuotaCycle } from '../src/lib/quota-cycles';

/** n completed, reliable cycles with the given peaks (repeated cyclically) */
function cycles(n: number, peaks: number[]): QuotaCycle[] {
  return Array.from({ length: n }, (_, i) => {
    const peak = peaks[i % peaks.length];
    return { start: i * 1000, end: i * 1000 + 999, peak, samples: 10, completed: true, limitReached: peak >= 100, reliable: true };
  });
}

function input(over: Partial<PlanInput> = {}): PlanInput {
  return {
    provider: 'claude', planLabel: 'Claude Max 5x', subscriptionUsd: 100,
    weeklyCycles: cycles(8, [50]), fiveHourCycles: cycles(40, [50]),
    apiCostUsd: 200, periodDays: 56, ...over,
  };
}
const codes = (a: ReturnType<typeof advisePlan>) => a.reasons.map((r) => r.code);

test('plan tiers come from the plan label and are only hints', () => {
  expect(planTier('claude', 'Claude Max 20x')?.index).toBe(2);
  expect(planTier('claude', 'Claude Max 5x')?.index).toBe(1);
  expect(planTier('claude', 'Pro')?.index).toBe(0);
  expect(planTier('codex', 'ChatGPT Plus')?.index).toBe(0);
  expect(planTier('codex', 'ChatGPT Pro')?.index).toBe(1);
  expect(planTier('claude', null)).toBeNull();
  expect(planTier('claude', 'Team')).toBeNull();
  expect(planTier('openrouter', 'anything')).toBeNull();
});

test('not enough completed cycles means "insufficient", with the counts', () => {
  const a = advisePlan(input({ weeklyCycles: cycles(2, [100]), fiveHourCycles: cycles(5, [100]) }));
  expect(a.verdict).toBe('insufficient');
  expect(codes(a)).toEqual(['tooFewCycles', 'tooFewCycles']);
  expect(a.reasons[0].params).toMatchObject({ cycles: 2, needed: 3 });
  // no data at all
  expect(advisePlan(input({ weeklyCycles: [], fiveHourCycles: [] })).verdict).toBe('insufficient');
  // running or partial cycles do not count
  const partial = cycles(10, [100]).map((c) => ({ ...c, reliable: false }));
  expect(advisePlan(input({ weeklyCycles: partial, fiveHourCycles: [] })).verdict).toBe('insufficient');
  const running = cycles(10, [100]).map((c) => ({ ...c, completed: false }));
  expect(advisePlan(input({ weeklyCycles: running, fiveHourCycles: [] })).verdict).toBe('insufficient');
});

test('repeated weekly limit hits recommend a higher plan and point at the next tier as a hint', () => {
  const a = advisePlan(input({ weeklyCycles: cycles(8, [100, 60, 55, 100]) }));
  expect(a.verdict).toBe('upgrade');
  expect(a.reasons[0]).toEqual({ code: 'limitHits', params: { window: 'weekly', hits: 4, cycles: 8, rate: 50 } });
  expect(a.suggestedTier).toEqual({ name: 'Max 20x', priceUsd: 200 });
  expect(codes(a)).toContain('costMultiple');
});

test('frequent five-hour limit hits recommend a higher plan on their own', () => {
  const a = advisePlan(input({ weeklyCycles: cycles(8, [50]), fiveHourCycles: cycles(40, [100, 40, 40, 40, 40]) }));
  expect(a.verdict).toBe('upgrade');
  expect(a.reasons.find((r) => r.code === 'limitHits')?.params.window).toBe('fiveHour');
});

test('a single weekly hit, or a rare five-hour hit, is not enough for an upgrade', () => {
  expect(advisePlan(input({ weeklyCycles: cycles(8, [100, 50, 50, 50, 50, 50, 50, 50]) })).verdict).toBe('keep');
  expect(advisePlan(input({ fiveHourCycles: cycles(40, [100, ...Array(19).fill(40)]) })).verdict).toBe('keep');
  // two hits but under 25 % of many cycles
  expect(advisePlan(input({ weeklyCycles: cycles(12, [100, 100, ...Array(10).fill(40)]) })).verdict).toBe('keep');
});

test('top tier has nothing to upgrade to', () => {
  const a = advisePlan(input({ planLabel: 'Claude Max 20x', weeklyCycles: cycles(8, [100]) }));
  expect(a.verdict).toBe('upgrade');
  expect(a.suggestedTier).toBeNull();
});

test('a quiet history on a higher tier suggests the lower tier', () => {
  const a = advisePlan(input({ weeklyCycles: cycles(8, [20, 30, 25]), fiveHourCycles: cycles(40, [10, 30, 20]), apiCostUsd: 100 }));
  expect(a.verdict).toBe('downgrade');
  expect(a.suggestedTier).toEqual({ name: 'Pro', priceUsd: 20 });
  expect(codes(a)).toEqual(expect.arrayContaining(['noLimitHits', 'typical', 'costMultiple']));
});

test('a downgrade is withheld when the plan clearly pays for itself, is the lowest tier, or is unknown', () => {
  const quiet = { weeklyCycles: cycles(8, [20, 30]), fiveHourCycles: cycles(40, [10, 30]) };
  // API-equivalent 6x the plan price
  const rich = advisePlan(input({ ...quiet, apiCostUsd: 1200 }));
  expect(rich.verdict).toBe('keep');
  expect(codes(rich)).toContain('valueHeld');
  const lowest = advisePlan(input({ ...quiet, planLabel: 'Claude Pro', subscriptionUsd: 20 }));
  expect(lowest.verdict).toBe('keep');
  expect(codes(lowest)).toContain('lowestTier');
  const unknown = advisePlan(input({ ...quiet, planLabel: null }));
  expect(unknown.verdict).toBe('keep');
  expect(codes(unknown)).toContain('tierUnknown');
});

test('moderate use stays on the current plan; any limit hit blocks a downgrade', () => {
  expect(advisePlan(input({ weeklyCycles: cycles(8, [60, 75]) })).verdict).toBe('keep');
  const hit = advisePlan(input({ weeklyCycles: cycles(8, [20, 20, 20, 20, 20, 20, 20, 100]), fiveHourCycles: cycles(40, [10]) }));
  expect(hit.verdict).toBe('keep');
  expect(codes(hit)).toContain('limitHits');
});

test('cost is scaled to a month and compared with the entered price; unknown price is said so', () => {
  const a = advisePlan(input({ apiCostUsd: 280, periodDays: 56, subscriptionUsd: 100 }));
  expect(a.apiMonthlyUsd).toBeCloseTo(152, 0);
  expect(a.costMultiple).toBeCloseTo(1.52, 1);
  const unset = advisePlan(input({ subscriptionUsd: 0 }));
  expect(unset.costMultiple).toBeNull();
  expect(codes(unset)).toContain('priceUnset');
  const unpriced = advisePlan(input({ apiCostUsd: null }));
  expect(unpriced.apiMonthlyUsd).toBeNull();
  expect(codes(unpriced)).not.toContain('costMultiple');
});

test('only the weekly data is needed when five-hour history is short', () => {
  const a = advisePlan(input({ weeklyCycles: cycles(4, [100, 100, 60, 60]), fiveHourCycles: cycles(3, [30]) }));
  expect(a.verdict).toBe('upgrade');
  expect(a.fiveHour?.cycles).toBe(3);
});
