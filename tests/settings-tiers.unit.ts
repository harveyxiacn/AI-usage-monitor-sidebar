import { test, expect } from '@playwright/test';
import { ADVANCED_ONLY_CARDS, CARD_IDS, CARD_KEYS, cardAdvancedOnly } from '../src/lib/settings-cards';
import { defaultSettings } from '../src/lib/settings-defaults';
import { controlShown } from '../src/lib/settings-search';
import { SECTIONS } from '../src/lib/settings-sections';
import {
  ADVANCED_SIDEBAR_ITEMS,
  ADVANCED_STORAGE_KEY,
  SETTING_TIERS,
  isAdvanced,
  loadShowAdvanced,
  saveShowAdvanced,
} from '../src/lib/settings-tiers';
import type { Settings } from '../src/lib/types';

test('every setting has a tier and nothing else is in the table', () => {
  expect(Object.keys(SETTING_TIERS).sort()).toEqual(Object.keys(defaultSettings).sort());
});

test('settings without a control of their own are internal, the rest are basic or advanced', () => {
  const internal = Object.entries(SETTING_TIERS).filter(([, tier]) => tier === 'internal').map(([key]) => key).sort();
  expect(internal).toEqual(['customPresets', 'lastSeenVersion', 'onboarded', 'skippedVersion', 'version']);
  // a setting a card owns and resets is never internal bookkeeping
  for (const id of CARD_IDS) {
    for (const key of CARD_KEYS[id]) {
      if (key === 'skippedVersion') continue;
      expect(SETTING_TIERS[key as keyof Settings], `${id}.${key}`).not.toBe('internal');
    }
  }
});

test('advanced sidebar items name real items and leave the main ones basic', () => {
  for (const item of ADVANCED_SIDEBAR_ITEMS) expect(item in defaultSettings.sidebarItems).toBe(true);
  expect(ADVANCED_SIDEBAR_ITEMS).not.toContain('fiveHour');
  expect(ADVANCED_SIDEBAR_ITEMS).not.toContain('weekly');
});

test('a card whose settings are all advanced only exists with "Show advanced settings"', () => {
  expect([...ADVANCED_ONLY_CARDS].sort()).toEqual(['accounts', 'integrations', 'shortcuts', 'sizeColour']);
  for (const id of ADVANCED_ONLY_CARDS) expect(SECTIONS.map((s) => s.id)).toContain(id);
  // mixed cards stay on the page: they have basic controls
  expect(cardAdvancedOnly('notifications')).toBe(false);
  expect(cardAdvancedOnly('behaviour')).toBe(false);
  expect(isAdvanced('colors')).toBe(true);
  expect(isAdvanced('theme')).toBe(false);
});

test('advanced controls show with the switch or a search, basic ones always', () => {
  const off = { revealsAdvanced: false };
  const on = { revealsAdvanced: true };
  expect(controlShown(off, false, true)).toBe(true);
  expect(controlShown(off, true, true)).toBe(false);
  expect(controlShown(on, true, true)).toBe(true);
  expect(controlShown(on, true, false), 'a search that does not match hides it').toBe(false);
  expect(controlShown(on, false, false)).toBe(false);
  expect(controlShown(undefined, true, true), 'outside the Settings tab everything shows').toBe(true);
});

test('the switch is remembered per viewer and storage failures mean off', () => {
  const data = new Map<string, string>();
  const storage = { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v) };
  expect(loadShowAdvanced(storage)).toBe(false);
  saveShowAdvanced(true, storage);
  expect(data.get(ADVANCED_STORAGE_KEY)).toBe('1');
  expect(loadShowAdvanced(storage)).toBe(true);
  saveShowAdvanced(false, storage);
  expect(loadShowAdvanced(storage)).toBe(false);

  const broken = {
    getItem: () => { throw new Error('blocked'); },
    setItem: () => { throw new Error('blocked'); },
  };
  expect(loadShowAdvanced(broken)).toBe(false);
  expect(() => saveShowAdvanced(true, broken)).not.toThrow();
});
