import { test, expect } from '@playwright/test';
import { matchesQuery, normalizeQuery } from '../src/lib/settings-search';
import { formatDiagnostics } from '../src/lib/diagnostics-format';
import { PRIVACY_ITEMS, privacyItemActive } from '../src/lib/privacy-network';
import { defaultSettings } from '../src/lib/settings-defaults';
import { mergeSettings } from '../src/lib/settings-writer';
import { readFileSync } from 'node:fs';
import type { Diagnostics } from '../src/lib/types';

const catalogue = (name: string): Record<string, string> =>
  JSON.parse(readFileSync(new URL(`../src/lib/i18n/${name}.json`, import.meta.url), 'utf8'));
const en = catalogue('en');
const zh = catalogue('zh-CN');

test('an empty query matches everything; terms are case-insensitive and all required', () => {
  expect(matchesQuery('', 'Theme')).toBe(true);
  expect(matchesQuery('   ', undefined)).toBe(true);
  expect(matchesQuery('THEME', 'Colour theme')).toBe(true);
  expect(matchesQuery('ring percent', 'Ring mode', 'Show the percent in the centre')).toBe(true);
  expect(matchesQuery('ring banana', 'Ring mode', 'Show the percent')).toBe(false);
  expect(normalizeQuery('  A   b ')).toBe('a b');
});

test('search works on Chinese labels too', () => {
  expect(matchesQuery('主题', '主题', '深色或浅色')).toBe(true);
  expect(matchesQuery('快捷键', '主题', '深色或浅色')).toBe(false);
});

test('every privacy row has texts in both languages and a real controlling setting', () => {
  for (const item of PRIVACY_ITEMS) {
    for (const part of ['title', 'detail', 'control']) {
      const key = `privacy.item.${item.id}.${part}`;
      expect(en[key], key).toBeTruthy();
      expect(zh[key], key).toBeTruthy();
    }
    if (item.setting) expect(item.setting in defaultSettings, item.id).toBe(true);
  }
  expect(PRIVACY_ITEMS.filter((i) => i.kind === 'request').map((i) => i.id)).toEqual(
    ['claude', 'codex', 'copilot', 'openrouter', 'update', 'pricing', 'assessment', 'webhook'],
  );
});

test('a privacy row follows its setting; user-initiated rows have no switch', () => {
  const byId = Object.fromEntries(PRIVACY_ITEMS.map((i) => [i.id, i]));
  expect(privacyItemActive(byId.claude, defaultSettings)).toBe(true);
  expect(privacyItemActive(byId.copilot, defaultSettings), 'experimental provider is off by default').toBe(false);
  const quiet = mergeSettings(defaultSettings, {
    providers: { claude: { enabled: false } },
    autoUpdateCheck: false,
    autoPricingCheck: false,
    ingestEnabled: false,
  });
  expect(privacyItemActive(byId.claude, quiet)).toBe(false);
  expect(privacyItemActive(byId.update, quiet)).toBe(false);
  expect(privacyItemActive(byId.pricing, quiet)).toBe(false);
  expect(privacyItemActive(byId.sessionLogs, quiet)).toBe(false);
  expect(privacyItemActive(byId.assessment, quiet)).toBeNull();
  expect(privacyItemActive(byId.ownFiles, quiet)).toBeNull();
});

test('the diagnostics text carries the facts and the masked data it was given', () => {
  const report: Diagnostics = {
    appVersion: '0.6.0',
    os: 'linux',
    arch: 'x86_64',
    backend: 'x11',
    sessionType: 'wayland',
    providers: [
      { id: 'claude', displayName: 'Claude', enabled: true, experimental: false, loggedIn: true, status: 'ok', planLabel: 'Max', account: 'h•••@g•••.com', error: null, fetchedAt: '2026-10-01T00:00:00Z' },
      { id: 'copilot', displayName: 'Copilot', enabled: false, experimental: true, loggedIn: false, status: null, planLabel: null, account: null, error: 'boom', fetchedAt: null },
    ],
    settings: { edge: 'right' },
    logDir: '/l', configDir: '/c', dataDir: '/d', logFile: 'a.log', logTail: 'line',
  };
  const text = formatDiagnostics(report);
  for (const part of ['0.6.0', 'linux x86_64', 'x11 (session: wayland)', 'h•••@g•••.com', 'copilot: disabled', 'experimental', 'error: boom', '"edge": "right"', 'Log folder: /l', 'a.log']) {
    expect(text, part).toContain(part);
  }
  expect(formatDiagnostics({ ...report, logTail: '', logFile: null })).toContain('(no log yet)');
});
