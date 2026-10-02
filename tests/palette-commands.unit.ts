import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { rankCommands } from '../src/lib/palette';
import { TOGGLES, buildCommands, tomorrowMorning, type PaletteContext } from '../src/lib/palette-commands';
import { BUILTIN_IDS, BUILTIN_PRESETS } from '../src/lib/presets';
import { SECTIONS } from '../src/lib/settings-sections';
import { defaultSettings } from '../src/lib/settings-defaults';
import type { Settings } from '../src/lib/types';

function setup(overrides: Partial<Settings> = {}, now = Date.UTC(2026, 9, 2, 12)) {
  const calls: string[] = [];
  const patches: unknown[] = [];
  const keys = new Set<string>();
  const settings: Settings = { ...structuredClone(defaultSettings), ...overrides };
  const record = (name: string) => () => void calls.push(name);
  const ctx: PaletteContext = {
    settings,
    t: (key, params) => {
      keys.add(key);
      return params ? `${key}${JSON.stringify(params)}` : key;
    },
    sessionsLabel: (key) => `sessions.${key}`,
    now: () => now,
    navigate: (tab) => void calls.push(`tab:${tab}`),
    openHistoryView: (v) => void calls.push(`history:${v}`),
    openSessionsView: (v) => void calls.push(`sessions:${v}`),
    openSettingsCard: (id) => void calls.push(`card:${id}`),
    refresh: record('refresh'),
    rescan: record('rescan'),
    toggleSidebar: record('toggleSidebar'),
    copyDiagnostics: record('copyDiagnostics'),
    exportCsv: record('exportCsv'),
    openLogFolder: record('openLogs'),
    shareCard: record('shareCard'),
    patch: (p) => void patches.push(p),
  };
  const commands = buildCommands(ctx);
  const byId = (id: string) => {
    const c = commands.find((x) => x.id === id);
    if (!c) throw new Error(`no command ${id}`);
    return c;
  };
  return { commands, byId, calls, patches, keys };
}

test('ids are unique and every group is represented', () => {
  const { commands } = setup();
  expect(new Set(commands.map((c) => c.id)).size).toBe(commands.length);
  for (const group of ['navigate', 'action', 'setting']) expect(commands.some((c) => c.group === group)).toBe(true);
});

test('navigation covers the tabs, every History and Sessions sub-view and each Settings card', async () => {
  const { byId, calls } = setup();
  await byId('nav:overview').run();
  await byId('nav:history:usage').run();
  await byId('nav:history:quota').run();
  await byId('nav:history:cost').run();
  await byId('nav:sessions:browse').run();
  await byId('nav:sessions:insights').run();
  await byId('nav:settings').run();
  for (const s of SECTIONS) await byId(`nav:settings:${s.id}`).run();
  expect(calls.slice(0, 7)).toEqual(['tab:overview', 'history:usage', 'history:quota', 'history:cost', 'sessions:browse', 'sessions:insights', 'tab:settings']);
  expect(calls.slice(7)).toEqual(SECTIONS.map((s) => `card:${s.id}`));
});

test('actions call straight through', async () => {
  const { byId, calls } = setup();
  for (const id of ['refresh', 'rescan', 'toggleSidebar', 'shareCard', 'copyDiagnostics', 'exportCsv', 'openLogs']) await byId(`action:${id}`).run();
  expect(calls).toEqual(['refresh', 'rescan', 'toggleSidebar', 'shareCard', 'copyDiagnostics', 'exportCsv', 'openLogs']);
});

test('pause and resume are one command that follows the current state', async () => {
  const running = setup({ pollingPaused: false });
  expect(running.byId('action:pause').title).toBe('palette.action.pause');
  await running.byId('action:pause').run();
  expect(running.patches).toEqual([{ pollingPaused: true }]);
  const paused = setup({ pollingPaused: true });
  expect(paused.byId('action:pause').title).toBe('palette.action.resume');
  await paused.byId('action:pause').run();
  expect(paused.patches).toEqual([{ pollingPaused: false }]);
});

test('focus presets patch focusUntil: an hour, tomorrow 08:00, until turned off, off', async () => {
  const now = new Date(2026, 9, 2, 21, 30).getTime();
  const { byId, patches } = setup({}, now);
  for (const id of ['hour', 'morning', 'forever', 'off']) await byId(`action:focus:${id}`).run();
  expect(patches).toEqual([{ focusUntil: now + 3_600_000 }, { focusUntil: new Date(2026, 9, 3, 8, 0).getTime() }, { focusUntil: -1 }, { focusUntil: 0 }]);
});

test('tomorrowMorning is 08:00 local on the next calendar day', () => {
  const late = new Date(2026, 11, 31, 23, 59).getTime();
  expect(tomorrowMorning(late)).toBe(new Date(2027, 0, 1, 8, 0).getTime());
  const early = new Date(2026, 9, 2, 1, 0).getTime();
  expect(tomorrowMorning(early)).toBe(new Date(2026, 9, 3, 8, 0).getTime());
});

test('built-in and custom presets are applied as their patch', async () => {
  const { byId, patches, commands } = setup({ customPresets: { 'My look': { theme: 'light' } } });
  for (const id of BUILTIN_IDS) await byId(`preset:builtin:${id}`).run();
  await byId('preset:custom:My look').run();
  expect(patches).toEqual([...BUILTIN_IDS.map((id) => BUILTIN_PRESETS[id]), { theme: 'light' }]);
  expect(commands.filter((c) => c.id.startsWith('preset:custom:'))).toHaveLength(1);
});

test('setting toggles flip the current value and show it as a hint', async () => {
  const { byId, patches } = setup({ autoHide: true, hideAccountEmail: false });
  expect(byId('toggle:autoHide').hint).toBe('palette.state.on');
  expect(byId('toggle:hideAccountEmail').hint).toBe('palette.state.off');
  await byId('toggle:autoHide').run();
  await byId('toggle:hideAccountEmail').run();
  expect(patches).toEqual([{ autoHide: false }, { hideAccountEmail: true }]);
});

test('sidebar item toggles patch only their own key', async () => {
  const { byId, patches } = setup();
  await byId('toggle:sidebarItems.logo').run();
  expect(patches).toEqual([{ sidebarItems: { logo: !defaultSettings.sidebarItems.logo } }]);
});

test('every toggle key is a real boolean setting', () => {
  for (const [key] of TOGGLES) expect(typeof defaultSettings[key], key).toBe('boolean');
});

test('searching finds a setting by its key and a page by its words', () => {
  const { commands } = setup();
  const found = (q: string) => rankCommands(q, commands).map((c) => c.id);
  expect(found('autoHide')[0]).toBe('toggle:autoHide');
  expect(found('share')[0]).toBe('action:shareCard');
  expect(found('budget')).toContain('nav:history:cost');
  expect(found('privacy')).toContain('nav:settings:privacy');
});

test('every catalogue key the registry uses exists in both languages', () => {
  const { keys } = setup({ customPresets: { x: {} } });
  const load = (name: string) => JSON.parse(readFileSync(`src/lib/i18n/${name}.json`, 'utf8')) as Record<string, string>;
  const english = load('en');
  const chinese = load('zh-CN');
  for (const key of keys) {
    expect(english[key], `en ${key}`).toBeTruthy();
    expect(chinese[key], `zh ${key}`).toBeTruthy();
  }
});
