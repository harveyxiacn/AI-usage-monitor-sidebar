<!--
  Settings → Accounts: extra Claude Code / Codex logins next to the primary one.
  Each account is a CLI config directory the app reads exactly like the default
  one: quota and the local session logs (token history, cost, sessions). The
  folder is only checked for existence; nothing in it is ever opened by this
  card. Each account carries its own subscription price (its own ROI). [FRONTEND]
-->
<script lang="ts">
  import { onDestroy } from 'svelte';
  import Toggle from '../Toggle.svelte';
  import SettingsBlock from './SettingsBlock.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import ProviderLogo from '$lib/components/ProviderLogo.svelte';
  import { checkAccountDir, pickAccountFolder } from '$lib/api';
  import {
    ACCOUNT_PROVIDERS,
    MAX_ACCOUNTS,
    MAX_LABEL_CHARS,
    accountErrors,
    providerKey,
    signInCommands,
    suggestAccountId,
  } from '$lib/accounts';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { providerDisplayName } from '$lib/providers';
  import { settings } from '$lib/stores/settings.svelte';
  import type { AccountCheck, AccountSettings } from '$lib/types';

  const accounts = $derived(settings.value.accounts ?? []);

  /** the entry being edited (`null` id = a new one); `null` = the form is closed */
  let draft = $state<AccountSettings | null>(null);
  let editing = $state<string | null>(null);
  let touched = $state(false);
  let check = $state<AccountCheck | null>(null);
  let pickError = $state<string | null>(null);
  let confirmRemove = $state<string | null>(null);
  /** existence check per saved account, shown in the list */
  let listChecks = $state<Record<string, AccountCheck | null>>({});
  let checkSeq = 0;
  let disposed = false;
  onDestroy(() => {
    disposed = true;
    checkSeq++;
  });

  const errors = $derived(draft ? accountErrors(draft, accounts, editing) : []);
  const commands = $derived(draft ? signInCommands(draft.provider, draft.configDir) : []);

  // a saved account's folder status, re-read whenever the list changes
  $effect(() => {
    const list = accounts.map((a) => ({ id: a.id, provider: a.provider, dir: a.configDir }));
    for (const a of list) {
      void checkAccountDir(a.provider, a.dir)
        .then((c) => {
          if (!disposed) listChecks = { ...listChecks, [a.id]: c };
        })
        .catch(() => {
          if (!disposed) listChecks = { ...listChecks, [a.id]: null };
        });
    }
  });

  // the form's own check, debounced while the path is typed
  $effect(() => {
    const d = draft;
    if (!d || d.configDir.trim() === '') {
      check = null;
      return;
    }
    const seq = ++checkSeq;
    const timer = setTimeout(() => {
      void checkAccountDir(d.provider, d.configDir)
        .then((c) => {
          if (!disposed && seq === checkSeq) check = c;
        })
        .catch(() => {
          if (!disposed && seq === checkSeq) check = null;
        });
    }, 250);
    return () => clearTimeout(timer);
  });

  function startAdd() {
    draft = { id: '', provider: 'claude', label: '', configDir: '', enabled: true };
    editing = null;
    touched = false;
    check = null;
    pickError = null;
  }

  function startEdit(a: AccountSettings) {
    draft = { ...a };
    editing = a.id;
    touched = false;
    check = null;
    pickError = null;
  }

  function cancel() {
    draft = null;
    editing = null;
  }

  async function browse() {
    if (!draft) return;
    pickError = null;
    try {
      const picked = await pickAccountFolder();
      if (picked && draft) draft.configDir = picked;
    } catch (e) {
      pickError = String(e);
    }
  }

  async function save() {
    if (!draft) return;
    touched = true;
    const next: AccountSettings = { ...draft, label: draft.label.trim(), configDir: draft.configDir.trim() };
    // a new account gets its slug from the label; an edited one keeps its id (history belongs to it)
    if (editing === null) next.id = suggestAccountId(next.label, accounts);
    if (accountErrors(next, accounts, editing).length > 0) return;
    const list = editing === null ? [...accounts, next] : accounts.map((a) => (a.id === editing ? next : a));
    await settings.patch({ accounts: list });
    draft = null;
    editing = null;
  }

  async function setEnabled(id: string, enabled: boolean) {
    await settings.patch({ accounts: accounts.map((a) => (a.id === id ? { ...a, enabled } : a)) });
  }

  async function remove(id: string) {
    confirmRemove = null;
    const gone = accounts.find((a) => a.id === id);
    // the stored history stays (it is the user's data); only the price entry is reset
    await settings.patch({
      accounts: accounts.filter((a) => a.id !== id),
      ...(gone && (settings.value.subscriptionUsd[providerKey(gone.provider, id)] ?? 0) > 0 ? { subscriptionUsd: { [providerKey(gone.provider, id)]: 0 } } : {}),
    });
  }

  /** Monthly plan price of one extra account, filed under its provider key (`claude@work`). */
  const priceOf = (a: AccountSettings) => settings.value.subscriptionUsd[providerKey(a.provider, a.id)] ?? 0;
  async function setPrice(a: AccountSettings, value: number) {
    await settings.patch({ subscriptionUsd: { [providerKey(a.provider, a.id)]: Math.min(10_000, Math.max(0, value || 0)) } });
  }

  /** "folder found · credentials file found" for the list and the form */
  function statusOf(c: AccountCheck | null): { text: string; tone: 'ok' | 'warn' | 'bad' } | null {
    if (!c) return null;
    if (!c.absolute) return { text: t('settings.accounts.check.relative'), tone: 'bad' };
    if (!c.dirFound) return { text: t('settings.accounts.check.noDir'), tone: 'bad' };
    if (c.keychainOnly) {
      return c.keychainFound
        ? { text: t('settings.accounts.check.keychainFound'), tone: 'ok' }
        : { text: t('settings.accounts.check.keychain', { service: c.keychainService ?? '' }), tone: 'warn' };
    }
    if (!c.credentialsFound) return { text: t('settings.accounts.check.noLogin'), tone: 'warn' };
    return { text: t('settings.accounts.check.ok'), tone: 'ok' };
  }

  const searchText = $derived(
    [
      t('settings.accounts'),
      t('settings.accounts.hint'),
      t('settings.accounts.add'),
      t('settings.accounts.signIn'),
      t('settings.accounts.signIn.claude'),
      t('settings.accounts.signIn.codex'),
      t('settings.accounts.dataNote'),
      t('settings.accounts.price'),
      ...accounts.map((a) => a.label),
    ].join(' ')
  );
</script>

<SettingsCard id="accounts" title={t('settings.accounts')} reset="accounts">
  <SettingsBlock text={searchText}>
    <p class="note first">{t('settings.accounts.hint')}</p>

    {#if accounts.length === 0 && !draft}
      <p class="empty">{t('settings.accounts.none')}</p>
    {/if}

    {#each accounts as a (a.id)}
      {@const status = statusOf(listChecks[a.id] ?? null)}
      <div class="arow" data-account={a.id}>
        <span class="alogo"><ProviderLogo provider={a.provider} size={18} /></span>
        <div class="awho">
          <span class="aname">{providerDisplayName(a.provider)} · {a.label}</span>
          <span class="adir" title={a.configDir}>{a.configDir}</span>
          {#if status}<span class="astatus" data-tone={status.tone}>{status.text}</span>{/if}
        </div>
        <Toggle
          checked={a.enabled}
          label={`${a.label} — ${t('settings.providerEnabled')}`}
          onchange={(v) => void setEnabled(a.id, v)}
        />
        <button class="btn small" onclick={() => startEdit(a)}>{t('settings.accounts.edit')}</button>
        {#if confirmRemove === a.id}
          <button class="btn small danger" onclick={() => void remove(a.id)}>{t('settings.accounts.confirmRemove')}</button>
          <button class="btn small" onclick={() => (confirmRemove = null)}>{t('common.cancel')}</button>
        {:else}
          <button class="btn small" onclick={() => (confirmRemove = a.id)}>{t('common.remove')}</button>
        {/if}
      </div>
      <label class="aprice">
        <span>{t('settings.accounts.price', { label: a.label })}</span>
        <input
          class="field"
          type="number"
          min="0"
          max="10000"
          step="1"
          value={priceOf(a)}
          aria-label={t('settings.accounts.price', { label: a.label })}
          onchange={(e) => void setPrice(a, e.currentTarget.valueAsNumber)}
        />
      </label>
    {/each}

    {#if draft}
      {@const status = statusOf(check)}
      <form class="form" onsubmit={(e) => { e.preventDefault(); void save(); }}>
        <label class="frow">
          <span>{t('settings.accounts.provider')}</span>
          <select class="field" bind:value={draft.provider} disabled={editing !== null}>
            {#each ACCOUNT_PROVIDERS as p (p)}<option value={p}>{providerDisplayName(p)}</option>{/each}
          </select>
        </label>
        <label class="frow">
          <span>{t('settings.accounts.label')}</span>
          <input
            class="field"
            type="text"
            maxlength={MAX_LABEL_CHARS}
            placeholder={t('settings.accounts.label.placeholder')}
            autocomplete="off"
            bind:value={draft.label}
          />
        </label>
        <div class="frow">
          <label for="acct-dir">{t('settings.accounts.dir')}</label>
          <div class="dirctl">
            <input
              id="acct-dir"
              class="field mono"
              type="text"
              spellcheck="false"
              autocomplete="off"
              placeholder={draft.provider === 'claude' ? '/home/you/.claude-work' : '/home/you/.codex-work'}
              bind:value={draft.configDir}
            />
            <button type="button" class="btn small" onclick={() => void browse()}>{t('settings.accounts.browse')}</button>
          </div>
        </div>
        {#if status}<p class="check" data-tone={status.tone} role="status">{status.text}</p>{/if}
        {#if pickError}<p class="err" role="alert">{pickError}</p>{/if}

        <div class="signin">
          <span class="signin-title">{t('settings.accounts.signIn')}</span>
          <p class="note">{t(draft.provider === 'claude' ? 'settings.accounts.signIn.claude' : 'settings.accounts.signIn.codex')}</p>
          {#each commands as line (line)}<code>{line}</code>{/each}
        </div>

        {#if touched && errors.length > 0}
          <ul class="errs" role="alert">
            {#each errors as e (e)}<li>{tDyn(e)}</li>{/each}
          </ul>
        {/if}
        <div class="actions">
          <button type="submit" class="btn primary">{t('common.save')}</button>
          <button type="button" class="btn" onclick={cancel}>{t('common.cancel')}</button>
        </div>
      </form>
    {:else}
      <div class="actions">
        <button class="btn" disabled={accounts.length >= MAX_ACCOUNTS} onclick={startAdd}>{t('settings.accounts.add')}</button>
        {#if accounts.length >= MAX_ACCOUNTS}<span class="note">{t('settings.accounts.err.limit')}</span>{/if}
      </div>
    {/if}
    <p class="note">{t('settings.accounts.dataNote')}</p>
  </SettingsBlock>
</SettingsCard>

<style>
  .first {
    margin-top: 0;
  }

  .empty {
    margin: 0.5rem 0;
    font-size: 0.8125rem;
    color: var(--muted);
  }

  .arow {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4375rem 0 0;
  }

  .alogo {
    display: grid;
    place-items: center;
    width: 1.75rem;
    height: 1.75rem;
    border-radius: 999px;
    background: var(--surface-2);
    flex: none;
  }

  .awho {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    flex: 1 1 auto;
    min-width: 0;
  }

  .aname {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .adir {
    font-size: 0.6875rem;
    color: var(--faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono, ui-monospace, monospace);
  }

  .aprice {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 0.5rem;
    padding: 0.25rem 0 0.4375rem;
    font-size: 0.75rem;
    color: var(--muted);
    border-bottom: 1px solid var(--border);
  }

  .aprice .field {
    width: 5.5rem;
  }

  .astatus,
  .check {
    font-size: 0.6875rem;
    color: var(--muted);
  }

  [data-tone='ok'] {
    color: var(--ok, #2fb344);
  }

  [data-tone='warn'] {
    color: var(--warn);
  }

  [data-tone='bad'] {
    color: var(--critical);
  }

  .check {
    margin: 0.25rem 0 0;
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin: 0.625rem 0;
    padding: 0.75rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: var(--surface-2);
  }

  .frow {
    display: grid;
    grid-template-columns: 7rem minmax(0, 1fr);
    align-items: center;
    gap: 0.5rem;
    font-size: 0.8125rem;
  }

  .dirctl {
    display: flex;
    gap: 0.375rem;
    min-width: 0;
  }

  .dirctl .field {
    flex: 1 1 auto;
    min-width: 0;
  }

  .mono {
    font-family: var(--mono, ui-monospace, monospace);
  }

  .signin {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .signin-title {
    font-size: 0.75rem;
    font-weight: 600;
  }

  .signin code {
    display: block;
    padding: 0.25rem 0.5rem;
    border-radius: 0.375rem;
    background: var(--surface);
    font-size: 0.75rem;
    overflow-wrap: anywhere;
    user-select: all;
  }

  .errs {
    margin: 0;
    padding-left: 1.125rem;
    color: var(--critical);
    font-size: 0.75rem;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-top: 0.375rem;
  }

  .danger {
    color: var(--critical);
  }

  @media (max-width: 34rem) {
    .frow {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
