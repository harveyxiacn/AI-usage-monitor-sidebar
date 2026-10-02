// The command palette's registry: navigation, actions and setting toggles. [FRONTEND]
//
// Pure: everything that touches the app (stores, Tauri, the DOM) arrives
// through `PaletteContext`, so `tests/palette-commands.unit.ts` can build the
// registry against stubs and check ids, titles, toggles and presets.
import type { PaletteCommand } from './palette';
import { BUILTIN_IDS, BUILTIN_PRESETS } from './presets';
import { SECTIONS } from './settings-sections';
import type { SettingsPatch } from './settings-writer';
import type { DashboardTab, Settings, SidebarItems } from './types';
import { HISTORY_VIEWS, type HistoryView } from './history-view-state';

export type SessionsView = 'browse' | 'insights';

export interface PaletteContext {
  settings: Settings;
  /** Catalogue lookup; the registry only ever passes keys that exist. */
  t: (key: string, params?: Record<string, string | number>) => string;
  /** Sessions has its own label table (`session-labels.svelte.ts`). */
  sessionsLabel: (key: 'title' | SessionsView) => string;
  now: () => number;
  navigate: (tab: DashboardTab) => void;
  openHistoryView: (view: HistoryView) => void;
  openSessionsView: (view: SessionsView) => void;
  openSettingsCard: (id: string) => void;
  refresh: () => void;
  rescan: () => void;
  toggleSidebar: () => void;
  copyDiagnostics: () => void;
  exportCsv: () => void;
  openLogFolder: () => void;
  shareCard: () => void;
  patch: (patch: SettingsPatch) => void;
}

/** Top-level boolean settings the palette can flip, with their label key. */
export const TOGGLES = [
  ['autoHide', 'settings.autoHide'],
  ['adaptiveRefresh', 'settings.adaptiveRefresh'],
  ['autostart', 'settings.autostart'],
  ['alwaysOnTop', 'settings.alwaysOnTop'],
  ['sidebarAnimations', 'settings.sidebarAnimations'],
  ['notifications', 'settings.notifications'],
  ['forecastNotifications', 'settings.forecastNotifications'],
  ['focusHidesSidebar', 'settings.focusHidesSidebar'],
  ['hideAccountEmail', 'settings.hideAccountEmail'],
  ['ingestEnabled', 'settings.ingestEnabled'],
  ['exportSnapshot', 'integrations.export'],
  ['autoUpdateCheck', 'settings.autoUpdateCheck'],
  ['autoPricingCheck', 'settings.autoPricingCheck'],
] as const satisfies readonly (readonly [keyof Settings, string])[];

export const SIDEBAR_ITEM_KEYS: readonly (keyof SidebarItems)[] = [
  'fiveHour',
  'weekly',
  'scoped',
  'other',
  'logo',
  'percentLabel',
  'moreButton',
];

/** Epoch ms of the next 08:00 local time after `now` (tomorrow morning). */
export function tomorrowMorning(now: number): number {
  const d = new Date(now);
  d.setDate(d.getDate() + 1);
  d.setHours(8, 0, 0, 0);
  return d.getTime();
}

export function buildCommands(ctx: PaletteContext): PaletteCommand[] {
  const { t, settings } = ctx;
  const out: PaletteCommand[] = [];
  const goto = (place: string) => t('palette.goto', { place });
  const on = (value: boolean) => t(value ? 'palette.state.on' : 'palette.state.off');

  // ---- navigation ----
  out.push({ id: 'nav:overview', group: 'navigate', title: goto(t('tab.overview')), keywords: 'home dashboard', run: () => ctx.navigate('overview') });
  for (const view of HISTORY_VIEWS) {
    out.push({
      id: `nav:history:${view}`,
      group: 'navigate',
      title: goto(`${t('tab.history')} · ${t(`history.view.${view}`)}`),
      keywords: view === 'cost' ? 'budget spend money plan advisor upgrade' : view === 'quota' ? 'limits windows' : view === 'commits' ? 'git commit task cost' : 'tokens analytics',
      run: () => ctx.openHistoryView(view),
    });
  }
  for (const view of ['browse', 'insights'] as const) {
    out.push({
      id: `nav:sessions:${view}`,
      group: 'navigate',
      title: goto(`${ctx.sessionsLabel('title')} · ${ctx.sessionsLabel(view)}`),
      keywords: 'sessions conversations',
      run: () => ctx.openSessionsView(view),
    });
  }
  out.push({ id: 'nav:settings', group: 'navigate', title: goto(t('tab.settings')), keywords: 'preferences options', run: () => ctx.navigate('settings') });
  for (const section of SECTIONS) {
    out.push({
      id: `nav:settings:${section.id}`,
      group: 'navigate',
      title: goto(`${t('tab.settings')} · ${t(section.title)}`),
      keywords: 'preferences options',
      run: () => ctx.openSettingsCard(section.id),
    });
  }

  // ---- actions ----
  const action = (id: string, titleKey: string, keywords: string, run: () => void, hint?: string): void => {
    out.push({ id: `action:${id}`, group: 'action', title: t(titleKey), keywords, hint, run });
  };
  action('refresh', 'palette.action.refresh', 'reload poll update quota', ctx.refresh);
  action('rescan', 'palette.action.rescan', 'ingest logs reindex', ctx.rescan);
  action('toggleSidebar', 'palette.action.toggleSidebar', 'show hide bar sidebar', ctx.toggleSidebar);
  action('pause', settings.pollingPaused ? 'palette.action.resume' : 'palette.action.pause', 'polling pause resume stop', () => ctx.patch({ pollingPaused: !settings.pollingPaused }), on(settings.pollingPaused));
  action('shareCard', 'palette.action.shareCard', 'share image png card screenshot', ctx.shareCard);
  action('copyDiagnostics', 'palette.action.copyDiagnostics', 'diagnostics bug report support clipboard', ctx.copyDiagnostics);
  action('exportCsv', 'palette.action.exportCsv', 'export csv download usage', ctx.exportCsv);
  action('openLogs', 'palette.action.openLogs', 'log folder directory open', ctx.openLogFolder);
  const focusAt = (id: string, titleKey: string, until: () => number) =>
    action(`focus:${id}`, titleKey, 'focus do not disturb dnd quiet', () => ctx.patch({ focusUntil: until() }));
  focusAt('hour', 'palette.focus.hour', () => ctx.now() + 3_600_000);
  focusAt('morning', 'palette.focus.morning', () => tomorrowMorning(ctx.now()));
  focusAt('forever', 'palette.focus.forever', () => -1);
  focusAt('off', 'palette.focus.off', () => 0);

  // ---- presets (applied straight away; Settings › Presets previews them) ----
  for (const id of BUILTIN_IDS) {
    out.push({
      id: `preset:builtin:${id}`,
      group: 'action',
      title: t('palette.preset', { name: t(`settings.preset.${id}`) }),
      keywords: `preset ${id} theme look`,
      run: () => ctx.patch(BUILTIN_PRESETS[id]),
    });
  }
  for (const name of Object.keys(settings.customPresets ?? {}).sort()) {
    out.push({
      id: `preset:custom:${name}`,
      group: 'action',
      title: t('palette.preset', { name }),
      keywords: 'preset custom',
      run: () => ctx.patch(settings.customPresets[name] as SettingsPatch),
    });
  }

  // ---- setting toggles ----
  for (const [key, labelKey] of TOGGLES) {
    const current = settings[key];
    out.push({
      id: `toggle:${key}`,
      group: 'setting',
      title: t('palette.toggle', { name: t(labelKey) }),
      keywords: key,
      hint: on(current),
      run: () => ctx.patch({ [key]: !current } as SettingsPatch),
    });
  }
  for (const key of SIDEBAR_ITEM_KEYS) {
    const current = settings.sidebarItems[key];
    out.push({
      id: `toggle:sidebarItems.${key}`,
      group: 'setting',
      title: t('palette.toggle', { name: t(`settings.sidebarItems.${key}`) }),
      keywords: `bar sidebar ${key}`,
      hint: on(current),
      run: () => ctx.patch({ sidebarItems: { [key]: !current } } as SettingsPatch),
    });
  }
  return out;
}
