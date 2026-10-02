// How old the price table behind every "estimated cost" is. Pure helpers, so
// they can be unit-tested; the loader lives in stores/pricing-freshness.ts.

/** Older than this and the cost note gets a "may be outdated" remark. */
export const PRICING_STALE_DAYS = 60;

const DAY_MS = 86_400_000;

export interface PricingStaleness {
  /** `YYYY-MM-DD` of the newest of the table revision and the last source check. */
  date: string;
  ageDays: number;
}

/**
 * `null` while the date is unknown (nothing to say) or recent enough; otherwise
 * the date to quote. `checkedAt` is the last successful look at the price
 * source, `updatedAt` the revision of the table in use; the newer one counts,
 * because checking and finding nothing new also proves the table is current.
 */
export function pricingStaleness(
  updatedAt: string | null | undefined,
  checkedAt: string | null | undefined,
  nowMs: number
): PricingStaleness | null {
  const times = [updatedAt, checkedAt]
    .map((v) => (v ? Date.parse(v) : NaN))
    .filter((v) => Number.isFinite(v));
  if (times.length === 0) return null;
  const newest = Math.max(...times);
  const ageDays = Math.floor((nowMs - newest) / DAY_MS);
  if (ageDays <= PRICING_STALE_DAYS) return null;
  return { date: new Date(newest).toISOString().slice(0, 10), ageDays };
}
