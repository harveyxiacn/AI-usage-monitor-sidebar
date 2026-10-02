// Extra accounts: validation (mirror of the backend), keys, history grouping,
// and the guarantee that a snapshot without extra accounts behaves exactly as
// before. See src/lib/accounts.ts.
import { test, expect } from '@playwright/test';
import {
  MAX_ACCOUNTS,
  accountChoices,
  accountErrors,
  accountFilterOf,
  accountIdsOf,
  accountQuery,
  providerKey,
  accountMatches,
  accountName,
  isAbsolutePath,
  isValidAccountId,
  signInCommands,
  suggestAccountId,
} from '../src/lib/accounts';
import { quotaKey, sampleKey, splitQuotaKey } from '../src/lib/providers';
import { levelsOf, windowKey } from '../src/lib/ring-events';
import { buildQuotaHistory } from '../src/lib/quota-history';
import { sparkValues } from '../src/lib/quota-spark';
import { handlePeak, handleSegments } from '../src/lib/sidebar-visuals';
import { barProviders } from '../src/lib/sidebar-items';
import { CARD_KEYS, isCardModified, resetPatch } from '../src/lib/settings-cards';
import { defaultSettings } from '../src/lib/settings-defaults';
import { mergeSettings } from '../src/lib/settings-writer';
import { mockSettings, mockSnapshot } from '../src/lib/mock';
import { historyCsv } from '../src/lib/history';
import { sessionFilters } from '../src/lib/session-insights';
import { subscriptionTotal } from '../src/lib/subscription';
import { todaySummaries } from '../src/lib/today';
import type { AccountSettings, AppSnapshot, HistoryRow, ProviderQuota, QuotaSample, Settings } from '../src/lib/types';

const acct = (over: Partial<AccountSettings> = {}): AccountSettings => ({
  id: 'work',
  provider: 'claude',
  label: 'Work',
  configDir: '/home/me/.claude-work',
  enabled: true,
  ...over,
});

test('account ids are slugs and paths must be absolute', () => {
  for (const ok of ['work', 'a', 'my-acct-2', 'x'.repeat(24)]) expect(isValidAccountId(ok), ok).toBe(true);
  for (const bad of ['', 'Work', 'a_b', 'a b', 'x'.repeat(25), 'é']) expect(isValidAccountId(bad), bad).toBe(false);
  for (const ok of ['/home/me/.claude-work', 'C:\\Users\\me\\.claude', 'C:/Users/me', '\\\\server\\share\\x'])
    expect(isAbsolutePath(ok), ok).toBe(true);
  for (const bad of ['', 'work', '~/.claude', './x', '../x']) expect(isAbsolutePath(bad), bad).toBe(false);
});

test('a draft is rejected for each rule the backend enforces', () => {
  expect(accountErrors(acct(), [], null)).toEqual([]);
  expect(accountErrors(acct({ label: '   ' }), [], null)).toContain('settings.accounts.err.label');
  expect(accountErrors(acct({ label: 'x'.repeat(41) }), [], null)).toContain('settings.accounts.err.labelLong');
  expect(accountErrors(acct({ label: 'x'.repeat(40) }), [], null)).toEqual([]);
  expect(accountErrors(acct({ configDir: 'relative/dir' }), [], null)).toContain('settings.accounts.err.dir');
  expect(accountErrors(acct({ provider: 'copilot' as 'claude' }), [], null)).toContain('settings.accounts.err.provider');
  expect(accountErrors(acct({ id: 'Bad Id' }), [], null)).toContain('settings.accounts.err.id');
  // a duplicate id is only a problem for a different entry
  expect(accountErrors(acct(), [acct()], null)).toContain('settings.accounts.err.id');
  expect(accountErrors(acct(), [acct()], 'work')).toEqual([]);
  // the cap applies to adding, not to editing
  const full = Array.from({ length: MAX_ACCOUNTS }, (_, i) => acct({ id: `a${i}` }));
  expect(accountErrors(acct({ id: 'new' }), full, null)).toContain('settings.accounts.err.limit');
  expect(accountErrors(acct({ id: 'a0' }), full, 'a0')).toEqual([]);
});

test('ids are derived from the label and stay unique', () => {
  expect(suggestAccountId('Work', [])).toBe('work');
  expect(suggestAccountId('My Work Account!', [])).toBe('my-work-account');
  expect(suggestAccountId('Work', [acct()])).toBe('work-2');
  expect(suggestAccountId('Work', [acct(), acct({ id: 'work-2' })])).toBe('work-3');
  expect(suggestAccountId('工作', [])).toBe('account');
  expect(isValidAccountId(suggestAccountId('x'.repeat(60), []))).toBe(true);
});

test('the sign-in hint names the right environment variable and quotes spaces', () => {
  expect(signInCommands('claude', '/h/.claude-work')).toEqual(['CLAUDE_CONFIG_DIR=/h/.claude-work claude', '/login']);
  expect(signInCommands('codex', '/h/.codex-work')).toEqual(['CODEX_HOME=/h/.codex-work codex login']);
  expect(signInCommands('claude', 'C:\\My Dir\\x')[0]).toBe('CLAUDE_CONFIG_DIR="C:\\My Dir\\x" claude');
  expect(signInCommands('codex', '')[0]).toContain('<dir>');
});

test('keys: the primary account is the bare provider id', () => {
  expect(quotaKey({ provider: 'claude' })).toBe('claude');
  expect(quotaKey({ provider: 'claude', accountId: null })).toBe('claude');
  expect(quotaKey({ provider: 'claude', accountId: 'work' })).toBe('claude@work');
  expect(sampleKey({ provider: 'claude', account: 'work' })).toBe('claude@work');
  expect(sampleKey({ provider: 'claude' })).toBe('claude');
  expect(splitQuotaKey('claude')).toEqual({ provider: 'claude', account: '' });
  expect(splitQuotaKey('claude@work')).toEqual({ provider: 'claude', account: 'work' });
});

test('the history account selector filters without touching the primary series', () => {
  expect(accountMatches(undefined, 'all')).toBe(true);
  expect(accountMatches(null, 'primary')).toBe(true);
  expect(accountMatches('work', 'primary')).toBe(false);
  expect(accountMatches('work', 'work')).toBe(true);
  expect(accountMatches(undefined, 'work')).toBe(false);
  expect(accountName([acct()], 'work')).toBe('Work');
  expect(accountName([acct()], 'gone')).toBe('gone');
  expect(accountName([acct()], null)).toBeNull();
});

const sample = (minute: number, usedPercent: number, over: Partial<QuotaSample> = {}): QuotaSample => ({
  provider: 'claude',
  kind: 'five_hour',
  scope: null,
  usedPercent,
  resetsAt: '2026-09-22T01:00:00Z',
  plan: 'Pro',
  ts: new Date(Date.UTC(2026, 8, 22, 0, minute)).toISOString(),
  ...over,
});

test('quota history keeps every account in its own series', () => {
  const samples = [
    sample(0, 10),
    sample(10, 20),
    sample(0, 50, { account: 'work' }),
    sample(10, 60, { account: 'work' }),
  ];
  const series = buildQuotaHistory(samples);
  expect(series).toHaveLength(2);
  const primary = series.find((s) => s.account === null)!;
  const work = series.find((s) => s.account === 'work')!;
  expect(primary.samples.map((s) => s.usedPercent)).toEqual([10, 20]);
  expect(work.samples.map((s) => s.usedPercent)).toEqual([50, 60]);
  // the primary account's series key is exactly what it was before accounts existed
  expect(primary.key).toBe(JSON.stringify(['claude', 'five_hour', null]));
  expect(work.key).not.toBe(primary.key);
});

test('a popover sparkline only reads the samples it was given', () => {
  const now = Date.UTC(2026, 8, 22, 1, 0);
  const picked = sparkValues([sample(0, 10), sample(10, 20)], 'claude', { kind: 'five_hour', scope: null }, now);
  expect(picked).toEqual([10, 20]);
});

function withWork(base: AppSnapshot): AppSnapshot {
  const claude = base.providers.find((p) => p.provider === 'claude')!;
  const work: ProviderQuota = {
    ...structuredClone(claude),
    displayName: 'Claude · Work',
    accountId: 'work',
    accountLabel: 'Work',
    windows: claude.windows.map((w) => ({ ...w, usedPercent: 95 })),
  };
  return { ...base, providers: [...base.providers, work] };
}

test('two accounts of one provider never share a window key, handle segment or bar entry', () => {
  const snap = withWork(structuredClone(mockSnapshot));
  const levels = levelsOf(snap, { warn: 70, critical: 90 });
  const w = snap.providers[0].windows[0];
  expect(levels.has(windowKey('claude', w))).toBe(true);
  expect(levels.has(windowKey('claude@work', w))).toBe(true);
  expect(levels.get(windowKey('claude@work', w))!.percent).toBe(95);
  expect(levels.get(windowKey('claude', w))!.percent).not.toBe(95);

  const s: Settings = structuredClone(mockSettings);
  const segments = handleSegments(snap, s);
  const keys = segments.map((g) => g.key);
  expect(new Set(keys).size).toBe(keys.length);
  expect(keys).toContain('claude');
  expect(keys).toContain('claude@work');
  // the busiest source is the work account, and the title can tell which
  expect(handlePeak(snap, s)).toMatchObject({ provider: 'claude', key: 'claude@work', used: 95 });
  expect(barProviders(snap, s).map((p) => quotaKey(p))).toEqual(['claude', 'claude@work', 'codex']);
});

test('without extra accounts the handle and the keys are what they were', () => {
  const s: Settings = structuredClone(mockSettings);
  const segments = handleSegments(mockSnapshot, s);
  expect(segments.map((g) => g.key)).toEqual(segments.map((g) => g.provider));
  expect(handlePeak(mockSnapshot, s)?.key).toBe(handlePeak(mockSnapshot, s)?.provider);
  for (const q of mockSnapshot.providers) expect(quotaKey(q)).toBe(q.provider);
  expect(defaultSettings.accounts).toEqual([]);
  expect(mockSettings.accounts).toEqual([]);
});

test('accounts live in exactly one settings card and an empty list is not "modified"', () => {
  const owners = Object.entries(CARD_KEYS).filter(([, keys]) => keys.includes('accounts'));
  expect(owners.map(([card]) => card)).toEqual(['accounts']);
  expect(isCardModified('accounts', defaultSettings, defaultSettings)).toBe(false);
  const edited = mergeSettings(defaultSettings, { accounts: [acct()] });
  expect(isCardModified('accounts', edited, defaultSettings)).toBe(true);
  // the list is replaced as a whole, and the card reset empties it
  expect(mergeSettings(edited, { accounts: [acct({ id: 'b' })] }).accounts.map((a) => a.id)).toEqual(['b']);
  expect(mergeSettings(edited, resetPatch('accounts', defaultSettings)).accounts).toEqual([]);
});

test('the account selector maps to the backend query values and lists removed accounts', () => {
  expect(accountQuery('all')).toBeNull();
  expect(accountQuery('primary')).toBe('');
  expect(accountQuery('work')).toBe('work');
  expect(accountFilterOf(null)).toBe('primary');
  expect(accountFilterOf('work')).toBe('work');
  // no extra account anywhere: no choices, so the selector stays hidden
  expect(accountChoices([], [])).toEqual([]);
  expect(accountChoices([acct()], ['work', 'old', null, undefined])).toEqual([
    { id: 'work', label: 'Work', removed: false },
    { id: 'old', label: 'old', removed: true },
  ]);
  expect(accountIdsOf(undefined)).toEqual([]);
  expect(accountIdsOf({ claude: 1, 'claude@work': 2, 'codex@lab': 3 })).toEqual(['work', 'lab']);
  expect(providerKey('claude', 'work')).toBe('claude@work');
  expect(providerKey('claude', null)).toBe('claude');
  expect(providerKey('claude', '')).toBe('claude');
});

test('a plan price belongs to one login: provider key, or provider@account', () => {
  const prices = { claude: 100, codex: 20, 'claude@work': 20 };
  expect(subscriptionTotal({ claude: 100, codex: 20 }, null)).toBe(120);
  expect(subscriptionTotal(prices, null)).toBe(140);
  expect(subscriptionTotal(prices, 'claude')).toBe(120);
  expect(subscriptionTotal(prices, 'claude', '')).toBe(100);
  expect(subscriptionTotal(prices, 'claude', 'work')).toBe(20);
  expect(subscriptionTotal(prices, null, 'work')).toBe(20);
  expect(subscriptionTotal(prices, 'codex', 'work')).toBe(0);
});

test('the CSV gains an account column only when asked (an extra account exists)', () => {
  const row = {
    bucketStart: '2026-09-22T00:00:00+00:00', provider: 'claude', model: 'm', reasoningEffort: null, project: null, account: 'work',
    inputTokens: 1, cacheWriteTokens: 0, cacheReadTokens: 0, outputTokens: 2, reasoningTokens: 0, totalTokens: 3, requests: 1, estimatedCostUsd: null,
  } as HistoryRow;
  const [plain] = historyCsv([row]).split('\r\n');
  expect(plain).not.toContain('account');
  expect(plain.split(',').slice(0, 3)).toEqual(['bucket_start', 'provider', 'model']);
  const [header, record] = historyCsv([row, { ...row, account: null }], true).split('\r\n');
  expect(header.split(',').slice(0, 4)).toEqual(['bucket_start', 'provider', 'account', 'model']);
  expect(record.split(',')[2]).toBe('work');
  expect(historyCsv([{ ...row, account: null }], true).split('\r\n')[1].split(',')[2]).toBe('');
});

test('today summaries split by account once an extra account has usage', () => {
  const t = (totalTokens: number) => ({ inputTokens: 0, cacheWriteTokens: 0, cacheReadTokens: 0, outputTokens: 0, reasoningTokens: 0, totalTokens, requests: 1, estimatedCostUsd: null });
  const at = new Date(2026, 8, 22, 9).toISOString();
  const row = (account: string | null, totalTokens: number) => ({ ...t(totalTokens), bucketStart: at, provider: 'claude', model: null, project: null, ...(account ? { account } : {}) });
  const noExtra = todaySummaries({ rows: [row(null, 10)], totals: t(10), byProvider: { claude: t(10) }, projects: [], costApproximate: false });
  expect([...noExtra.keys()]).toEqual(['claude']);
  const split = todaySummaries({
    rows: [row(null, 10), row('work', 30)], totals: t(40), byProvider: { claude: t(40) },
    byAccount: { claude: t(10), 'claude@work': t(30) }, projects: [], costApproximate: false,
  });
  expect(split.get('claude')!.totals.totalTokens).toBe(10);
  expect(split.get('claude@work')!.totals.totalTokens).toBe(30);
  expect(split.get('claude@work')!.hours[9]).toBe(30);
  expect(split.get('claude')!.hours[9]).toBe(10);
});

test('session filters only carry an account when one is chosen', () => {
  expect('account' in sessionFilters(null, '', null)).toBe(false);
  expect(sessionFilters(null, 'claude', null, '').account).toBe('');
  expect(sessionFilters(null, 'claude', null, 'work').account).toBe('work');
});
