// Plan advisor: "would a higher or a lower plan fit how you actually use it?"
// A pure function over data the History tab already has (quota cycles from the
// stored samples, the API-equivalent cost estimate, the subscription price the
// user typed in). No i18n here so `tests/plan-advisor.unit.ts` can import it in
// Node; `PlanAdvisor.svelte` turns the reason codes into text. [FRONTEND]
//
// It never claims a billing fact: prices are *hints* from `planTier`, the cost
// is the existing API-equivalent estimate, and the verdict is about limits
// being reached, not about money.
//
// Rules (constants below):
//   cycles    only completed, reliable cycles of the account-wide windows count
//             (`buildCycles` already marks a partial one unreliable)
//   data      < MIN_WEEKLY_CYCLES weekly cycles AND < MIN_FIVE_HOUR_CYCLES
//             five-hour cycles  ->  "insufficient"
//   upgrade   weekly limit reached in >= 2 cycles and >= 25 % of them, OR the
//             five-hour limit in >= 4 cycles and >= 20 % of them
//   downgrade no limit reached in any counted cycle, weekly median <= 40 % and
//             weekly peak <= 70 % (when weekly cycles exist), five-hour median
//             <= 40 % and peak <= 80 % (when five-hour cycles exist), a lower
//             tier is known, and the API-equivalent cost is not >= 2x the plan
//             price (that is a plan that is paying for itself)
//   keep      everything else
import { limitReachedCount, type QuotaCycle } from './quota-cycles';

export const MIN_WEEKLY_CYCLES = 3;
export const MIN_FIVE_HOUR_CYCLES = 12;
export const UPGRADE_WEEKLY = { hits: 2, rate: 0.25 } as const;
export const UPGRADE_FIVE_HOUR = { hits: 4, rate: 0.2 } as const;
export const DOWNGRADE_WEEKLY = { median: 40, peak: 70 } as const;
export const DOWNGRADE_FIVE_HOUR = { median: 40, peak: 80 } as const;
export const DOWNGRADE_MAX_COST_MULTIPLE = 2;
const DAYS_PER_MONTH = 30.4;

export interface PlanTier { name: string; priceUsd: number }

/** Plan ladders as price *hints*; the user confirms the real figure. */
const LADDERS: Readonly<Record<string, readonly PlanTier[]>> = {
  claude: [{ name: 'Pro', priceUsd: 20 }, { name: 'Max 5x', priceUsd: 100 }, { name: 'Max 20x', priceUsd: 200 }],
  codex: [{ name: 'Plus', priceUsd: 20 }, { name: 'Pro', priceUsd: 200 }],
};

/** Position of the user's plan on the ladder, from the plan label the provider reports. */
export function planTier(provider: string, plan: string | null | undefined): { ladder: readonly PlanTier[]; index: number } | null {
  const ladder = LADDERS[provider];
  const p = (plan ?? '').toLowerCase();
  if (!ladder || !p) return null;
  let index = -1;
  if (provider === 'claude') {
    index = /20\s*x/.test(p) ? 2 : /5\s*x/.test(p) || p.includes('max') ? 1 : p.includes('pro') ? 0 : -1;
  } else if (provider === 'codex') {
    index = p.includes('plus') ? 0 : p.includes('pro') ? 1 : -1;
  }
  return index < 0 ? null : { ladder, index };
}

export type PlanVerdict = 'upgrade' | 'downgrade' | 'keep' | 'insufficient';

/** Machine-readable reason; `PlanAdvisor.svelte` owns the wording (`history.plan.reason.<code>`). */
export interface PlanReason {
  code:
    | 'limitHits' | 'noLimitHits' | 'typical' | 'costMultiple' | 'priceUnset' | 'lowestTier'
    | 'tooFewCycles' | 'valueHeld' | 'tierUnknown';
  params: Record<string, string | number>;
}

export interface WindowStats {
  cycles: number;
  hits: number;
  median: number;
  peak: number;
}

export interface PlanAdvice {
  provider: string;
  verdict: PlanVerdict;
  reasons: PlanReason[];
  weekly: WindowStats | null;
  fiveHour: WindowStats | null;
  /** API-equivalent estimate scaled to 30.4 days; null when unknown */
  apiMonthlyUsd: number | null;
  /** apiMonthlyUsd / subscription price; null when either is unknown */
  costMultiple: number | null;
  currentTier: PlanTier | null;
  /** the tier a verdict points at, as a price hint */
  suggestedTier: PlanTier | null;
}

export interface PlanInput {
  provider: string;
  /** the provider's plan label ("Claude Max 5x") */
  planLabel: string | null;
  /** what the user pays per month (0 = unknown) */
  subscriptionUsd: number;
  weeklyCycles: readonly QuotaCycle[];
  fiveHourCycles: readonly QuotaCycle[];
  /** API-equivalent cost over `periodDays`; null when unknown (unpriced models) */
  apiCostUsd: number | null;
  periodDays: number;
}

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

function statsOf(cycles: readonly QuotaCycle[]): WindowStats | null {
  const counted = cycles.filter((c) => c.completed && c.reliable);
  if (counted.length === 0) return null;
  const peaks = counted.map((c) => Math.min(100, c.peak));
  return { cycles: counted.length, hits: limitReachedCount(counted), median: median(peaks), peak: Math.max(...peaks) };
}

const pct = (v: number) => Math.round(v);
const oneDecimal = (v: number) => Math.round(v * 10) / 10;

export function advisePlan(input: PlanInput): PlanAdvice {
  const weekly = statsOf(input.weeklyCycles);
  const fiveHour = statsOf(input.fiveHourCycles);
  const tier = planTier(input.provider, input.planLabel);
  const currentTier = tier ? tier.ladder[tier.index] : null;
  const price = Number.isFinite(input.subscriptionUsd) && input.subscriptionUsd > 0 ? input.subscriptionUsd : 0;
  const apiMonthlyUsd = input.apiCostUsd !== null && Number.isFinite(input.apiCostUsd) && input.periodDays > 0
    ? (input.apiCostUsd / input.periodDays) * DAYS_PER_MONTH : null;
  const costMultiple = apiMonthlyUsd !== null && price > 0 ? apiMonthlyUsd / price : null;
  const base = { provider: input.provider, weekly, fiveHour, apiMonthlyUsd, costMultiple, currentTier, suggestedTier: null as PlanTier | null };

  const enoughWeekly = (weekly?.cycles ?? 0) >= MIN_WEEKLY_CYCLES;
  const enoughFiveHour = (fiveHour?.cycles ?? 0) >= MIN_FIVE_HOUR_CYCLES;
  if (!enoughWeekly && !enoughFiveHour) {
    return {
      ...base,
      verdict: 'insufficient',
      reasons: [
        { code: 'tooFewCycles', params: { window: 'weekly', cycles: weekly?.cycles ?? 0, needed: MIN_WEEKLY_CYCLES } },
        { code: 'tooFewCycles', params: { window: 'fiveHour', cycles: fiveHour?.cycles ?? 0, needed: MIN_FIVE_HOUR_CYCLES } },
      ],
    };
  }

  const w = enoughWeekly ? weekly : null;
  const f = enoughFiveHour ? fiveHour : null;
  const reasons: PlanReason[] = [];
  const hitReasons = () => {
    for (const [window, s] of [['weekly', w], ['fiveHour', f]] as const) {
      if (s && s.hits > 0) reasons.push({ code: 'limitHits', params: { window, hits: s.hits, cycles: s.cycles, rate: pct((s.hits / s.cycles) * 100) } });
    }
  };
  const typicalReasons = () => {
    for (const [window, s] of [['weekly', w], ['fiveHour', f]] as const) {
      if (s) reasons.push({ code: 'typical', params: { window, median: pct(s.median), peak: pct(s.peak) } });
    }
  };
  const costReason = () => {
    if (price <= 0) reasons.push({ code: 'priceUnset', params: {} });
    else if (apiMonthlyUsd !== null && costMultiple !== null) {
      reasons.push({ code: 'costMultiple', params: { cost: apiMonthlyUsd, price, multiple: oneDecimal(costMultiple) } });
    }
  };

  const upgrade =
    (w !== null && w.hits >= UPGRADE_WEEKLY.hits && w.hits / w.cycles >= UPGRADE_WEEKLY.rate) ||
    (f !== null && f.hits >= UPGRADE_FIVE_HOUR.hits && f.hits / f.cycles >= UPGRADE_FIVE_HOUR.rate);
  if (upgrade) {
    hitReasons();
    typicalReasons();
    costReason();
    if (!tier) reasons.push({ code: 'tierUnknown', params: {} });
    const next = tier && tier.index + 1 < tier.ladder.length ? tier.ladder[tier.index + 1] : null;
    return { ...base, verdict: 'upgrade', reasons, suggestedTier: next };
  }

  const anyHits = (w?.hits ?? 0) + (f?.hits ?? 0) > 0;
  const lowUsage =
    !anyHits &&
    (w === null || (w.median <= DOWNGRADE_WEEKLY.median && w.peak <= DOWNGRADE_WEEKLY.peak)) &&
    (f === null || (f.median <= DOWNGRADE_FIVE_HOUR.median && f.peak <= DOWNGRADE_FIVE_HOUR.peak));
  if (!lowUsage) {
    hitReasons();
    typicalReasons();
    costReason();
    return { ...base, verdict: 'keep', reasons };
  }

  reasons.push({ code: 'noLimitHits', params: { cycles: (w?.cycles ?? 0) + (f?.cycles ?? 0) } });
  typicalReasons();
  costReason();
  if (!tier) {
    reasons.push({ code: 'tierUnknown', params: {} });
    return { ...base, verdict: 'keep', reasons };
  }
  if (tier.index === 0) {
    reasons.push({ code: 'lowestTier', params: {} });
    return { ...base, verdict: 'keep', reasons };
  }
  if (costMultiple !== null && costMultiple >= DOWNGRADE_MAX_COST_MULTIPLE) {
    reasons.push({ code: 'valueHeld', params: { multiple: oneDecimal(costMultiple) } });
    return { ...base, verdict: 'keep', reasons };
  }
  return { ...base, verdict: 'downgrade', reasons, suggestedTier: tier.ladder[tier.index - 1] };
}
