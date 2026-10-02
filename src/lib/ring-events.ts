// Change detection behind the sidebar's one-shot micro-animations: compares the
// previous and the next snapshot of every window and says what just happened.
// Pure, so `tests/ring-events.unit.ts` covers it; never fires on first load.
// [FRONTEND]
import { quotaKey } from './providers';
import { severityOf, type Severity } from './severity';
import type { AppSnapshot, ProviderId, QuotaWindow, Thresholds } from './types';

export type RingEventKind = 'warn' | 'critical' | 'reset';

export interface RingEvent {
  /** `windowKey()` of the window that changed */
  key: string;
  kind: RingEventKind;
}

export interface WindowLevel {
  percent: number;
  severity: Severity;
}

/** A drop of at least this many points (from at least `RESET_MIN_PREVIOUS`) is a reset. */
export const RESET_MIN_DROP = 25;
export const RESET_MIN_PREVIOUS = 30;

export function windowKey(provider: ProviderId, w: Pick<QuotaWindow, 'kind' | 'scope'>): string {
  return `${provider}|${w.kind}|${w.scope ?? ''}`;
}

/** Everything the next comparison needs: used % and severity per window. */
export function levelsOf(snap: AppSnapshot | null, th: Thresholds): Map<string, WindowLevel> {
  const out = new Map<string, WindowLevel>();
  for (const q of snap?.providers ?? []) {
    for (const w of q.windows) {
      if (w.usedPercent == null || !Number.isFinite(w.usedPercent)) continue;
      out.set(windowKey(quotaKey(q), w), { percent: w.usedPercent, severity: severityOf(w.usedPercent, th) });
    }
  }
  return out;
}

const RANK: Record<Severity, number> = { normal: 0, warn: 1, critical: 2 };

/**
 * Events between two level maps. `prev === null` is the first load: nothing
 * animates. A window that is new in `next` has nothing to compare with and is
 * ignored too, so is one whose previous value is unknown.
 */
export function detectRingEvents(prev: Map<string, WindowLevel> | null, next: Map<string, WindowLevel>): RingEvent[] {
  if (!prev) return [];
  const events: RingEvent[] = [];
  for (const [key, now] of next) {
    const before = prev.get(key);
    if (!before) continue;
    if (before.percent >= RESET_MIN_PREVIOUS && before.percent - now.percent >= RESET_MIN_DROP) {
      events.push({ key, kind: 'reset' });
    } else if (RANK[now.severity] > RANK[before.severity]) {
      events.push({ key, kind: now.severity === 'critical' ? 'critical' : 'warn' });
    }
  }
  return events;
}
