<!--
  Settings → Integrations: snapshot export for status bars, pause polling,
  backup and restore. Self-contained (own state, reads/writes the settings
  store), so the settings tab only has to mount `<IntegrationsPanel />`. [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Field from '../Field.svelte';
  import Toggle from '../Toggle.svelte';
  import { backupData, getAppInfo, restartApp, restoreData } from '$lib/api';
  import { t } from '$lib/i18n/i18n.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import type { AppInfo, BackupInfo } from '$lib/types';

  const s = $derived(settings.value);

  let info = $state<AppInfo | null>(null);
  onMount(() => {
    void getAppInfo().then((i) => (info = i)).catch(() => {});
  });

  const snapshotPath = $derived.by(() => {
    if (!info) return 'snapshot.json';
    const sep = info.platform === 'windows' ? '\\' : '/';
    return `${info.dataDir.replace(/[\\/]+$/, '')}${sep}snapshot.json`;
  });

  const examples = $derived([
    { id: 'line', label: t('integrations.example.line'), text: 'ai-usage-sidebar --print' },
    { id: 'statusline', label: t('integrations.example.statusline'), text: 'ai-usage-sidebar --print --format statusline' },
    { id: 'json', label: t('integrations.example.json'), text: 'ai-usage-sidebar --print --format json --provider claude' },
    {
      id: 'claude',
      label: t('integrations.example.claudeCode'),
      text: '{ "statusLine": { "type": "command", "command": "ai-usage-sidebar --print --format statusline" } }',
    },
  ]);

  let copiedId = $state<string | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  async function copy(id: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copiedId = id;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copiedId = null), 1800);
    } catch {
      /* clipboard may be unavailable; the text is selectable anyway */
    }
  }

  // ---- backup / restore ----
  let busy = $state<'backup' | 'restore' | null>(null);
  let backupPath = $state<string | null>(null);
  let staged = $state<BackupInfo | null>(null);
  let error = $state<string | null>(null);

  async function backup() {
    busy = 'backup';
    error = null;
    try {
      backupPath = (await backupData()) ?? backupPath;
    } catch (e) {
      error = String(e);
    } finally {
      busy = null;
    }
  }

  async function restore() {
    busy = 'restore';
    error = null;
    try {
      staged = (await restoreData()) ?? staged;
    } catch (e) {
      error = String(e);
    } finally {
      busy = null;
    }
  }

  const stagedDate = $derived(
    staged?.createdAt ? new Date(staged.createdAt).toLocaleString() : t('integrations.restore.unknownDate')
  );
</script>

<article class="card group">
  <h3>{t('integrations.title')}</h3>

  <Field label={t('integrations.pause')} hint={t('integrations.pause.hint')}>
    <Toggle
      checked={s.pollingPaused}
      label={t('integrations.pause')}
      onchange={(v) => void settings.patch({ pollingPaused: v })}
    />
  </Field>

  <Field label={t('integrations.export')} hint={t('integrations.export.hint')}>
    <Toggle
      checked={s.exportSnapshot}
      label={t('integrations.export')}
      onchange={(v) => void settings.patch({ exportSnapshot: v })}
    />
  </Field>

  {#if s.exportSnapshot}
    <Field label={t('integrations.export.file')} wide>
      <code class="path" title={snapshotPath}>{snapshotPath}</code>
    </Field>
  {/if}

  <Field label={t('integrations.examples')} hint={t('integrations.examples.hint')} wide>
    <ul class="examples">
      {#each examples as ex (ex.id)}
        <li>
          <span class="ex-label">{ex.label}</span>
          <code>{ex.text}</code>
          <button class="btn" type="button" onclick={() => void copy(ex.id, ex.text)}>
            {copiedId === ex.id ? t('integrations.copied') : t('integrations.copy')}
          </button>
        </li>
      {/each}
    </ul>
  </Field>

  <h3 class="sub">{t('integrations.backup.title')}</h3>
  <p class="warn" role="note">{t('integrations.backup.warn')}</p>
  <div class="actions">
    <button class="btn" type="button" disabled={busy !== null} onclick={() => void backup()}>
      {busy === 'backup' ? t('integrations.backup.working') : t('integrations.backup.create')}
    </button>
    <button class="btn" type="button" disabled={busy !== null} onclick={() => void restore()}>
      {busy === 'restore' ? t('integrations.backup.working') : t('integrations.restore')}
    </button>
  </div>
  <p class="hint">{t('integrations.restore.hint')}</p>

  {#if backupPath}
    <p class="ok" role="status">{t('integrations.backup.done', { path: backupPath })}</p>
  {/if}
  {#if staged}
    <div class="ok" role="status">
      <span>{t('integrations.restore.ready', { date: stagedDate })}</span>
      <button class="btn" type="button" onclick={() => void restartApp()}>{t('integrations.restart')}</button>
    </div>
  {/if}
  {#if error}
    <p class="err" role="alert">{t('common.error', { message: error })}</p>
  {/if}
</article>

<style>
  .group {
    padding: 0.75rem 1rem 1rem;
    min-width: 0;
  }

  h3 {
    margin: 0 0 0.25rem;
    font-size: 0.875rem;
    font-weight: 600;
  }

  h3.sub {
    margin-top: 0.875rem;
  }

  .path,
  .examples code {
    font-family: var(--mono, ui-monospace, monospace);
    font-size: 0.75rem;
    overflow-wrap: anywhere;
    user-select: all;
  }

  .examples {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .examples li {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 0.125rem 0.5rem;
    align-items: center;
  }

  .ex-label {
    grid-column: 1 / -1;
    font-size: 0.75rem;
    color: var(--muted);
  }

  .warn,
  .hint {
    margin: 0.25rem 0;
    font-size: 0.75rem;
    color: var(--muted);
  }

  .warn {
    color: var(--warn);
  }

  .actions {
    display: flex;
    gap: 0.5rem;
    padding: 0.5rem 0 0.25rem;
  }

  .ok,
  .err {
    margin: 0.5rem 0 0;
    font-size: 0.8125rem;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    overflow-wrap: anywhere;
  }

  .err {
    color: var(--critical, var(--warn));
  }
</style>
