// Extra accounts (`Settings.accounts`): validation mirrored from the backend
// (`settings::clamp_accounts`), id generation and the sign-in hint. Rune-free so
// `tests/accounts.unit.ts` can exercise it. [FRONTEND]
import type { AccountSettings } from './types';

/** Backend cap (`settings::MAX_ACCOUNTS`). */
export const MAX_ACCOUNTS = 6;
export const MAX_LABEL_CHARS = 40;
export const MAX_ID_CHARS = 24;
export const ACCOUNT_PROVIDERS: ReadonlyArray<AccountSettings['provider']> = ['claude', 'codex'];

/** `[a-z0-9-]{1,24}` */
export const isValidAccountId = (id: string): boolean => /^[a-z0-9-]{1,24}$/.test(id);

/** Absolute on POSIX (`/…`, `~` is not expanded) or Windows (`C:\…`, `C:/…`, `\\server\share`). */
export function isAbsolutePath(path: string): boolean {
  const p = path.trim();
  return p.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(p) || p.startsWith('\\\\');
}

/** Why a draft cannot be saved: i18n keys, empty = fine. */
export type AccountError =
  | 'settings.accounts.err.provider'
  | 'settings.accounts.err.label'
  | 'settings.accounts.err.labelLong'
  | 'settings.accounts.err.dir'
  | 'settings.accounts.err.id'
  | 'settings.accounts.err.limit';

/**
 * `editing` is the id of the entry being edited (null for a new one), so an
 * edit does not collide with itself.
 */
export function accountErrors(draft: AccountSettings, existing: readonly AccountSettings[], editing: string | null): AccountError[] {
  const errors: AccountError[] = [];
  if (!ACCOUNT_PROVIDERS.includes(draft.provider)) errors.push('settings.accounts.err.provider');
  const label = draft.label.trim();
  if (label === '') errors.push('settings.accounts.err.label');
  else if ([...label].length > MAX_LABEL_CHARS) errors.push('settings.accounts.err.labelLong');
  if (!isAbsolutePath(draft.configDir)) errors.push('settings.accounts.err.dir');
  const others = existing.filter((a) => a.id !== editing);
  if (!isValidAccountId(draft.id) || others.some((a) => a.id === draft.id)) errors.push('settings.accounts.err.id');
  if (editing === null && existing.length >= MAX_ACCOUNTS) errors.push('settings.accounts.err.limit');
  return errors;
}

/** A unique slug derived from the label ("Work account" → "work-account", "work-account-2" when taken). */
export function suggestAccountId(label: string, existing: readonly AccountSettings[]): string {
  const base =
    label
      .toLowerCase()
      .normalize('NFKD')
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, MAX_ID_CHARS - 3)
      .replace(/-+$/g, '') || 'account';
  const taken = new Set(existing.map((a) => a.id));
  if (!taken.has(base)) return base;
  for (let n = 2; n < 100; n++) {
    const candidate = `${base}-${n}`;
    if (!taken.has(candidate)) return candidate;
  }
  return `${base}-${Date.now() % 1000}`;
}

/**
 * Shell lines that sign in a second account. Shown as a hint only: the app
 * never runs them and never reads what they write.
 */
export function signInCommands(provider: AccountSettings['provider'], dir: string): string[] {
  const d = dir.trim() === '' ? '<dir>' : dir.trim();
  const quoted = /\s/.test(d) ? `"${d}"` : d;
  return provider === 'claude'
    ? [`CLAUDE_CONFIG_DIR=${quoted} claude`, '/login']
    : [`CODEX_HOME=${quoted} codex login`];
}

/**
 * History account selector: `all`, `primary` (accounts without an id) or an
 * extra account's id.
 */
export function accountMatches(account: string | null | undefined, filter: string): boolean {
  if (filter === 'all') return true;
  if (filter === 'primary') return !account;
  return account === filter;
}

/** Display name of a quota's account: the label, or null for the primary account. */
export function accountName(accounts: readonly AccountSettings[], id: string | null | undefined): string | null {
  if (!id) return null;
  return accounts.find((a) => a.id === id)?.label ?? id;
}

/**
 * The query value of a selector choice: `all` → null (every account),
 * `primary` → '' (the primary account only), an id → that extra account.
 * Matches `HistoryQuery.account` and friends.
 */
export function accountQuery(filter: string): string | null {
  if (filter === 'all') return null;
  return filter === 'primary' ? '' : filter;
}

/** Selector choice of an account id found in data: `primary` for none. */
export function accountFilterOf(account: string | null | undefined): string {
  return account ? account : 'primary';
}

/**
 * The extra accounts a selector offers: the configured ones, then ids that only
 * appear in stored history (an account that was removed keeps its rows),
 * labelled by id. Empty while no extra account exists, which hides the selector.
 */
export function accountChoices(accounts: readonly AccountSettings[], seen: Iterable<string | null | undefined> = []): Array<{ id: string; label: string; removed: boolean }> {
  const out = accounts.map((a) => ({ id: a.id, label: a.label, removed: false }));
  for (const id of seen) {
    if (id && !out.some((c) => c.id === id)) out.push({ id, label: id, removed: true });
  }
  return out;
}

/** Ids of the extra accounts in a `byAccount` map (`claude@work` → `work`). */
export function accountIdsOf(byAccount: Record<string, unknown> | undefined): string[] {
  return Object.keys(byAccount ?? {}).flatMap((key) => {
    const at = key.indexOf('@');
    return at > 0 ? [key.slice(at + 1)] : [];
  });
}

/** `provider` or `provider@account` (mirrors the backend's `provider_key`). */
export const providerKey = (provider: string, account?: string | null): string => (account ? `${provider}@${account}` : provider);
