// Thin wrapper around Tauri invoke/listen with a browser mock so the UI can be
// developed with plain `pnpm dev` in a browser. Command names are the contract
// in docs/ARCHITECTURE.md §5. [FRONTEND owns this file — keep names stable]

import type {
  AppInfo,
  AppSnapshot,
  DashboardTab,
  HistoryQuery,
  HistoryResult,
  IngestStats,
  MonitorInfo,
  PopoverRequest,
  PricingTable,
  ProviderId,
  ProviderInfo,
  QuotaHistoryQuery,
  QuotaSample,
  Settings,
  SidebarState,
} from './types';
import { mockInvoke, mockListen } from './mock';
import type { SettingsPatch } from './settings-writer';

export const isTauri = (): boolean =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<T>(cmd, args);
  }
  return mockInvoke<T>(cmd, args);
}

export type Unlisten = () => void;

export async function listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten> {
  if (isTauri()) {
    const { listen } = await import('@tauri-apps/api/event');
    return listen<T>(event, (e) => handler(e.payload));
  }
  return mockListen<T>(event, handler);
}

// ---- backend ----
export const getSnapshot = () => invoke<AppSnapshot>('get_snapshot');
export const refreshNow = (provider?: ProviderId) => invoke<AppSnapshot>('refresh_now', { provider: provider ?? null });
export const getSettings = () => invoke<Settings>('get_settings');
export const updateSettings = (patch: SettingsPatch) => invoke<Settings>('update_settings', { patch });
export const getUsageHistory = (query: HistoryQuery) => invoke<HistoryResult>('get_usage_history', { query });
export const getQuotaHistory = (query: QuotaHistoryQuery) => invoke<QuotaSample[]>('get_quota_history', { query });
export const getPricing = () => invoke<PricingTable>('get_pricing');
export const setPricing = (table: PricingTable) => invoke<PricingTable>('set_pricing', { table });
/** Only reaches the network when Settings.pricingUrl is set. */
export const refreshPricing = () => invoke<PricingTable>('refresh_pricing');
export const reingestLogs = () => invoke<IngestStats>('reingest_logs');
export const getProviders = () => invoke<ProviderInfo[]>('get_providers');
export const getAppInfo = () => invoke<AppInfo>('get_app_info');

/** Native save dialog on desktop; a normal file download in browser previews. */
export async function exportUsageCsv(csv: string, suggestedName: string): Promise<string | null> {
  if (isTauri()) return invoke<string | null>('export_usage_csv', { csv, suggestedName });
  const url = URL.createObjectURL(new Blob(['\uFEFF', csv], { type: 'text/csv;charset=utf-8' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = suggestedName;
  document.body.appendChild(link);
  try { link.click(); } finally {
    link.remove();
    // WebKit consumes the blob asynchronously after the click.
    setTimeout(() => URL.revokeObjectURL(url), 30_000);
  }
  return suggestedName;
}

// ---- platform ----
export const sidebarSetExpanded = (expanded: boolean) => invoke<void>('sidebar_set_expanded', { expanded });
export const sidebarRelayout = (width: number, height: number) => invoke<void>('sidebar_relayout', { width, height });
/** dx/dy: CSS px the pointer travelled since the drag started */
export const sidebarDrag = (phase: 'start' | 'move' | 'end' | 'cancel', dx = 0, dy = 0) => invoke<void>('sidebar_drag', { phase, dx, dy });
export const popoverShow = (req: PopoverRequest) => invoke<void>('popover_show', { req });
export const popoverHide = () => invoke<void>('popover_hide');
export const popoverRelayout = (width: number, height: number) => invoke<void>('popover_relayout', { width, height });
export const popoverSetPinned = (pinned: boolean) => invoke<void>('popover_set_pinned', { pinned });
export const hoverReport = (source: 'bar' | 'popover', hovered: boolean) => invoke<void>('hover_report', { source, hovered });
export const openDashboard = (tab?: DashboardTab) => invoke<void>('open_dashboard', { tab: tab ?? null });
export const applyWindowSettings = () => invoke<void>('apply_window_settings');
export const getMonitors = () => invoke<MonitorInfo[]>('get_monitors');
export const quitApp = () => invoke<void>('quit_app');

// ---- events ----
export const onSnapshotUpdated = (h: (s: AppSnapshot) => void) => listen<AppSnapshot>('snapshot-updated', h);
export const onSettingsUpdated = (h: (s: Settings) => void) => listen<Settings>('settings-updated', h);
export const onIngestProgress = (h: (s: IngestStats) => void) => listen<IngestStats>('ingest-progress', h);
export const onPopoverTarget = (h: (r: PopoverRequest) => void) => listen<PopoverRequest>('popover-target', h);
export const onSidebarState = (h: (s: SidebarState) => void) => listen<SidebarState>('sidebar-state', h);
export const onDashboardNavigate = (h: (p: { tab: DashboardTab }) => void) => listen<{ tab: DashboardTab }>('dashboard-navigate', h);
