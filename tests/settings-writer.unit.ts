import { test, expect } from '@playwright/test';
import { mergeSettings, SettingsWriter, type SettingsPatch } from '../src/lib/settings-writer';
import { mockSettings } from '../src/lib/mock';
import type { Settings } from '../src/lib/types';

// Robustness of the frontend merge against partial or odd payloads: an older
// or newer backend, or a hand-edited settings.json echoed back, must never
// throw in the UI or wipe the groups a patch does not mention.

const partial = (omit: string[]): Settings => {
  const copy = structuredClone(mockSettings) as unknown as Record<string, unknown>;
  for (const key of omit) delete copy[key];
  return copy as unknown as Settings;
};

test('merging never mutates its inputs', () => {
  const base = structuredClone(mockSettings);
  const patch: SettingsPatch = { colors: { claude: '#010203' }, providers: { claude: { enabled: false } } };
  const frozenBase = JSON.stringify(base);
  const frozenPatch = JSON.stringify(patch);
  const out = mergeSettings(base, patch);
  expect(JSON.stringify(base)).toBe(frozenBase);
  expect(JSON.stringify(patch)).toBe(frozenPatch);
  expect(out.colors.claude).toBe('#010203');
  expect(out.providers.claude.enabled).toBe(false);
  expect(out.providers.codex).toEqual(base.providers.codex);
});

test('a base that lacks the nested groups still merges into complete groups', () => {
  const base = partial(['colors', 'sizes', 'thresholds', 'sidebarItems', 'subscriptionUsd', 'webhook']);
  const out = mergeSettings(base, { colors: { claude: '#abcdef' }, sizes: { ringSize: 60 }, webhook: { enabled: false } });
  expect(out.colors.claude).toBe('#abcdef');
  expect(out.sizes.ringSize).toBe(60);
  expect(out.webhook.enabled).toBe(false);
  expect(out.thresholds).toEqual({});
  expect(out.theme).toBe(mockSettings.theme);
});

test('a base without providers gets the patched provider with safe defaults', () => {
  const out = mergeSettings(partial(['providers']), { providers: { claude: { enabled: false } } });
  expect(out.providers.claude).toEqual({ enabled: false, showInSidebar: true, order: 0 });
});

test('an unknown provider id in a patch is created, existing ones untouched', () => {
  const out = mergeSettings(mockSettings, { providers: { future: { order: 9 } } });
  expect(out.providers.future).toEqual({ enabled: true, showInSidebar: true, order: 9 });
  expect(out.providers.claude).toEqual(mockSettings.providers.claude);
});

test('null, array and scalar group patches do not throw or corrupt other keys', () => {
  const odd = [null, [1, 2], 5, 'x', true] as unknown[];
  for (const value of odd) {
    const patch = { colors: value, sizes: value, thresholds: value, sidebarItems: value, webhook: value } as unknown as SettingsPatch;
    const out = mergeSettings(mockSettings, patch);
    expect(out.theme).toBe(mockSettings.theme);
    expect(out.colors.codex).toBe(mockSettings.colors.codex);
    expect(out.sizes.ringSize).toBe(mockSettings.sizes.ringSize);
    expect(out.thresholds.warn).toBe(mockSettings.thresholds.warn);
  }
});

test('an empty patch is the identity on values', () => {
  expect(mergeSettings(mockSettings, {})).toEqual(mockSettings);
});

test('a payload from the backend that lacks newer keys is accepted and later edits still apply', async () => {
  // e.g. a settings-updated event from an older process, or a minimal file
  let visible = structuredClone(mockSettings);
  const writer = new SettingsWriter(visible, async (patch) => mergeSettings(visible, patch),
    (value) => { visible = value; }, (error) => { throw error; });
  writer.receive(partial(['customPresets', 'accounts', 'webhook', 'sidebarItems']));
  expect(visible.theme).toBe(mockSettings.theme);
  await writer.patch({ theme: 'light', sidebarItems: { logo: false } });
  expect(visible.theme).toBe('light');
  expect(visible.sidebarItems.logo).toBe(false);
});

test('a failed save does not stop later saves, and the queue drains in order', async () => {
  const seen: SettingsPatch[] = [];
  let visible = structuredClone(mockSettings);
  const errors: unknown[] = [];
  const writer = new SettingsWriter(visible, async (patch) => {
    seen.push(patch);
    if (seen.length === 2) throw new Error('settings.json is read-only');
    return mergeSettings(visible, patch);
  }, (value) => { visible = value; }, (error) => errors.push(error));
  await Promise.all([writer.patch({ scale: 1.1 }), writer.patch({ scale: 1.2 }), writer.patch({ opacity: 0.5 })]);
  expect(seen.map((p) => Object.keys(p)[0])).toEqual(['scale', 'scale', 'opacity']);
  expect(errors).toHaveLength(1);
  expect(visible.opacity).toBe(0.5);
  expect(visible.scale).toBe(1.1);
});
