import { test, expect } from '@playwright/test';
import { compactReset } from '../src/lib/countdown';
import { arcPath, forecastArcRange, handleSegments, labelLayout, miniText, splitColumns, willRunOut } from '../src/lib/sidebar-visuals';
import { mockSettings } from '../src/lib/mock';
import type { AppSnapshot, ProviderQuota, QuotaWindow, Settings } from '../src/lib/types';

const NOW = Date.parse('2026-09-15T00:00:00Z');
const MIN = 60_000;
const at = (min: number) => new Date(NOW + min * MIN).toISOString();

test('compactReset is terse, locale-free and rounds like the popover', () => {
  expect(compactReset(null, NOW)).toBe('');
  expect(compactReset('garbage', NOW)).toBe('');
  expect(compactReset(at(45), NOW)).toBe('45m');
  expect(compactReset(at(72), NOW)).toBe('1h12');
  expect(compactReset(at(120), NOW)).toBe('2h00');
  expect(compactReset(at(-5), NOW)).toBe('0m');
  expect(compactReset(at(3 * 24 * 60 + 4 * 60 + 30), NOW)).toBe('3d4h');
});

const base = { content: 'percent', position: 'below', horizontal: false, percent: '73%', reset: '1h12' } as const;

test('label content on a vertical bar', () => {
  expect(labelLayout(base)).toEqual({ center: null, main: '73%', sub: null });
  expect(labelLayout({ ...base, content: 'reset' })).toEqual({ center: null, main: '1h12', sub: null });
  expect(labelLayout({ ...base, content: 'both' })).toEqual({ center: null, main: '73%', sub: '1h12' });
});

test('an unknown reset degrades to the percentage instead of an empty label', () => {
  expect(labelLayout({ ...base, content: 'reset', reset: '' }).main).toBe('73%');
  expect(labelLayout({ ...base, content: 'both', reset: '' })).toEqual({ center: null, main: '73%', sub: null });
});

test('centre position always keeps the percentage in the centre', () => {
  expect(labelLayout({ ...base, position: 'center' })).toEqual({ center: '73%', main: null, sub: null });
  expect(labelLayout({ ...base, position: 'center', content: 'reset' })).toEqual({ center: '73%', main: '1h12', sub: null });
  expect(labelLayout({ ...base, position: 'center', content: 'both' })).toEqual({ center: '73%', main: '1h12', sub: null });
});

test('a horizontal bar writes percent and countdown on one line', () => {
  expect(labelLayout({ ...base, horizontal: true })).toEqual({ center: null, main: '73% · 1h12', sub: null });
  expect(labelLayout({ ...base, horizontal: true, content: 'both' }).main).toBe('73% · 1h12');
  expect(labelLayout({ ...base, horizontal: true, content: 'reset' }).main).toBe('1h12');
  expect(labelLayout({ ...base, horizontal: true, position: 'center' })).toEqual({ center: '73%', main: '1h12', sub: null });
});

test('mini-bar text: percent leads, countdown on its own line (vertical) or joined (horizontal)', () => {
  const both = labelLayout({ ...base, content: 'both' });
  expect(miniText(both, false)).toEqual({ primary: '73%', secondary: '1h12' });
  expect(miniText(labelLayout({ ...base, content: 'both', horizontal: true }), true)).toEqual({ primary: '73% · 1h12', secondary: null });
  expect(miniText(labelLayout({ ...base, position: 'center', content: 'reset' }), false)).toEqual({ primary: '73%', secondary: '1h12' });
  expect(miniText(labelLayout({ ...base, content: 'reset' }), false)).toEqual({ primary: '1h12', secondary: null });
});

test('forecast arc range follows the used/remaining flip and ignores non-forward projections', () => {
  expect(forecastArcRange(40, 80, 'used')).toEqual({ from: 40, to: 80 });
  expect(forecastArcRange(40, 80, 'remaining')).toEqual({ from: 20, to: 60 });
  expect(forecastArcRange(40, 140, 'used')).toEqual({ from: 40, to: 100 });
  expect(forecastArcRange(40, 40, 'used')).toBeNull();
  expect(forecastArcRange(40, 30, 'used')).toBeNull();
  expect(forecastArcRange(null, 80, 'used')).toBeNull();
  expect(forecastArcRange(40, null, 'used')).toBeNull();
});

test('arcPath starts at 12 o clock, flags large arcs and never closes into a full circle', () => {
  expect(arcPath(50, 40, 0, 25)).toBe('M50.000 10.000A40.000 40.000 0 0 1 90.000 50.000');
  expect(arcPath(50, 40, 0, 75)).toContain(' 0 1 1 ');
  expect(arcPath(50, 40, 40, 40)).toBe('');
  const full = arcPath(50, 40, 0, 100);
  expect(full.startsWith('M50.000 10.000')).toBe(true);
  expect(full).not.toBe('');
  expect(full.endsWith('50.000 10.000')).toBe(false);
});

test('willRunOut is true only when the projection reaches the limit', () => {
  expect(willRunOut(100)).toBe(true);
  expect(willRunOut(99.7)).toBe(true);
  expect(willRunOut(85)).toBe(false);
  expect(willRunOut(null)).toBe(false);
});

test('popover columns need three rows; scoped limits go right, otherwise the rows are halved', () => {
  expect(splitColumns([1, 2], [])).toBeNull();
  expect(splitColumns([1], [2])).toBeNull();
  expect(splitColumns([1, 2], [3])).toEqual([[1, 2], [3]]);
  expect(splitColumns([1, 2, 3], [])).toEqual([[1, 2], [3]]);
  expect(splitColumns([1, 2, 3, 4], [])).toEqual([[1, 2], [3, 4]]);
});

const win = (kind: QuotaWindow['kind'], usedPercent: number): QuotaWindow => ({
  kind,
  label: kind,
  windowSeconds: null,
  usedPercent,
  resetsAt: null,
  scope: null,
  isPrimary: kind === 'five_hour',
});
const quota = (provider: 'claude' | 'codex', windows: QuotaWindow[], status: ProviderQuota['status'] = 'ok'): ProviderQuota => ({
  provider,
  displayName: provider,
  plan: null,
  planLabel: null,
  account: null,
  windows,
  extras: [],
  nextAttemptAt: null,
  fetchedAt: new Date(0).toISOString(),
  source: 'api',
  status,
  error: null,
  credits: null,
});
const snap = (providers: ProviderQuota[]): AppSnapshot => ({ providers } as unknown as AppSnapshot);
const settings: Settings = structuredClone(mockSettings);

test('handle segments: one per polled provider, coloured by its own worst window', () => {
  const s = snap([
    quota('claude', [win('five_hour', 20), win('seven_day', 95)]),
    quota('codex', [win('five_hour', 72)]),
  ]);
  const segs = handleSegments(s, settings);
  expect(segs.map((g) => [g.provider, g.severity, g.usedPercent])).toEqual([
    ['claude', 'critical', 95],
    ['codex', 'warn', 72],
  ]);
  expect(segs[0].accent).toBe('var(--critical)');
  expect(segs[1].accent).toBe('var(--warn)');
});

test('handle segments skip disabled providers and keep normal ones on their accent', () => {
  const s = snap([quota('claude', [win('five_hour', 10)]), quota('codex', [], 'disabled')]);
  const segs = handleSegments(s, settings);
  expect(segs).toHaveLength(1);
  expect(segs[0].severity).toBe('normal');
  expect(segs[0].accent).toContain('--accent-claude');
  expect(handleSegments(null, settings)).toEqual([]);
});
