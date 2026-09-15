// Browser mock of the Rust backend for `pnpm dev` without Tauri. [FRONTEND owns]
import type { AppSnapshot, Settings } from './types';

const now = Date.now();
const iso = (ms: number) => new Date(ms).toISOString();

export const mockSnapshot: AppSnapshot = {
  generatedAt: iso(now),
  providers: [
    {
      provider: 'claude',
      displayName: 'Claude',
      plan: 'max',
      planLabel: 'Claude Max 5x',
      account: { email: 'you@example.com', name: 'You' },
      fetchedAt: iso(now),
      source: 'api',
      status: 'ok',
      error: null,
      credits: null,
      windows: [
        { kind: 'five_hour', label: '5-hour', windowSeconds: 18000, usedPercent: 73, resetsAt: iso(now + 51 * 60_000), scope: null, isPrimary: true },
        { kind: 'seven_day', label: 'Weekly', windowSeconds: 604800, usedPercent: 7, resetsAt: iso(now + 3 * 86_400_000), scope: null, isPrimary: false },
        { kind: 'seven_day', label: 'Weekly · Fable', windowSeconds: 604800, usedPercent: 24, resetsAt: iso(now + 3 * 86_400_000), scope: 'Fable', isPrimary: false },
      ],
    },
    {
      provider: 'codex',
      displayName: 'Codex',
      plan: 'plus',
      planLabel: 'ChatGPT Plus',
      account: { email: 'you@example.com', name: null },
      fetchedAt: iso(now),
      source: 'api',
      status: 'ok',
      error: null,
      credits: { hasCredits: false, unlimited: false, balance: '0' },
      windows: [
        { kind: 'five_hour', label: '5-hour', windowSeconds: 18000, usedPercent: 21, resetsAt: iso(now + 2 * 3600_000), scope: null, isPrimary: true },
        { kind: 'seven_day', label: 'Weekly', windowSeconds: 604800, usedPercent: 41, resetsAt: iso(now + 5 * 86_400_000), scope: null, isPrimary: false },
      ],
    },
  ],
};

export const mockSettings: Settings = {
  version: 1,
  language: 'auto',
  theme: 'dark',
  edge: 'right',
  verticalAlign: 'center',
  verticalOffset: 0,
  monitor: null,
  autoHide: false,
  autoHideDelayMs: 800,
  collapsedWidth: 6,
  ringMode: 'primary',
  percentMode: 'used',
  showPercentLabel: true,
  refreshIntervalSec: 60,
  providers: { claude: { enabled: true, order: 0 }, codex: { enabled: true, order: 1 } },
  ingestEnabled: true,
  autostart: false,
  opacity: 1,
  scale: 1,
  thresholds: { warn: 70, critical: 90 },
  notifications: false,
  alwaysOnTop: true,
};

let settings = structuredClone(mockSettings);
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

export async function mockInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  // eslint-disable-next-line no-console
  console.debug('[mock invoke]', cmd, args);
  switch (cmd) {
    case 'get_snapshot':
    case 'refresh_now':
      return structuredClone(mockSnapshot) as T;
    case 'get_settings':
      return structuredClone(settings) as T;
    case 'update_settings':
      settings = { ...settings, ...(args?.patch as Partial<Settings>) };
      mockEmit('settings-updated', structuredClone(settings));
      return structuredClone(settings) as T;
    case 'get_usage_history':
      return { rows: [], totals: emptyTotals(), byProvider: {} } as T;
    case 'get_quota_history':
      return [] as T;
    case 'get_pricing':
      return { entries: [], updatedAt: null } as T;
    case 'set_pricing':
      return args?.table as T;
    case 'reingest_logs':
      return { filesScanned: 0, filesUpdated: 0, eventsAdded: 0, durationMs: 0, errors: [], running: false } as T;
    case 'get_providers':
      return [] as T;
    case 'get_app_info':
      return { version: '0.0.0-mock', dataDir: '/mock', configDir: '/mock', platform: 'linux', backend: 'browser' } as T;
    case 'get_monitors':
      return [] as T;
    case 'popover_show':
      mockEmit('popover-target', args?.req);
      return undefined as T;
    default:
      return undefined as T;
  }
}

function emptyTotals() {
  return { inputTokens: 0, cacheWriteTokens: 0, cacheReadTokens: 0, outputTokens: 0, reasoningTokens: 0, totalTokens: 0, requests: 0, estimatedCostUsd: null };
}
