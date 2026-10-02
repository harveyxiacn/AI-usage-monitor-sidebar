// "Today" summary of one provider for the Overview cards, derived from an
// hourly `get_usage_history` result so no extra backend aggregate is needed.
// Pure (no i18n) for unit tests. [FRONTEND]
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

/** One summary per provider that has usage in `result` (an hourly query over today). */
export function todaySummaries(result: HistoryResult): Map<string, TodaySummary> {
  const out = new Map<string, TodaySummary>();
  for (const [provider, totals] of Object.entries(result.byProvider)) {
    out.set(provider, { totals, hours: Array.from({ length: 24 }, () => 0), cacheHitRate: cacheHitRate(totals) });
  }
  for (const row of result.rows) {
    const summary = out.get(row.provider);
    const hour = new Date(row.bucketStart).getHours();
    if (summary && Number.isInteger(hour)) summary.hours[hour] += row.totalTokens;
  }
  return out;
}
