// Subscription ROI: what the month's usage would have cost at API prices
// compared with the flat subscription. Estimates only. [FRONTEND]

/** Prices shown as *hints*; the user confirms the real figure. */
export function planPriceHint(provider: string, plan: string | null | undefined): number | null {
  const p = (plan ?? '').toLowerCase();
  if (!p) return null;
  if (provider === 'claude') {
    if (/20\s*x/.test(p)) return 200;
    if (/5\s*x/.test(p)) return 100;
    if (p.includes('max')) return 100;
    if (p.includes('pro')) return 20;
    return null;
  }
  if (provider === 'codex') {
    if (p.includes('plus')) return 20;
    if (p.includes('pro')) return 200;
    return null;
  }
  return null;
}

/**
 * Sum of the configured prices of one provider, or of all when `provider` is
 * null. Each login is its own subscription, so the price of an extra account
 * is filed under its provider key (`claude@work`) next to the primary one
 * (`claude`). `account`: null = every account, '' = the primary account only,
 * an id = that extra account.
 */
export function subscriptionTotal(prices: Readonly<Record<string, number>>, provider: string | null, account: string | null = null): number {
  return Object.entries(prices).reduce<number>((sum, [key, v]) => {
    const at = key.indexOf('@');
    const [p, a] = at > 0 ? [key.slice(0, at), key.slice(at + 1)] : [key, ''];
    if (provider && p !== provider) return sum;
    if (account !== null && a !== account) return sum;
    return sum + (Number.isFinite(v) && v > 0 ? v : 0);
  }, 0);
}

export function daysInMonthOf(ms: number): number {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate();
}

export interface SubscriptionRoi {
  spentUsd: number;
  subscriptionUsd: number;
  /** spent / subscription */
  multiple: number;
  /** month-end projection at the current pace, null in the first hours of the month */
  projectedUsd: number | null;
  projectedMultiple: number | null;
}

/** `elapsed` is the elapsed share of the month (0..1). Null when no subscription price is set. */
export function subscriptionRoi(spentUsd: number, subscriptionUsd: number, elapsed: number): SubscriptionRoi | null {
  if (!Number.isFinite(subscriptionUsd) || subscriptionUsd <= 0) return null;
  if (!Number.isFinite(spentUsd) || spentUsd < 0) return null;
  const projectedUsd = elapsed >= 0.02 ? spentUsd / Math.min(1, elapsed) : null;
  return {
    spentUsd,
    subscriptionUsd,
    multiple: spentUsd / subscriptionUsd,
    projectedUsd,
    projectedMultiple: projectedUsd === null ? null : projectedUsd / subscriptionUsd,
  };
}

/**
 * Cumulative series continued at the current daily pace to the last day of the
 * month: nulls before the last elapsed day, then one value per remaining day.
 */
export function projectToMonthEnd(
  series: ReadonlyArray<{ date: string; cumulativeUsd: number }>,
  daysInMonth: number,
): Array<number | null> {
  const out: Array<number | null> = series.map(() => null);
  if (series.length === 0 || series.length >= daysInMonth) return out;
  const spent = series[series.length - 1].cumulativeUsd;
  const perDay = spent / series.length;
  out[series.length - 1] = spent;
  for (let day = series.length + 1; day <= daysInMonth; day++) out.push(perDay * day);
  return out;
}
