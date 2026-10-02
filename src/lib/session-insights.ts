// Pure helpers for the Sessions → Insights view. [FRONTEND]
// `buildInsights` mirrors src-tauri/src/sessions/insights.rs for the browser
// preview mock; the real numbers always come from `get_session_insights`.
import type { Histogram, HistogramBin, InsightSession, InsightThresholds, SessionInsights, SessionListQuery, SessionSummary, ToolStat } from './session-types';

export const INSIGHT_THRESHOLDS: InsightThresholds = { minFailures: 3, failureRate: 0.2, minRepeats: 5, repeatRate: 0.3 };
const TOP_N = 8;
const MIN_CALLS_FOR_RATE = 3;
const MAX_BINS = 60;

/** Linear-interpolated percentile (p in 0..1). */
export function percentile(values: readonly number[], p: number): number | null {
  const sorted = values.filter(Number.isFinite).sort((a, b) => a - b);
  if (!sorted.length) return null;
  const pos = Math.min(1, Math.max(0, p)) * (sorted.length - 1);
  const lo = Math.floor(pos), hi = Math.ceil(pos);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo);
}

/** Bins are uniform on a log axis (three per decade), like the Rust side. */
export function logHistogram(values: readonly number[]): Histogram {
  const finite = values.filter(v => Number.isFinite(v) && v >= 0);
  const positive = finite.filter(v => v > 0);
  const hist: Histogram = { bins: [], zeroCount: finite.length - positive.length, sample: finite.length, median: percentile(finite, 0.5), p90: percentile(finite, 0.9) };
  if (!positive.length) return hist;
  const index = (v: number) => Math.floor(Math.log10(v) * 3);
  const hi = index(Math.max(...positive));
  const lo = Math.max(index(Math.min(...positive)), hi - (MAX_BINS - 1));
  const edge = (k: number) => 10 ** (k / 3);
  hist.bins = Array.from({ length: hi - lo + 1 }, (_, i) => ({ from: edge(lo + i), to: edge(lo + i + 1), count: 0 }));
  for (const v of positive) hist.bins[Math.min(hi, Math.max(lo, index(v))) - lo].count++;
  return hist;
}

/** Fractional position of `value` on the bin axis (0 = left edge of the first bin), for drawing markers. */
export function binPosition(value: number | null, bins: readonly HistogramBin[]): number | null {
  if (value == null || !(value > 0) || !bins.length) return null;
  const first = bins[0], last = bins[bins.length - 1];
  if (value <= first.from) return 0;
  if (value >= last.to) return bins.length;
  const i = bins.findIndex(b => value < b.to);
  const b = bins[i];
  return i + (Math.log(value) - Math.log(b.from)) / (Math.log(b.to) - Math.log(b.from));
}

/** Bubble radius in px: a failure-free session is still visible. */
export function bubbleRadius(failures: number): number { return 4 + Math.min(Math.max(failures, 0), 12) * 1.1; }

export function rate(n: number, d: number): number | null { return d > 0 ? n / d : null; }
export function formatRate(value: number | null): string { return value == null ? '—' : `${(value * 100).toFixed(value < 0.1 && value > 0 ? 1 : 0)}%`; }

/** Cost axis/label text: sub-cent values keep their significant digits. */
export function formatUsdCompact(usd: number): string {
  if (usd >= 100) return `$${Math.round(usd)}`;
  if (usd >= 1) return `$${usd.toFixed(2)}`;
  if (usd >= 0.01) return `$${usd.toFixed(3)}`;
  return `$${usd.toPrecision(2)}`;
}
export function formatMinutes(ms: number): string {
  const s = ms / 1000;
  if (s < 90) return `${Math.round(s)}s`;
  if (s < 5400) return `${Math.round(s / 60)}m`;
  return `${(s / 3600).toFixed(1)}h`;
}

export function insightFlags(s: Pick<SessionSummary, 'toolCalls' | 'toolFailures' | 'repeatedToolCalls'>, t: InsightThresholds = INSIGHT_THRESHOLDS): InsightSession['flags'] {
  const out: InsightSession['flags'] = [];
  if (s.toolFailures >= t.minFailures && (rate(s.toolFailures, s.toolCalls) ?? 0) >= t.failureRate) out.push('failures');
  if (s.repeatedToolCalls >= t.minRepeats && (rate(s.repeatedToolCalls, s.toolCalls) ?? 0) >= t.repeatRate) out.push('repeats');
  return out;
}

function top(rows: InsightSession[], key: (r: InsightSession) => number | null): InsightSession[] {
  return rows.flatMap(r => { const k = key(r); return k == null ? [] : [[k, r] as const]; })
    .sort((a, b) => b[0] - a[0] || a[1].provider.localeCompare(b[1].provider) || a[1].sessionId.localeCompare(b[1].sessionId))
    .slice(0, TOP_N).map(([, r]) => r);
}

/** Browser-preview mirror of the Rust aggregate. */
export function buildInsights(sessions: SessionSummary[], tools: ToolStat[], total = sessions.length): SessionInsights {
  const points: InsightSession[] = sessions.map(s => ({
    provider: s.provider, sessionId: s.sessionId, title: s.title, project: s.project, lastTs: s.lastTs, totalTokens: s.totalTokens,
    costUsd: s.estimatedCostUsd ?? null, activeDurationMs: s.activeDurationMs, userTurns: s.userTurns, toolCalls: s.toolCalls,
    toolFailures: s.toolFailures, repeatedToolCalls: s.repeatedToolCalls,
    failureRate: rate(s.toolFailures, s.toolCalls), repeatRate: rate(s.repeatedToolCalls, s.toolCalls), flags: insightFlags(s),
  }));
  const costs = points.flatMap(p => p.costUsd == null ? [] : [p.costUsd]);
  const durations = points.flatMap(p => p.activeDurationMs == null ? [] : [p.activeDurationMs]);
  const sum = (f: (p: InsightSession) => number) => points.reduce((a, p) => a + f(p), 0);
  const calls = sum(p => p.toolCalls), failures = sum(p => p.toolFailures), repeats = sum(p => p.repeatedToolCalls);
  return {
    totalSessions: total, truncated: total > points.length,
    kpis: { sessions: points.length, pricedSessions: costs.length, medianCostUsd: percentile(costs, 0.5), medianActiveMs: percentile(durations, 0.5),
      medianTurns: percentile(points.map(p => p.userTurns), 0.5), toolCalls: calls, toolFailures: failures, repeatedToolCalls: repeats,
      failureRate: rate(failures, calls), repeatRate: rate(repeats, calls) },
    costHistogram: logHistogram(costs), durationHistogram: logHistogram(durations),
    topCost: top(points, p => (p.costUsd ?? 0) > 0 ? p.costUsd : null),
    topDuration: top(points, p => (p.activeDurationMs ?? 0) > 0 ? p.activeDurationMs : null),
    topFailures: top(points, p => p.toolFailures > 0 && p.toolCalls >= MIN_CALLS_FOR_RATE ? p.failureRate : null),
    topRepeats: top(points, p => p.repeatedToolCalls > 0 ? p.repeatedToolCalls : null),
    points, tools, thresholds: INSIGHT_THRESHOLDS,
  };
}

/** The filters a History range hands to the Sessions tab (dates are ISO instants, `to` exclusive). */
export function sessionFilters(range: { from: number; to: number } | null, provider: string, project: string | null, account: string | null = null): SessionListQuery {
  return { search: '', sort: 'recent', offset: 0, limit: 25, provider: provider || null, project: project || null,
    // the key only exists for a chosen account, so a plain query is exactly what it was
    ...(account === null ? {} : { account }),
    from: range ? new Date(range.from).toISOString() : null, to: range ? new Date(range.to).toISOString() : null };
}

/** `?view=insights&provider=&project=&from=&to=` (from/to as ISO instants or yyyy-mm-dd). */
export function filtersFromParams(params: URLSearchParams): Partial<SessionListQuery> | null {
  const out: Partial<SessionListQuery> = {};
  const provider = params.get('provider');
  if (provider === 'claude' || provider === 'codex') out.provider = provider;
  const project = params.get('project');
  if (project) out.project = project;
  for (const key of ['from', 'to'] as const) {
    const raw = params.get(key);
    const ms = raw ? Date.parse(/^\d{4}-\d\d-\d\d$/.test(raw) ? `${raw}T00:00:00` : raw) : NaN;
    if (Number.isFinite(ms)) out[key] = new Date(ms).toISOString();
  }
  return Object.keys(out).length ? out : null;
}
