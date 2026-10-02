// Loads the price table's age once and shares it between every cost note.
import { getPriceUpdateStatus, getPricing } from '$lib/api';
import { pricingStaleness, type PricingStaleness } from '$lib/pricing-age';

const TTL_MS = 5 * 60_000;
let cached: { at: number; value: Promise<PricingStaleness | null> } | null = null;

/** Resolves to the staleness to show, or `null` (fresh, unknown or on any error). */
export function loadPricingStaleness(now = Date.now()): Promise<PricingStaleness | null> {
  if (cached && now - cached.at < TTL_MS) return cached.value;
  const value = (async () => {
    try {
      const [table, status] = await Promise.all([getPricing(), getPriceUpdateStatus()]);
      return pricingStaleness(table.updatedAt, status.error ? null : status.checkedAt, Date.now());
    } catch {
      return null;
    }
  })();
  cached = { at: now, value };
  return value;
}
