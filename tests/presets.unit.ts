import { test, expect } from '@playwright/test';
import {
  BUILTIN_IDS,
  BUILTIN_PRESETS,
  MAX_CUSTOM_PRESETS,
  MAX_PRESET_NAME,
  addCustomPreset,
  capturePreset,
  presetChanges,
  removeCustomPreset,
} from '../src/lib/presets';
import { diffSettings, formatSettingValue } from '../src/lib/settings-diff';
import { defaultSettings } from '../src/lib/settings-defaults';
import { mergeSettings } from '../src/lib/settings-writer';

test('there are four built-in presets and the screen-share one hides e-mails', () => {
  expect(BUILTIN_IDS).toEqual(['minimal', 'power', 'screenShare', 'cyber']);
  expect(BUILTIN_PRESETS.screenShare.hideAccountEmail).toBe(true);
  expect(BUILTIN_PRESETS.cyber.surfaceStyle).toBe('cyber');
});

test('built-in presets only use keys that exist, and change something from the defaults', () => {
  for (const id of BUILTIN_IDS) {
    for (const key of Object.keys(BUILTIN_PRESETS[id])) expect(key in defaultSettings, `${id}.${key}`).toBe(true);
    expect(presetChanges(defaultSettings, BUILTIN_PRESETS[id]).length, id).toBeGreaterThan(0);
  }
});

test('the preview lists exactly what applying would change, and nothing when already applied', () => {
  const changes = presetChanges(defaultSettings, BUILTIN_PRESETS.cyber);
  expect(changes.map((c) => c.path).sort()).toEqual(['percentPosition', 'surfaceStyle']);
  const applied = mergeSettings(defaultSettings, BUILTIN_PRESETS.cyber);
  expect(presetChanges(applied, BUILTIN_PRESETS.cyber)).toEqual([]);
});

test('nested changes are reported per leaf', () => {
  const changes = presetChanges(defaultSettings, BUILTIN_PRESETS.minimal);
  const paths = changes.map((c) => c.path);
  expect(paths).toContain('sidebarItems.scoped');
  expect(paths).toContain('sidebarItems.percentLabel');
  expect(paths).not.toContain('sidebarItems.logo');
  expect(paths).not.toContain('showScopedRing');
});

test('diffSettings reports before and after and formats values compactly', () => {
  const next = mergeSettings(defaultSettings, { edge: 'left', monitor: 'DP-1', autoHide: true });
  const byPath = Object.fromEntries(diffSettings(defaultSettings, next).map((c) => [c.path, c]));
  expect(byPath.edge).toMatchObject({ before: 'right', after: 'left' });
  expect(formatSettingValue(byPath.autoHide.after)).toBe('on');
  expect(formatSettingValue(byPath.monitor.before)).toBe('—');
  expect(formatSettingValue('')).toBe('""');
  expect(formatSettingValue({ a: 1, b: 2 })).toBe('{2}');
});

test('capturing the current look keeps appearance and sidebar items but not the position', () => {
  const current = mergeSettings(defaultSettings, { theme: 'light', edge: 'top', sizes: { ringSize: 80 }, sidebarItems: { logo: false } });
  const patch = capturePreset(current);
  expect(patch.theme).toBe('light');
  expect(patch.sizes?.ringSize).toBe(80);
  expect(patch.sidebarItems?.logo).toBe(false);
  expect('edge' in patch).toBe(false);
  expect('hideAccountEmail' in patch).toBe(false);
});

test('custom presets: trimmed unique names, replace in place, capped at ten', () => {
  let presets = {};
  const patch = { theme: 'light' as const };
  const first = addCustomPreset(presets, '  Night  ', patch);
  expect(first.ok && Object.keys(first.presets)).toEqual(['Night']);
  expect(addCustomPreset(presets, '   ', patch)).toEqual({ ok: false, reason: 'name' });
  expect(addCustomPreset(presets, 'x'.repeat(MAX_PRESET_NAME + 1), patch)).toEqual({ ok: false, reason: 'name' });
  expect(addCustomPreset(presets, 'x'.repeat(MAX_PRESET_NAME), patch).ok).toBe(true);

  for (let i = 0; i < MAX_CUSTOM_PRESETS; i++) {
    const result = addCustomPreset(presets, `p${i}`, patch);
    expect(result.ok).toBe(true);
    if (result.ok) presets = result.presets;
  }
  expect(addCustomPreset(presets, 'one too many', patch)).toEqual({ ok: false, reason: 'full' });
  expect(addCustomPreset(presets, 'p3', { theme: 'dark' }).ok, 'replacing is always allowed').toBe(true);
  expect(Object.keys(removeCustomPreset(presets, 'p3'))).toHaveLength(MAX_CUSTOM_PRESETS - 1);
  expect(Object.keys(presets)).toHaveLength(MAX_CUSTOM_PRESETS);
});
