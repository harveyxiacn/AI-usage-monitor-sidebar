// Browser mock of the Rust backend for `pnpm dev` without Tauri. [FRONTEND owns]
//
// Everything here is deterministic (seeded LCG) so the dashboard charts look
// the same on every reload and screenshots are comparable. The mock implements
// the *semantics* of the real commands (bucketing, filtering, grouping, cost
// estimation) so the UI code is exercised exactly as it would be in Tauri.
import type {
  AppInfo,
  AppSnapshot,
  Bucket,
  HistoryQuery,
  HistoryResult,
  HistoryRow,
  IngestStats,
  MonitorInfo,
  PricingEntry,
  PricingTable,
  ProviderId,
  ProviderInfo,
  QuotaHistoryQuery,
  QuotaSample,
  Settings,
  TokenTotals,
} from './types';
import { mergeSettings, type SettingsPatch } from './settings-writer';

const now = Date.now();
const iso = (ms: number) => new Date(ms).toISOString();
const HOUR = 3_600_000;
const DAY = 24 * HOUR;

// --------------------------------------------------------------- snapshot ---

export const mockSnapshot: AppSnapshot = {
  generatedAt: iso(now),
  providers: [
    {
      provider: 'claude',
      displayName: 'Claude',
      plan: 'max',
      planLabel: 'Claude Max 5x',
      account: { email: 'you@example.com', name: 'You' },
      fetchedAt: iso(now - 12_000),
      source: 'api',
      status: 'ok',
      error: null,
      credits: null,
      extras: [],
      // Forecasts mirror what src-tauri/src/forecast.rs would derive from the
      // sample history below: the 5-hour window burns fast enough to run out
      // before it resets, the weekly ones only drift upwards, and "Weekly ·
      // Fable" is the idle window with no forecast at all.
      windows: [
        { kind: 'five_hour', label: '5-hour', windowSeconds: 18000, usedPercent: 73, resetsAt: iso(now + 51 * 60_000), scope: null, isPrimary: true,
          forecast: { projectedPercentAtReset: 107.4, exhaustsAt: iso(now + 40 * 60_000), ratePercentPerHour: 40.5, confidence: 'high' } },
        { kind: 'seven_day', label: 'Weekly', windowSeconds: 604800, usedPercent: 31, resetsAt: iso(now + 3 * DAY), scope: null, isPrimary: false,
          forecast: { projectedPercentAtReset: 74.2, exhaustsAt: null, ratePercentPerHour: 0.6, confidence: 'medium' } },
        { kind: 'seven_day', label: 'Weekly · Fable', windowSeconds: 604800, usedPercent: 24, resetsAt: iso(now + 3 * DAY), scope: 'Fable', isPrimary: false },
        { kind: 'seven_day', label: 'Weekly · Opus', windowSeconds: 604800, usedPercent: 62, resetsAt: iso(now + 3 * DAY), scope: 'Opus', isPrimary: false,
          forecast: { projectedPercentAtReset: 95.6, exhaustsAt: null, ratePercentPerHour: 0.47, confidence: 'low' } },
      ],
    },
    {
      provider: 'codex',
      displayName: 'Codex',
      plan: 'plus',
      planLabel: 'ChatGPT Plus',
      account: { email: 'you@example.com', name: null },
      fetchedAt: iso(now - 12_000),
      source: 'api',
      status: 'ok',
      error: null,
      credits: { hasCredits: false, unlimited: false, balance: '0' },
      extras: [
        { kind: 'reset_credits', value: '2', detail: '0', severity: 'info' },
        { kind: 'model_unavailable', value: '1', detail: 'gpt-5.3-codex-spark', severity: 'warn' },
      ],
      windows: [
        { kind: 'five_hour', label: '5-hour', windowSeconds: 18000, usedPercent: 21, resetsAt: iso(now + 2 * HOUR + 5 * 60_000), scope: null, isPrimary: true,
          forecast: { projectedPercentAtReset: 33.5, exhaustsAt: null, ratePercentPerHour: 6, confidence: 'medium' } },
        { kind: 'seven_day', label: 'Weekly', windowSeconds: 604800, usedPercent: 41, resetsAt: iso(now + 5 * DAY), scope: null, isPrimary: false,
          forecast: { projectedPercentAtReset: 77.0, exhaustsAt: null, ratePercentPerHour: 0.3, confidence: 'high' } },
        { kind: 'other', label: 'GPT-5.3-Codex-Spark · 5-hour', windowSeconds: 18000, usedPercent: 4, resetsAt: iso(now + 4 * HOUR), scope: 'GPT-5.3-Codex-Spark', isPrimary: false },
        { kind: 'other', label: 'GPT-5.3-Codex-Spark · weekly', windowSeconds: 604800, usedPercent: 12, resetsAt: iso(now + 5 * DAY), scope: 'GPT-5.3-Codex-Spark', isPrimary: false },
      ],
    },
  ],
};

export const mockSettings: Settings = {
  version: 1,
  language: 'auto',
  theme: 'dark',
  surfaceStyle: 'glass',
  cyberAccent: 'neon',
  edge: 'right',
  verticalAlign: 'center',
  verticalOffset: 0,
  monitor: null,
  autoHide: false,
  autoHideDelayMs: 800,
  collapsedWidth: 6,
  ringMode: 'concentric',
  showScopedRing: true,
  percentMode: 'used',
  showPercentLabel: true,
  sidebarItems: { fiveHour: true, weekly: true, scoped: true, other: true, logo: true, percentLabel: true, moreButton: true },
  refreshIntervalSec: 60,
  providers: {
    claude: { enabled: true, showInSidebar: true, order: 0 },
    codex: { enabled: true, showInSidebar: true, order: 1 },
  },
  ingestEnabled: true,
  autostart: false,
  opacity: 1,
  scale: 1,
  thresholds: { warn: 70, critical: 90 },
  colors: { claude: '#ff5c1a', codex: '#10a37f', warn: '#f5c542', critical: '#ff3b30', surface: '', text: '' },
  sizes: { ringSize: 56, ringStroke: 4.5, barGap: 18, barPadding: 10, cornerRadius: 26, labelSize: 13 },
  notifications: false,
  forecastNotifications: true,
  hideAccountEmail: false,
  alwaysOnTop: true,
};

export const mockProviders: ProviderInfo[] = [
  {
    id: 'claude',
    displayName: 'Claude',
    loggedIn: true,
    credentialPath: '~/.claude/.credentials.json',
    logPath: '~/.claude/projects/**/*.jsonl',
    planLabel: 'Claude Max 5x',
  },
  {
    id: 'codex',
    displayName: 'Codex',
    loggedIn: true,
    credentialPath: '~/.codex/auth.json',
    logPath: '~/.codex/sessions/**/*.jsonl',
    planLabel: 'ChatGPT Plus',
  },
];

export const mockMonitors: MonitorInfo[] = [
  { name: 'DP-1', x: 0, y: 0, width: 3840, height: 2160, scaleFactor: 1.5, isPrimary: true },
  { name: 'HDMI-A-1', x: 3840, y: 240, width: 1920, height: 1080, scaleFactor: 1, isPrimary: false },
];

// Keep the browser preview aligned with src-tauri/src/pricing.rs defaults.
const PRICING: PricingEntry[] = [
  { modelPattern: 'claude-opus-4', inputPerM: 15.0, outputPerM: 75.0, cacheWritePerM: 18.75, cacheReadPerM: 1.5 },
  { modelPattern: 'claude-opus-4-1', inputPerM: 15.0, outputPerM: 75.0, cacheWritePerM: 18.75, cacheReadPerM: 1.5 },
  { modelPattern: 'claude-opus-4-5', inputPerM: 5.0, outputPerM: 25.0, cacheWritePerM: 6.25, cacheReadPerM: 0.5 },
  { modelPattern: 'claude-opus-4-6', inputPerM: 5.0, outputPerM: 25.0, cacheWritePerM: 6.25, cacheReadPerM: 0.5 },
  { modelPattern: 'claude-opus-4-7', inputPerM: 5.0, outputPerM: 25.0, cacheWritePerM: 6.25, cacheReadPerM: 0.5 },
  { modelPattern: 'claude-opus-4-8', inputPerM: 5.0, outputPerM: 25.0, cacheWritePerM: 6.25, cacheReadPerM: 0.5 },
  { modelPattern: 'claude-opus-5', inputPerM: 5.0, outputPerM: 25.0, cacheWritePerM: 6.25, cacheReadPerM: 0.5 },
  { modelPattern: 'claude-sonnet-4', inputPerM: 3.0, outputPerM: 15.0, cacheWritePerM: 3.75, cacheReadPerM: 0.3 },
  { modelPattern: 'claude-sonnet-4-5', inputPerM: 3.0, outputPerM: 15.0, cacheWritePerM: 3.75, cacheReadPerM: 0.3 },
  { modelPattern: 'claude-sonnet-4-6', inputPerM: 3.0, outputPerM: 15.0, cacheWritePerM: 3.75, cacheReadPerM: 0.3 },
  { modelPattern: 'claude-sonnet-5', inputPerM: 2.0, outputPerM: 10.0, cacheWritePerM: 2.5, cacheReadPerM: 0.2 },
  { modelPattern: 'claude-haiku-4-5', inputPerM: 1.0, outputPerM: 5.0, cacheWritePerM: 1.25, cacheReadPerM: 0.1 },
  { modelPattern: 'claude-fable-5', inputPerM: 10.0, outputPerM: 50.0, cacheWritePerM: 12.5, cacheReadPerM: 1.0 },
  { modelPattern: 'claude-mythos-5', inputPerM: 10.0, outputPerM: 50.0, cacheWritePerM: 12.5, cacheReadPerM: 1.0 },
  { modelPattern: 'claude-fable-5-1', inputPerM: 10.0, outputPerM: 50.0, cacheWritePerM: 12.5, cacheReadPerM: 0.25 },
  { modelPattern: 'claude-mythos-5-1', inputPerM: 10.0, outputPerM: 50.0, cacheWritePerM: 12.5, cacheReadPerM: 0.25 },
  { modelPattern: 'gpt-5', inputPerM: 1.25, outputPerM: 10.0, cacheWritePerM: 1.25, cacheReadPerM: 0.125 },
  { modelPattern: 'gpt-5-codex', inputPerM: 1.25, outputPerM: 10.0, cacheWritePerM: 1.25, cacheReadPerM: 0.125 },
  { modelPattern: 'gpt-5.1', inputPerM: 1.25, outputPerM: 10.0, cacheWritePerM: 1.25, cacheReadPerM: 0.125 },
  { modelPattern: 'gpt-5.1-codex', inputPerM: 1.25, outputPerM: 10.0, cacheWritePerM: 1.25, cacheReadPerM: 0.125 },
  { modelPattern: 'gpt-5.1-codex-max', inputPerM: 1.25, outputPerM: 10.0, cacheWritePerM: 1.25, cacheReadPerM: 0.125 },
  { modelPattern: 'gpt-5.1-codex-mini', inputPerM: 0.25, outputPerM: 2.0, cacheWritePerM: 0.25, cacheReadPerM: 0.025 },
  { modelPattern: 'gpt-5.2', inputPerM: 1.75, outputPerM: 14.0, cacheWritePerM: 1.75, cacheReadPerM: 0.175 },
  { modelPattern: 'gpt-5.2-codex', inputPerM: 1.75, outputPerM: 14.0, cacheWritePerM: 1.75, cacheReadPerM: 0.175 },
  { modelPattern: 'gpt-5.3-codex', inputPerM: 1.75, outputPerM: 14.0, cacheWritePerM: 1.75, cacheReadPerM: 0.175 },
  { modelPattern: 'gpt-5.4', inputPerM: 2.5, outputPerM: 15.0, cacheWritePerM: 2.5, cacheReadPerM: 0.25 },
  { modelPattern: 'gpt-5.5', inputPerM: 5.0, outputPerM: 30.0, cacheWritePerM: 5.0, cacheReadPerM: 0.5 },
  { modelPattern: 'gpt-5.6-sol', inputPerM: 4.0, outputPerM: 20.0, cacheWritePerM: 5.0, cacheReadPerM: 0.4 },
  { modelPattern: 'gpt-5.6-terra', inputPerM: 2.0, outputPerM: 12.0, cacheWritePerM: 2.5, cacheReadPerM: 0.2 },
  { modelPattern: 'gpt-5.6-luna', inputPerM: 0.2, outputPerM: 1.2, cacheWritePerM: 0.25, cacheReadPerM: 0.02 },
  { modelPattern: 'gpt-6-astra', inputPerM: 10.0, outputPerM: 50.0, cacheWritePerM: 12.5, cacheReadPerM: 1.0 },
  { modelPattern: 'gpt-5-mini', inputPerM: 0.25, outputPerM: 2.0, cacheWritePerM: 0.25, cacheReadPerM: 0.025 },
  { modelPattern: 'gpt-5-nano', inputPerM: 0.05, outputPerM: 0.4, cacheWritePerM: 0.05, cacheReadPerM: 0.005 },
  { modelPattern: 'codex-mini', inputPerM: 1.5, outputPerM: 6.0, cacheWritePerM: 1.5, cacheReadPerM: 0.375 },
  { modelPattern: 'codex-mini-latest', inputPerM: 1.5, outputPerM: 6.0, cacheWritePerM: 1.5, cacheReadPerM: 0.375 },
];

let pricing: PricingTable = { entries: structuredClone(PRICING), updatedAt: iso(now - 9 * DAY) };

export const mockAppInfo: AppInfo = {
  version: '0.1.0-mock',
  dataDir: '~/.local/share/ai-usage-sidebar',
  configDir: '~/.config/ai-usage-sidebar',
  platform: 'linux',
  backend: 'browser',
};

// ------------------------------------------------------------ fake events ---

interface MockEvent {
  ts: number;
  provider: ProviderId;
  model: string;
  project: string | null;
  inputTokens: number;
  cacheWriteTokens: number;
  cacheReadTokens: number;
  outputTokens: number;
  reasoningTokens: number;
  requests: number;
}

const MODELS: Record<ProviderId, string[]> = {
  claude: ['claude-opus-5-20260514', 'claude-sonnet-4-6-20260219', 'claude-haiku-4-5-20251001'],
  codex: ['gpt-5.3-codex', 'gpt-5.3-codex-spark'],
};

/** Same basenames intentionally exercise exact project identity in the UI. */
const PROJECTS = [
  '/home/demo/projects/website',
  '/home/demo/work/client/website',
  'C:\\Users\\Demo\\Projects\\billing-service',
  '',
];

/** Deterministic 32-bit LCG so every reload produces the same chart. */
function lcg(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 0x1_0000_0000;
  };
}

/** 90 days of synthetic sessions — enough for the 90d preset and hour buckets. */
const events: MockEvent[] = (() => {
  const rnd = lcg(0xc0ffee);
  const out: MockEvent[] = [];
  const startOfToday = new Date(now);
  startOfToday.setHours(0, 0, 0, 0);

  for (let dayBack = 89; dayBack >= 0; dayBack -= 1) {
    const dayStart = startOfToday.getTime() - dayBack * DAY;
    const weekday = new Date(dayStart).getDay();
    // weekends are quiet; a slow upward trend over the 90 days
    const dayWeight = (weekday === 0 || weekday === 6 ? 0.35 : 1) * (0.55 + (89 - dayBack) / 120);

    for (const provider of ['claude', 'codex'] as ProviderId[]) {
      // Codex is used roughly half as much as Claude in this fake dataset
      const providerWeight = provider === 'claude' ? 1 : 0.55;
      const models = MODELS[provider];
      for (let m = 0; m < models.length; m += 1) {
        // the flagship model dominates, the small model is a long tail
        const modelWeight = [1, 0.45, 0.18][m] ?? 0.1;
        for (let hour = 8; hour <= 23; hour += 1) {
          // working-hours bell curve
          const hourWeight = Math.max(0, 1 - Math.abs(hour - 15) / 9);
          const p = 0.55 * dayWeight * providerWeight * modelWeight * hourWeight;
          if (rnd() > p) continue;

          const requests = 1 + Math.floor(rnd() * 12 * dayWeight);
          const scale = 900 + rnd() * 5200;
          const input = Math.round(requests * scale * (0.25 + rnd() * 0.4));
          const cacheRead = Math.round(requests * scale * (2 + rnd() * 6));
          const cacheWrite = Math.round(requests * scale * (0.15 + rnd() * 0.5));
          const output = Math.round(requests * scale * (0.1 + rnd() * 0.35));
          const reasoning = provider === 'codex' ? Math.round(output * (0.2 + rnd() * 0.8)) : Math.round(output * rnd() * 0.3);
          out.push({
            ts: dayStart + hour * HOUR + Math.floor(rnd() * HOUR),
            provider,
            model: models[m],
            project: PROJECTS[(dayBack + hour + m) % PROJECTS.length] || null,
            inputTokens: input,
            cacheWriteTokens: cacheWrite,
            cacheReadTokens: cacheRead,
            outputTokens: output,
            reasoningTokens: reasoning,
            requests,
          });
        }
      }
    }
  }
  return out.sort((a, b) => a.ts - b.ts);
})();

// ---------------------------------------------------------------- helpers ---

function emptyTotals(): TokenTotals {
  return {
    inputTokens: 0,
    cacheWriteTokens: 0,
    cacheReadTokens: 0,
    outputTokens: 0,
    reasoningTokens: 0,
    totalTokens: 0,
    requests: 0,
    estimatedCostUsd: null,
  };
}

/** Built-ins match exact normalized names; custom entries allow prefixes. */
function priceFor(model: string): PricingEntry | null {
  const normalized = model.trim().toLowerCase().replace(/-\d{6,8}$/, '');
  let best: PricingEntry | null = null;
  for (const e of pricing.entries) {
    const pattern = e.modelPattern.toLowerCase();
    const builtin = PRICING.some((entry) => entry.modelPattern === pattern);
    if (pattern && (builtin ? normalized === pattern : normalized.startsWith(pattern)) && (!best || pattern.length > best.modelPattern.length)) best = e;
  }
  return best;
}

function costOf(model: string | null, t: TokenTotals): number | null {
  if (!model) return null;
  const p = priceFor(model);
  if (!p) return null;
  return (
    (t.inputTokens * p.inputPerM +
      t.outputTokens * p.outputPerM +
      t.cacheWriteTokens * p.cacheWritePerM +
      t.cacheReadTokens * p.cacheReadPerM) /
    1_000_000
  );
}

/** Start of the bucket containing `ts`, in *local* time (contract §4). */
function bucketStart(ts: number, bucket: Bucket): number {
  const d = new Date(ts);
  d.setMinutes(0, 0, 0);
  if (bucket === 'hour') return d.getTime();
  d.setHours(0, 0, 0, 0);
  if (bucket === 'day') return d.getTime();
  if (bucket === 'week') {
    // ISO weeks: Monday is day 0
    const shift = (d.getDay() + 6) % 7;
    d.setDate(d.getDate() - shift);
    return d.getTime();
  }
  d.setDate(1);
  return d.getTime();
}

function addInto(acc: TokenTotals, e: MockEvent) {
  acc.inputTokens += e.inputTokens;
  acc.cacheWriteTokens += e.cacheWriteTokens;
  acc.cacheReadTokens += e.cacheReadTokens;
  acc.outputTokens += e.outputTokens;
  acc.reasoningTokens += e.reasoningTokens;
  acc.requests += e.requests;
  acc.totalTokens += e.inputTokens + e.cacheWriteTokens + e.cacheReadTokens + e.outputTokens;
}

function runHistory(q: HistoryQuery): HistoryResult {
  const from = Date.parse(q.from);
  const to = Date.parse(q.to);
  const rows = new Map<string, HistoryRow>();
  const totals = emptyTotals();
  const byProvider: Record<string, TokenTotals> = {};
  const projects = new Set<string>();
  // cost is accumulated per model then summed, because the price list is
  // per-model — a grouped-by-provider row still gets a meaningful estimate.
  const rowCost = new Map<string, number | null>();
  const providerCost: Record<string, number | null> = {};
  let totalCost: number | null = 0;

  for (const e of events) {
    if (e.ts < from || e.ts >= to) continue;
    if (q.provider && e.provider !== q.provider) continue;
    const project = e.project ?? '';
    projects.add(project);
    if (q.project != null && project !== q.project) continue;

    const bs = bucketStart(e.ts, q.bucket);
    const rowProject = q.groupByProject ? project : q.project ?? null;
    const key = JSON.stringify([bs, e.provider, q.groupByModel ? e.model : null, rowProject]);
    let row = rows.get(key);
    if (!row) {
      row = {
        ...emptyTotals(),
        bucketStart: iso(bs),
        provider: e.provider,
        model: q.groupByModel ? e.model : null,
        project: rowProject,
      };
      rows.set(key, row);
      rowCost.set(key, 0);
    }
    addInto(row, e);
    addInto(totals, e);
    byProvider[e.provider] ??= emptyTotals();
    addInto(byProvider[e.provider], e);

    const c = costOf(e.model, {
      ...emptyTotals(),
      inputTokens: e.inputTokens,
      cacheWriteTokens: e.cacheWriteTokens,
      cacheReadTokens: e.cacheReadTokens,
      outputTokens: e.outputTokens,
    });
    const prev = rowCost.get(key);
    rowCost.set(key, prev == null || c == null ? null : prev + c);
    if (!(e.provider in providerCost)) providerCost[e.provider] = 0;
    const pc = providerCost[e.provider];
    providerCost[e.provider] = pc == null || c == null ? null : pc + c;
    totalCost = totalCost == null || c == null ? null : totalCost + c;
  }

  for (const [key, row] of rows) row.estimatedCostUsd = rowCost.get(key) ?? null;
  for (const p of Object.keys(byProvider)) byProvider[p].estimatedCostUsd = providerCost[p] ?? null;
  totals.estimatedCostUsd = totalCost;

  const list = [...rows.values()].sort(
    (a, b) =>
      Date.parse(a.bucketStart) - Date.parse(b.bucketStart) ||
      a.provider.localeCompare(b.provider) ||
      (a.model ?? '').localeCompare(b.model ?? '') ||
      (a.project ?? '').localeCompare(b.project ?? '')
  );
  return { rows: list, totals, byProvider, projects: [...projects].sort((a, b) => a.localeCompare(b)) };
}

/** Quota samples every 30 min for the last 14 days, sawtooth per window. */
function runQuotaHistory(q: QuotaHistoryQuery): QuotaSample[] {
  const from = Date.parse(q.from);
  const to = Date.parse(q.to);
  const rnd = lcg(0xbeef);
  const out: QuotaSample[] = [];
  for (const provider of ['claude', 'codex'] as ProviderId[]) {
    if (q.provider && provider !== q.provider) continue;
    const plan = provider === 'claude' ? 'max' : 'plus';
    for (let ts = to - 14 * DAY; ts < to; ts += HOUR / 2) {
      if (ts < from) continue;
      const base = provider === 'claude' ? 1 : 0.6;
      // 5-hour window: sawtooth that resets every 5 h
      const phase5 = ((ts % (5 * HOUR)) / (5 * HOUR)) * 100;
      const five = Math.min(100, Math.max(0, phase5 * base * (0.5 + rnd() * 0.9)));
      // weekly window: slow ramp that resets on the week boundary
      const phase7 = ((ts % (7 * DAY)) / (7 * DAY)) * 100;
      const seven = Math.min(100, Math.max(0, phase7 * base * (0.6 + rnd() * 0.4)));
      out.push({ provider, kind: 'five_hour', scope: null, usedPercent: Math.round(five), resetsAt: iso(ts + 5 * HOUR - (ts % (5 * HOUR))), plan, ts: iso(ts) });
      out.push({ provider, kind: 'seven_day', scope: null, usedPercent: Math.round(seven), resetsAt: iso(ts + 7 * DAY - (ts % (7 * DAY))), plan, ts: iso(ts) });
    }
  }
  return out;
}

// ------------------------------------------------------------- event bus ----

/**
 * `?settings=<url-encoded JSON patch>` seeds the mock settings for this page
 * load. The sidebar route has no UI of its own to change settings with, so
 * this is how the e2e suite renders the bar in a given configuration.
 */
function seededSettings(): Settings {
  const base = structuredClone(mockSettings);
  if (typeof window === 'undefined') return base;
  const raw = new URLSearchParams(window.location.search).get('settings');
  if (!raw) return base;
  try {
    return mergeSettings(base, JSON.parse(raw) as SettingsPatch);
  } catch {
    return base;
  }
}

let settings = seededSettings();
let snapshot = structuredClone(mockSnapshot);
const listeners = new Map<string, Set<(p: unknown) => void>>();

export function mockEmit(event: string, payload: unknown) {
  listeners.get(event)?.forEach((h) => h(payload));
}

export async function mockListen<T>(event: string, handler: (p: T) => void): Promise<() => void> {
  if (!listeners.has(event)) listeners.set(event, new Set());
  const h = handler as (p: unknown) => void;
  listeners.get(event)!.add(h);
  return () => listeners.get(event)?.delete(h);
}

/** Small random walk so "Refresh" visibly does something in the browser. */
function jitterSnapshot(provider?: ProviderId | null) {
  const stamp = iso(Date.now());
  snapshot = {
    generatedAt: stamp,
    providers: snapshot.providers.map((p) => {
      if (provider && p.provider !== provider) return p;
      return {
        ...p,
        fetchedAt: stamp,
        windows: p.windows.map((w) => ({
          ...w,
          usedPercent: Math.min(100, Math.max(0, Math.round(w.usedPercent + (Math.random() * 8 - 3)))),
        })),
      };
    }),
  };
  return structuredClone(snapshot);
}

export async function mockInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  switch (cmd) {
    case 'get_snapshot':
      return structuredClone(snapshot) as T;
    case 'refresh_now': {
      const s = jitterSnapshot(args?.provider as ProviderId | null);
      mockEmit('snapshot-updated', structuredClone(s));
      return s as T;
    }
    case 'get_settings':
      return structuredClone(settings) as T;
    case 'update_settings': {
      const patch = (args?.patch ?? {}) as SettingsPatch;
      // `providers`, `colors` and `sizes` are merged per key, like the Rust
      // update_settings does (docs/ARCHITECTURE.md §5 / §7).
      settings = mergeSettings(settings, patch);
      mockEmit('settings-updated', structuredClone(settings));
      return structuredClone(settings) as T;
    }
    case 'get_usage_history':
      return runHistory(args?.query as HistoryQuery) as T;
    case 'get_quota_history':
      return runQuotaHistory(args?.query as QuotaHistoryQuery) as T;
    case 'get_pricing':
      return structuredClone(pricing) as T;
    case 'set_pricing': {
      const table = args?.table as PricingTable;
      const entries: PricingEntry[] = [];
      for (const entry of table.entries) {
        if (!entry.modelPattern.trim() || [entry.inputPerM, entry.outputPerM, entry.cacheWritePerM, entry.cacheReadPerM].some((value) => !Number.isFinite(value) || value < 0)) {
          throw new Error('pricing entries require a model name and finite, non-negative rates');
        }
        const normalized = { ...entry, modelPattern: entry.modelPattern.trim().toLowerCase() };
        const index = entries.findIndex((candidate) => candidate.modelPattern === normalized.modelPattern);
        if (index === -1) entries.push(normalized);
        else entries[index] = normalized;
      }
      pricing = { entries, updatedAt: iso(Date.now()) };
      return structuredClone(pricing) as T;
    }
    case 'reingest_logs': {
      const start = Date.now();
      const stats: IngestStats = { filesScanned: 0, filesUpdated: 0, eventsAdded: 0, durationMs: 0, errors: [], running: true };
      for (let i = 1; i <= 4; i += 1) {
        setTimeout(() => {
          mockEmit('ingest-progress', { ...stats, filesScanned: i * 312, filesUpdated: i * 7, eventsAdded: i * 1840, durationMs: Date.now() - start, running: true });
        }, i * 180);
      }
      const done: IngestStats = { filesScanned: 1248, filesUpdated: 29, eventsAdded: 7361, durationMs: 940, errors: [], running: false };
      setTimeout(() => mockEmit('ingest-progress', done), 900);
      return new Promise<T>((resolve) => setTimeout(() => resolve(done as T), 900));
    }
    case 'get_providers':
      return structuredClone(mockProviders) as T;
    case 'get_app_info':
      return structuredClone(mockAppInfo) as T;
    case 'get_monitors':
      return structuredClone(mockMonitors) as T;
    case 'popover_show':
      mockEmit('popover-target', args?.req);
      return undefined as T;
    case 'sidebar_set_expanded':
      mockEmit('sidebar-state', { expanded: Boolean(args?.expanded), pinned: false });
      return undefined as T;
    case 'open_dashboard':
      mockEmit('dashboard-navigate', { tab: args?.tab ?? 'overview' });
      return undefined as T;
    default:
      return undefined as T;
  }
}
