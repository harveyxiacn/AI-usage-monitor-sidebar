import { test, expect } from '@playwright/test';
import {
  CARD_IDS,
  CARD_KEYS,
  UNRESET_KEYS,
  isCardModified,
  resetAllPatch,
  resetPatch,
} from '../src/lib/settings-cards';
import { defaultSettings } from '../src/lib/settings-defaults';
import { mockSettings } from '../src/lib/mock';
import { mergeSettings } from '../src/lib/settings-writer';
import type { Settings } from '../src/lib/types';

test('the frontend defaults are exactly what the browser preview starts from', () => {
  // The preview plays an already-onboarded, up-to-date install, so the
  // first-run bookkeeping is the one place it may differ from the defaults.
  const { onboarded: _o, lastSeenVersion: _l, ...preview } = mockSettings;
  const { onboarded: _do, lastSeenVersion: _dl, ...defaults } = defaultSettings;
  expect(defaults).toEqual(preview);
});

test('every setting belongs to exactly one card, except the few nobody resets', () => {
  const owned = CARD_IDS.flatMap((id) => CARD_KEYS[id]);
  expect(new Set(owned).size).toBe(owned.length);
  const all = Object.keys(defaultSettings) as (keyof Settings)[];
  const orphans = all.filter((key) => !owned.includes(key) && !UNRESET_KEYS.includes(key));
  expect(orphans).toEqual([]);
  for (const key of owned) expect(key in defaultSettings, key).toBe(true);
});

test('a card reset restores its own keys and touches nothing else', () => {
  const edited = mergeSettings(defaultSettings, {
    edge: 'left',
    verticalOffset: 120,
    monitor: 'DP-1',
    alwaysOnTop: false,
    theme: 'light',
  });
  const reset = mergeSettings(edited, resetPatch('position', defaultSettings));
  expect(reset.edge).toBe('right');
  expect(reset.verticalOffset).toBe(0);
  expect(reset.monitor).toBeNull();
  expect(reset.alwaysOnTop).toBe(true);
  expect(reset.theme, 'another card is untouched').toBe('light');
});

test('nested groups reset completely, per key', () => {
  const edited = mergeSettings(defaultSettings, {
    sidebarItems: { weekly: false, logo: false },
    colors: { claude: '#000000' },
    sizes: { ringSize: 90 },
    providers: { codex: { enabled: false, order: 5 } },
    thresholds: { warn: 10, critical: 20 },
  });
  expect(mergeSettings(edited, resetPatch('sidebarItems', defaultSettings)).sidebarItems).toEqual(defaultSettings.sidebarItems);
  expect(mergeSettings(edited, resetPatch('sizeColour', defaultSettings)).colors).toEqual(defaultSettings.colors);
  expect(mergeSettings(edited, resetPatch('providers', defaultSettings)).providers).toEqual(defaultSettings.providers);
  expect(mergeSettings(edited, resetPatch('notifications', defaultSettings)).thresholds).toEqual(defaultSettings.thresholds);
});

test('a reset patch is a copy: editing it cannot corrupt the defaults', () => {
  const patch = resetPatch('sidebarItems', defaultSettings);
  (patch.sidebarItems as Record<string, boolean>).logo = false;
  expect(defaultSettings.sidebarItems.logo).toBe(true);
});

test('reset all restores everything but the presets and the schema version', () => {
  const edited = mergeSettings(defaultSettings, {
    language: 'zh-CN',
    scale: 1.4,
    autoHide: true,
    refreshIntervalSec: 600,
    hideAccountEmail: true,
    shortcutToggleSidebar: 'Ctrl+Alt+U',
    monthlyBudgetUsd: 50,
    pricingUrl: 'https://example.com/p.json',
    customPresets: { mine: { theme: 'light' } },
  });
  const reset = mergeSettings(edited, resetAllPatch(defaultSettings));
  expect({ ...reset, customPresets: {} }).toEqual(defaultSettings);
  expect(reset.customPresets, 'the user\'s own presets survive').toEqual({ mine: { theme: 'light' } });
});

test('a card reads as modified only when one of its own keys differs', () => {
  expect(isCardModified('appearance', defaultSettings, defaultSettings)).toBe(false);
  const edited = mergeSettings(defaultSettings, { scale: 1.2, edge: 'top' });
  expect(isCardModified('appearance', edited, defaultSettings)).toBe(true);
  expect(isCardModified('position', edited, defaultSettings)).toBe(true);
  expect(isCardModified('behaviour', edited, defaultSettings)).toBe(false);
  // a provider this build has never heard of is not a modification
  const extra = mergeSettings(defaultSettings, { providers: { future: { enabled: true, showInSidebar: true, order: 9 } } });
  expect(isCardModified('providers', extra, defaultSettings)).toBe(false);
  const off = mergeSettings(defaultSettings, { providers: { claude: { enabled: false } } });
  expect(isCardModified('providers', off, defaultSettings)).toBe(true);
});
