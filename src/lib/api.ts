// Thin wrapper around Tauri invoke/listen with a browser mock so the UI can be
// developed with plain `pnpm dev` in a browser. Command names are the contract
// in docs/ARCHITECTURE.md §5. [FRONTEND owns this file — keep names stable]

import type {
  AppInfo,
  AppSnapshot,
  BackupInfo,
  CalendarQuery,
  CalendarResult,
  DashboardTab,
  Diagnostics,
  HistoryQuery,
  HistoryResult,
  ImportResult,
  IngestStats,
  MonitorInfo,
  PopoverRequest,
  PriceUpdateStatus,
  PricingTable,
  ProviderId,
  ProviderInfo,
  QuotaHistoryQuery,
  QuotaSample,
  SessionQuery,
  SessionsResult,
  Settings,
  SettingsVersion,
  ShortcutRegistrations,
  ShortcutStatus,
  SidebarState,
  TokenTotals,
  UpdateStatus,
  WindowUsageQuery,
} from './types';
import type { AnalysisSettings, EvaluationPreview, EvaluationReport, RequirementAssessment, SessionDetail, SessionInsights, SessionListQuery, SessionListResult } from './session-types';
import type { SettingsPatch } from './settings-writer';

export const isTauri = (): boolean =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<T>(cmd, args);
  }
  return (await import('./mock')).mockInvoke<T>(cmd, args);
}

/** Opens an https link in the system browser (a new tab in browser previews). */
export async function openExternal(url: string): Promise<void> {
  if (isTauri()) {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url);
  } else {
    window.open(url, '_blank', 'noopener');
  }
}

export type Unlisten = () => void;

export async function listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten> {
  if (isTauri()) {
    const { listen } = await import('@tauri-apps/api/event');
    return listen<T>(event, (e) => handler(e.payload));
  }
  return (await import('./mock')).mockListen<T>(event, handler);
}

// ---- backend ----
export const getSnapshot = () => invoke<AppSnapshot>('get_snapshot');
export const refreshNow = (provider?: ProviderId) => invoke<AppSnapshot>('refresh_now', { provider: provider ?? null });
export const getSettings = () => invoke<Settings>('get_settings');
export const updateSettings = (patch: SettingsPatch) => invoke<Settings>('update_settings', { patch });
export const getUsageHistory = (query: HistoryQuery) => invoke<HistoryResult>('get_usage_history', { query });
export const getUsageCalendar = (query: CalendarQuery) => invoke<CalendarResult>('get_usage_calendar', { query });
export const getUsageSessions = (query: SessionQuery) => invoke<SessionsResult>('get_usage_sessions', { query });
export const listSessions = (query: SessionListQuery) => invoke<SessionListResult>('list_sessions', { query });
export const getSessionInsights = (query: SessionListQuery) => invoke<SessionInsights>('get_session_insights', { query });
export const getSessionDetail = (provider: string, sessionId: string, offset = 0, limit = 40) => invoke<SessionDetail>('get_session_detail', { provider, sessionId, offset, limit });
export const setSessionAlias = (provider: string, sessionId: string, alias: string) => invoke<void>('set_session_alias', { provider, sessionId, alias });
export const getAnalysisSettings = () => invoke<AnalysisSettings>('get_analysis_settings');
export const saveAnalysisSettings = (settings: AnalysisSettings) => invoke<AnalysisSettings>('save_analysis_settings', { settings });
export const prepareSessionEvaluation = (provider: string, sessionId: string, turnIds: string[] = []) => invoke<EvaluationPreview>('prepare_session_evaluation', { provider, sessionId, turnIds });
export const evaluateSession = (preview: EvaluationPreview) => invoke<EvaluationReport>('evaluate_session', { preview });
export const getSessionEvaluations = (provider: string, sessionId: string) => invoke<EvaluationReport[]>('get_session_evaluations', { provider, sessionId });
export const saveEvaluationReview = (id: string, requirements: RequirementAssessment[]) => invoke<EvaluationReport>('save_evaluation_review', { id, requirements });
export const clearSessionAnalysis = (provider: string, sessionId: string) => invoke<void>('clear_session_analysis', { provider, sessionId });
export const getQuotaHistory = (query: QuotaHistoryQuery) => invoke<QuotaSample[]>('get_quota_history', { query });
export const getWindowUsage = (query: WindowUsageQuery) => invoke<TokenTotals[]>('get_window_usage', { query });
export const getPricing = () => invoke<PricingTable>('get_pricing');
export const setPricing = (table: PricingTable) => invoke<PricingTable>('set_pricing', { table });
/** Refreshes the configured source, or the official project source when pricingUrl is empty. */
export const refreshPricing = () => invoke<PricingTable>('refresh_pricing');
export const getPriceUpdateStatus = () => invoke<PriceUpdateStatus>('get_price_update_status');
/** Checks the official source when pricingUrl is empty, or the configured custom source. */
export const checkForPriceUpdates = () => invoke<PriceUpdateStatus>('check_for_price_updates');
/** Applies the checked source table. The backend rejects this while a manual table is active. */
export const applyPriceUpdate = () => invoke<PricingTable>('apply_price_update');
/** Explicitly discards a manual table and returns to the configured source. */
export const useSourcePricing = () => invoke<PricingTable>('use_source_pricing');
export const reingestLogs = () => invoke<IngestStats>('reingest_logs');
export const getProviders = () => invoke<ProviderInfo[]>('get_providers');
export const getAppInfo = () => invoke<AppInfo>('get_app_info');
/** Backs settings + database up into a new timestamped folder inside `dest`; no `dest` opens a folder picker. null = cancelled. */
export const backupData = (dest?: string) => invoke<string | null>('backup_data', { dest: dest ?? null });
/** Validates a backup and stages it; it is swapped in at the next start (`restartApp`). null = cancelled. */
export const restoreData = (src?: string) => invoke<BackupInfo | null>('restore_data', { src: src ?? null });
export const restartApp = () => invoke<void>('restart_app');
/** Everything a bug report needs; nothing secret, e-mails always masked. */
export const getDiagnostics = () => invoke<Diagnostics>('get_diagnostics');
/** Opens one of the app's own folders in the file manager. */
export const openFolder = (which: 'log' | 'config' | 'data') => invoke<void>('open_folder', { which });

// ---- settings backup / undo ----
/** Native save dialog (a download in browser previews); the saved path, or null when cancelled. */
export const exportSettings = () => invoke<string | null>('export_settings');

/** Native open dialog (a file picker in browser previews); null when cancelled. */
export async function importSettings(): Promise<ImportResult | null> {
  if (isTauri()) return invoke<ImportResult | null>('import_settings');
  const file = await new Promise<File | null>((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'application/json,.json';
    input.onchange = () => resolve(input.files?.[0] ?? null);
    input.oncancel = () => resolve(null);
    input.click();
  });
  if (!file) return null;
  return (await import('./mock')).mockInvoke<ImportResult>('import_settings', { file });
}

/** The last few settings versions, newest first. */
export const getSettingsHistory = () => invoke<SettingsVersion[]>('get_settings_history');
/** Make version `index` (0 = newest) the live settings; the replaced one is kept for undoing. */
export const restoreSettingsVersion = (index: number) => invoke<Settings>('restore_settings_version', { index });

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

// ---- updater ----
export const getUpdateStatus = () => invoke<UpdateStatus>('get_update_status');
/** Reads the release feed; never downloads anything. */
export const checkForUpdates = () => invoke<UpdateStatus>('check_for_updates');
/** Downloads, installs and restarts — only for bundles we may replace. */
export const installUpdate = () => invoke<void>('install_update');

// ---- platform ----
export const getShortcutStatus = () => invoke<ShortcutStatus>('get_shortcut_status');
/** Per shortcut: off / registered / failed / unsupported (native Wayland). */
export const getShortcutRegistrations = () => invoke<ShortcutRegistrations>('get_shortcut_registrations');
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
export const onUpdateStatus = (h: (s: UpdateStatus) => void) => listen<UpdateStatus>('update-status', h);
export const onPriceUpdateStatus = (h: (s: PriceUpdateStatus) => void) => listen<PriceUpdateStatus>('price-update-status', h);
