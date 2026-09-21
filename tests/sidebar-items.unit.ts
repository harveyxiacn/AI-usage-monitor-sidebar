import { test, expect } from '@playwright/test';
import {
  barGroups,
  barProviders,
  barSeverity,
  labelWindowOf,
  providerOnBar,
  windowOnBar,
  defaultSidebarItems,
} from '../src/lib/sidebar-items';
import { mockSettings, mockSnapshot } from '../src/lib/mock';
import type { AppSnapshot, ProviderQuota, QuotaWindow, Settings, SidebarItems } from '../src/lib/types';

function settingsWith(
  patch: Omit<Partial<Settings>, 'sidebarItems'> & { sidebarItems?: Partial<SidebarItems> }
): Settings {
  return {
    ...structuredClone(mockSettings),
    ...patch,
    sidebarItems: { ...defaultSidebarItems, ...patch.sidebarItems },
  };
}

const win = (
  kind: QuotaWindow['kind'],
  usedPercent: number,
  extra: Partial<QuotaWindow> = {}
): QuotaWindow => ({
  kind,
  label: kind,
  windowSeconds: null,
  usedPercent,
  resetsAt: null,
  scope: null,
  isPrimary: false,
  ...extra,
});

function quota(provider: 'claude' | 'codex', windows: QuotaWindow[]): ProviderQuota {
  return {
    provider,
    displayName: provider,
    plan: null,
    planLabel: null,
    account: null,
    windows,
    fetchedAt: new Date(0).toISOString(),
    source: 'api',
    status: 'ok',
    error: null,
    credits: null,
  };
}

/** Claude as the app really sees it: 5-hour primary, weekly, two scoped weeklies. */
const claude = () =>
  quota('claude', [
    win('five_hour', 73, { isPrimary: true }),
    win('seven_day', 31),
    win('seven_day', 24, { scope: 'Fable' }),
    win('other', 12, { scope: 'Spark' }),
  ]);

const kinds = (groups: QuotaWindow[][]) => groups.map((g) => g.map((w) => `${w.kind}:${w.scope ?? '-'}`));

test('window visibility buckets scoped windows together, whatever their kind', () => {
  const items = { ...defaultSidebarItems, scoped: false, other: false };
  expect(windowOnBar(win('five_hour', 1), items)).toBe(true);
  expect(windowOnBar(win('seven_day', 1), items)).toBe(true);
  expect(windowOnBar(win('other', 1), items)).toBe(false);
  // a per-feature window is "scoped" even though its kind is `other`
  expect(windowOnBar(win('other', 1, { scope: 'Spark' }), items)).toBe(false);
  expect(windowOnBar(win('seven_day', 1, { scope: 'Fable' }), items)).toBe(false);
  expect(windowOnBar(win('other', 1, { scope: 'Spark' }), { ...defaultSidebarItems, other: false })).toBe(true);
});

test('filtering happens before the groups are built, in all three ring modes', () => {
  const q = claude();

  // concentric: outer weekly, inner 5-hour, innermost scoped weekly
  expect(kinds(barGroups(q, settingsWith({ ringMode: 'concentric' })))).toEqual([
    ['seven_day:-', 'five_hour:-', 'seven_day:Fable'],
  ]);
  // "weekly" is the account-wide window only — the per-model one is "scoped"
  expect(
    kinds(barGroups(q, settingsWith({ ringMode: 'concentric', sidebarItems: { weekly: false } })))
  ).toEqual([['five_hour:-', 'seven_day:Fable']]);
  expect(
    kinds(barGroups(q, settingsWith({ ringMode: 'concentric', sidebarItems: { scoped: false } })))
  ).toEqual([['seven_day:-', 'five_hour:-']]);

  // primary: one ring, the primary account-wide window …
  expect(kinds(barGroups(q, settingsWith({ ringMode: 'primary' })))).toEqual([['five_hour:-']]);
  // … and when that one is hidden the weekly window takes the slot
  expect(
    kinds(barGroups(q, settingsWith({ ringMode: 'primary', sidebarItems: { fiveHour: false } })))
  ).toEqual([['seven_day:-']]);

  // all: one ring per account-wide window, filtered the same way
  expect(kinds(barGroups(q, settingsWith({ ringMode: 'all' })))).toEqual([
    ['five_hour:-'],
    ['seven_day:-'],
  ]);
  expect(
    kinds(barGroups(q, settingsWith({ ringMode: 'all', sidebarItems: { weekly: false } })))
  ).toEqual([['five_hour:-']]);
});

test('Codex never gets the scoped inner ring, whatever sidebarItems says', () => {
  const q = quota('codex', [
    win('five_hour', 21, { isPrimary: true }),
    win('seven_day', 41),
    win('other', 4, { scope: 'GPT-5.3-Codex-Spark' }),
  ]);
  expect(kinds(barGroups(q, settingsWith({ ringMode: 'concentric' })))).toEqual([
    ['seven_day:-', 'five_hour:-'],
  ]);
});

test('the label window falls back to the first visible window', () => {
  const q = claude();
  const concentric = barGroups(q, settingsWith({ ringMode: 'concentric' }))[0];
  expect(labelWindowOf(concentric)?.kind).toBe('five_hour');

  // the primary window is hidden: the label describes what is actually drawn
  const withoutFiveHour = barGroups(
    q,
    settingsWith({ ringMode: 'concentric', sidebarItems: { fiveHour: false } })
  )[0];
  expect(labelWindowOf(withoutFiveHour)?.kind).toBe('seven_day');
  expect(labelWindowOf(withoutFiveHour)?.scope).toBe(null);

  expect(labelWindowOf([])).toBe(null);
});

test('a provider whose windows are all hidden leaves the bar, a silent one keeps a placeholder', () => {
  const allHidden: Partial<SidebarItems> = { fiveHour: false, weekly: false, scoped: false, other: false };
  for (const ringMode of ['concentric', 'primary', 'all'] as const) {
    expect(barGroups(claude(), settingsWith({ ringMode, sidebarItems: allHidden }))).toEqual([]);
    // no windows at all (not signed in / nothing cached) is not the same thing:
    // one empty group keeps the dimmed status ring on the bar
    expect(barGroups(quota('claude', []), settingsWith({ ringMode, sidebarItems: allHidden }))).toEqual([[]]);
  }
});

test('showInSidebar hides a provider from the bar without switching it off', () => {
  const snap: AppSnapshot = { generatedAt: '', providers: [claude(), quota('codex', [win('seven_day', 41, { isPrimary: true })])] };
  const hidden = settingsWith({
    providers: {
      claude: { enabled: true, showInSidebar: false, order: 0 },
      codex: { enabled: true, showInSidebar: true, order: 1 },
    },
  });
  expect(barProviders(snap, hidden).map((p) => p.provider)).toEqual(['codex']);
  expect(providerOnBar(claude(), hidden)).toBe(false);

  const off = settingsWith({
    providers: {
      claude: { enabled: false, showInSidebar: true, order: 0 },
      codex: { enabled: true, showInSidebar: true, order: 1 },
    },
  });
  expect(barProviders(snap, off).map((p) => p.provider)).toEqual(['codex']);

  // settings that predate the field show the provider
  const legacy = settingsWith({ providers: { claude: { enabled: true, order: 0 } as never } });
  expect(providerOnBar(claude(), legacy)).toBe(true);
});

test('the bar-wide severity ignores everything that was hidden from the bar', () => {
  const snap: AppSnapshot = {
    generatedAt: '',
    providers: [
      quota('claude', [win('five_hour', 12, { isPrimary: true }), win('seven_day', 95, { scope: 'Fable' })]),
      quota('codex', [win('seven_day', 40, { isPrimary: true })]),
    ],
  };
  const visible = settingsWith({});
  expect(barSeverity(snap, visible)).toEqual({ severity: 'critical', leader: 'claude' });

  // hide the critical window, the whole provider, and every kind: the alarm stays
  const blind = settingsWith({
    sidebarItems: { fiveHour: false, weekly: false, scoped: false, other: false, logo: false, percentLabel: false, moreButton: false },
    providers: {
      claude: { enabled: true, showInSidebar: false, order: 0 },
      codex: { enabled: true, showInSidebar: false, order: 1 },
    },
  });
  expect(barSeverity(snap, blind)).toEqual({ severity: 'critical', leader: 'claude' });
  expect(barProviders(snap, blind)).toEqual([]);

  // a provider switched off entirely is not polled, so it cannot warn
  const disabled = settingsWith({
    providers: {
      claude: { enabled: false, showInSidebar: true, order: 0 },
      codex: { enabled: true, showInSidebar: true, order: 1 },
    },
  });
  expect(barSeverity(snap, disabled)).toEqual({ severity: 'normal', leader: 'codex' });

  expect(barSeverity(null, visible)).toEqual({ severity: 'normal', leader: null });
  expect(barSeverity({ generatedAt: '', providers: [] }, visible)).toEqual({ severity: 'normal', leader: null });
});

test('the shipped mock renders every provider and window kind by default', () => {
  const s = settingsWith({});
  expect(barProviders(mockSnapshot, s)).toHaveLength(2);
  for (const q of barProviders(mockSnapshot, s)) expect(barGroups(q, s)[0].length).toBeGreaterThan(1);
  expect(mockSettings.sidebarItems).toEqual(defaultSidebarItems);
});
