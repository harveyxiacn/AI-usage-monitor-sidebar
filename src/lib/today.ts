// "Today" summary of one provider for the Overview cards, derived from an
// hourly `get_usage_history` result so no extra backend aggregate is needed.
// Pure (no i18n) for unit tests. [FRONTEND]
import { providerKey } from './accounts';
import type { HistoryResult, TokenTotals } from './types';

export interface TodaySummary {
  totals: TokenTotals;
  /** total tokens per local hour of the day, 24 slots */
  hours: number[];
  /** cache reads as a share of all input-side tokens, 0..1; null when there is no input */
  cacheHitRate: number | null;
}

/** Share of input-side tokens served from the prompt cache. */
export function cacheHitRate(t: Pick<TokenTotals, 'inputTokens' | 'cacheReadTokens' | 'cacheWriteTokens'>): number | null {
  const input = t.inputTokens + t.cacheReadTokens + t.cacheWriteTokens;
  return input > 0 ? t.cacheReadTokens / input : null;
}

/**
 * One summary per provider key (`claude`, `claude@work`) that has usage in
 * `result` (an hourly query over today). Without any extra account this is
 * keyed by provider exactly as before; once one has usage, `byAccount` splits
 * the provider's totals so the primary card does not include its tokens.
 */
export function todaySummaries(result: HistoryResult): Map<string, TodaySummary> {
  const out = new Map<string, TodaySummary>();
  const split = result.byAccount && Object.keys(result.byAccount).length > 0 ? result.byAccount : null;
  for (const [key, totals] of Object.entries(split ?? result.byProvider)) {
    out.set(key, { totals, hours: Array.from({ length: 24 }, () => 0), cacheHitRate: cacheHitRate(totals) });
  }
  for (const row of result.rows) {
    const summary = out.get(split ? providerKey(row.provider, row.account) : row.provider);
    const hour = new Date(row.bucketStart).getHours();
    if (summary && Number.isInteger(hour)) summary.hours[hour] += row.totalTokens;
  }
  return out;
}
