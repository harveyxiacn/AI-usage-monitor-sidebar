import type { QuotaSample } from './types';

export type QuotaHistoryEvent = 'initial' | 'increase' | 'decrease' | 'reset' | 'plan_change' | 'unchanged';

export interface QuotaHistoryEntry {
  sample: QuotaSample;
  previous: QuotaSample | null;
  /** Change in used percentage points, not relative percent. */
  change: number | null;
  event: QuotaHistoryEvent;
}

export interface QuotaHistorySeries {
  key: string;
  provider: QuotaSample['provider'];
  kind: QuotaSample['kind'];
  scope: string | null;
  samples: QuotaSample[];
  entries: QuotaHistoryEntry[];
  firstUsedPercent: number;
  lastUsedPercent: number;
  /** Positive observed deltas within a cycle and plan; never reconstructed consumption. */
  observedIncrease: number;
  resetCount: number;
  planChangeCount: number;
}

function eventFor(previous: QuotaSample, current: QuotaSample, change: number): QuotaHistoryEvent {
  if (previous.plan !== current.plan) return 'plan_change';
  const oldReset = previous.resetsAt === null ? NaN : Date.parse(previous.resetsAt);
  const newReset = current.resetsAt === null ? NaN : Date.parse(current.resetsAt);
  if (Number.isFinite(oldReset) && Number.isFinite(newReset) && newReset > oldReset &&
      (Date.parse(current.ts) >= oldReset || change < 0)) return 'reset';
  return change > 0 ? 'increase' : change < 0 ? 'decrease' : 'unchanged';
}

/** Keep quota windows separate: account-wide and model-scoped percentages are not additive. */
export function buildQuotaHistory(samples: readonly QuotaSample[]): QuotaHistorySeries[] {
  const grouped = new Map<string, Map<number, QuotaSample>>();
  for (const sample of samples) {
    const timestamp = Date.parse(sample.ts);
    if (!Number.isFinite(timestamp) || !Number.isFinite(sample.usedPercent) ||
        sample.usedPercent < 0 || sample.usedPercent > 100) continue;
    const key = JSON.stringify([sample.provider, sample.kind, sample.scope]);
    let byTimestamp = grouped.get(key);
    if (!byTimestamp) {
      byTimestamp = new Map();
      grouped.set(key, byTimestamp);
    }
    // Map replacement preserves the last input observation at an equivalent instant.
    byTimestamp.set(timestamp, sample);
  }
  return [...grouped].sort(([a], [b]) => a.localeCompare(b)).map(([key, byTimestamp]) => {
    const ordered = [...byTimestamp].sort(([a], [b]) => a - b).map(([, sample]) => sample);
    const entries = ordered.map((sample, index): QuotaHistoryEntry => {
      const previous = index === 0 ? null : ordered[index - 1];
      if (!previous) return { sample, previous: null, change: null, event: 'initial' };
      const change = sample.usedPercent - previous.usedPercent;
      return { sample, previous, change, event: eventFor(previous, sample, change) };
    });
    return {
      key, provider: ordered[0].provider, kind: ordered[0].kind, scope: ordered[0].scope,
      samples: ordered, entries,
      firstUsedPercent: ordered[0].usedPercent,
      lastUsedPercent: ordered[ordered.length - 1].usedPercent,
      observedIncrease: entries.reduce((sum, entry) => sum + (entry.event === 'increase' ? entry.change! : 0), 0),
      resetCount: entries.filter((entry) => entry.event === 'reset').length,
      planChangeCount: entries.filter((entry) => entry.event === 'plan_change').length,
    };
  });
}
