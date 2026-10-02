// Pure countdown arithmetic behind "Resets in …" and the popover's re-render
// clock, split out of `format.ts` so it can be unit-tested without the i18n
// catalogues. [FRONTEND]

const SECOND = 1_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

export type ResetParts =
  | { kind: 'unknown' }
  | { kind: 'min'; m: number }
  | { kind: 'hourMin'; h: number; m: number }
  /** more than 24 h away: an absolute moment plus a coarse "3 d 4 h" relative */
  | { kind: 'at'; ts: number; d: number; h: number };

/** Breaks a reset moment into the pieces `formatReset` words. */
export function resetParts(resetsAt: string | null | undefined, now: number): ResetParts {
  if (!resetsAt) return { kind: 'unknown' };
  const ts = Date.parse(resetsAt);
  if (!Number.isFinite(ts)) return { kind: 'unknown' };
  const diff = ts - now;
  if (diff < DAY) {
    const totalMin = Math.max(0, Math.ceil(diff / MINUTE));
    if (totalMin < 60) return { kind: 'min', m: totalMin };
    return { kind: 'hourMin', h: Math.floor(totalMin / 60), m: totalMin % 60 };
  }
  // rounded down to the hour: "3 d 4 h" never overstates how soon it resets
  const totalH = Math.floor(diff / HOUR);
  return { kind: 'at', ts, d: Math.floor(totalH / 24), h: totalH % 24 };
}

/**
 * Milliseconds until the popover's clock has to tick again. Everything it shows
 * changes on a minute boundary *relative to its own anchor* (a reset moment
 * counts down to it, an "updated … ago" label counts up from the fetch), except
 * the "12 s ago" label of a fresh fetch, which needs a one-second tick for its
 * first minute. Never below 250 ms, never above a minute.
 */
export function nextTickDelay(now: number, anchors: Array<number | null | undefined>, fetchedAt?: number | null): number {
  if (fetchedAt != null && Number.isFinite(fetchedAt) && now - fetchedAt < MINUTE) return SECOND;
  let delay = MINUTE;
  for (const a of [...anchors, fetchedAt]) {
    if (a == null || !Number.isFinite(a)) continue;
    const toBoundary = (((a - now) % MINUTE) + MINUTE) % MINUTE;
    // +50 ms so the tick lands just past the boundary, never just before it
    delay = Math.min(delay, (toBoundary === 0 ? MINUTE : toBoundary) + 50);
  }
  return Math.max(250, Math.min(MINUTE, delay));
}

/**
 * Terse, locale-free countdown for the sidebar label: "45m", "1h12", "3d4h".
 * Empty string when the reset moment is unknown. Rounds the same way as
 * `resetParts`, so the bar and the popover never disagree by a minute.
 */
export function compactReset(resetsAt: string | null | undefined, now: number): string {
  const p = resetParts(resetsAt, now);
  switch (p.kind) {
    case 'unknown':
      return '';
    case 'min':
      return `${p.m}m`;
    case 'hourMin':
      return `${p.h}h${String(p.m).padStart(2, '0')}`;
    case 'at':
      return `${p.d}d${p.h}h`;
  }
}
