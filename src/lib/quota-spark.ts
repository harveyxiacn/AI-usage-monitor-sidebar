// The ~24 h used-% sparkline of a popover row: picks one window's samples out
// of the quota history and keeps a short-lived cache so opening the popover
// repeatedly never re-queries. Pure (the loader is injected). [FRONTEND]
import type { ProviderId, QuotaSample, QuotaWindow } from './types';

export const SPARK_SPAN_MS = 24 * 60 * 60 * 1000;
export const SPARK_MAX_POINTS = 48;
export const SPARK_TTL_MS = 60_000;

/** Used-% values of one window over the last 24 h, oldest first, thinned to <= `maxPoints`. */
export function sparkValues(
  samples: readonly QuotaSample[],
  provider: ProviderId,
  w: Pick<QuotaWindow, 'kind' | 'scope'>,
  now: number,
  maxPoints = SPARK_MAX_POINTS
): number[] {
  const from = now - SPARK_SPAN_MS;
  const picked = samples
    .filter((s) => s.provider === provider && s.kind === w.kind && (s.scope ?? null) === (w.scope ?? null))
    .map((s) => ({ t: Date.parse(s.ts), v: s.usedPercent }))
    .filter((s) => Number.isFinite(s.t) && s.t >= from && s.t <= now + 60_000 && Number.isFinite(s.v))
    .sort((a, b) => a.t - b.t)
    .map((s) => s.v);
  if (picked.length <= maxPoints) return picked;
  // evenly thinned, always keeping the newest sample
  const out: number[] = [];
  for (let i = 0; i < maxPoints; i++) out.push(picked[Math.round((i * (picked.length - 1)) / (maxPoints - 1))]);
  return out;
}

/** Per-key cache with a TTL and one in-flight request per key. */
export class SparkCache {
  private entries = new Map<string, { at: number; samples: QuotaSample[] }>();
  private inflight = new Map<string, Promise<QuotaSample[]>>();

  constructor(private ttlMs = SPARK_TTL_MS) {}

  async get(key: string, load: () => Promise<QuotaSample[]>, now = Date.now()): Promise<QuotaSample[]> {
    const hit = this.entries.get(key);
    if (hit && now - hit.at < this.ttlMs) return hit.samples;
    const running = this.inflight.get(key);
    if (running) return running;
    const p = load()
      .then((samples) => {
        this.entries.set(key, { at: now, samples });
        return samples;
      })
      .finally(() => this.inflight.delete(key));
    this.inflight.set(key, p);
    return p;
  }
}
