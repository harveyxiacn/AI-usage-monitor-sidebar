<!--
  Settings → Backup & history: export / import of settings.json and the undo
  ring (the last 5 versions from before a change). [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import ChangeList from './ChangeList.svelte';
  import PreUpgradeBackups from './PreUpgradeBackups.svelte';
  import SettingsBlock from './SettingsBlock.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import { exportSettings, getSettingsHistory, importSettings, restoreSettingsVersion } from '$lib/api';
  import { t } from '$lib/i18n/i18n.svelte';
  import { diffSettings, type SettingChange } from '$lib/settings-diff';
  import { settings } from '$lib/stores/settings.svelte';
  import type { SettingsVersion } from '$lib/types';

  const s = $derived(settings.value);

  let versions = $state<SettingsVersion[]>([]);
  let error = $state<string | null>(null);
  let message = $state<string | null>(null);
  let imported = $state<{ file: string; changes: SettingChange[]; ignored: string[] } | null>(null);
  let busy = $state(false);

  async function refresh() {
    try { versions = await getSettingsHistory(); }
    catch (e) { error = String(e); }
  }

  onMount(() => void refresh());

  // The ring changes with every settings write; the write lands before the
  // `settings-updated` event, so re-reading shortly after it is enough.
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    void settings.value;
    clearTimeout(timer);
    timer = setTimeout(() => void refresh(), 400);
    return () => clearTimeout(timer);
  });

  async function run(action: () => Promise<void>) {
    busy = true;
    error = null;
    message = null;
    try { await action(); }
    catch (e) { error = String(e); }
    finally { busy = false; }
  }

  const doExport = () => run(async () => {
    imported = null;
    const path = await exportSettings();
    if (path) message = t('settings.backup.exported', { path });
  });

  const doImport = () => run(async () => {
    imported = null;
    const result = await importSettings();
    if (!result) return;
    imported = { file: result.path, changes: diffSettings(result.before, result.after), ignored: result.ignored };
    await refresh();
  });

  const restore = (index: number) => run(async () => {
    imported = null;
    await restoreSettingsVersion(index);
    message = t('settings.backup.restored');
    await refresh();
  });

  const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

  const entries = $derived(
    versions.map((version, index) => ({ version, index, changes: diffSettings(s, version.settings) }))
  );

  const searchText = $derived(
    [t('settings.backup.export'), t('settings.backup.import'), t('settings.backup.hint'), t('settings.backup.undo'), t('settings.backup.recent'), t('settings.backup.recent.hint')].join(' ')
  );
</script>

<SettingsCard id="backup" title={t('settings.card.backup')} wide>
  <SettingsBlock text={searchText}>
    <p class="note">{t('settings.backup.hint')}</p>
    <div class="row">
      <button class="btn" disabled={busy} onclick={() => void doExport()}>{t('settings.backup.export')}</button>
      <button class="btn" disabled={busy} onclick={() => void doImport()}>{t('settings.backup.import')}</button>
      <button class="btn" disabled={busy || versions.length === 0} onclick={() => void restore(0)}>{t('settings.backup.undo')}</button>
      {#if versions.length === 0}<span class="muted small">{t('settings.backup.undoNone')}</span>{/if}
    </div>

    {#if error}<p class="err" role="alert">{t('common.error', { message: error })}</p>{/if}
    {#if message}<p class="ok" role="status">{message}</p>{/if}
    {#if imported}
      <div class="result" role="status">
        <p class="ok">
          {imported.changes.length === 0
            ? t('settings.backup.importedNone', { file: fileName(imported.file) })
            : t('settings.backup.imported', { file: fileName(imported.file), n: imported.changes.length })}
        </p>
        {#if imported.changes.length > 0}<ChangeList changes={imported.changes} />{/if}
        {#if imported.ignored.length > 0}
          <p class="note">{t('settings.backup.ignored', { keys: imported.ignored.join(', ') })}</p>
        {/if}
      </div>
    {/if}

    <h4>{t('settings.backup.recent')}</h4>
    <p class="note">{t('settings.backup.recent.hint')}</p>
    {#if entries.length === 0}
      <p class="muted small">{t('settings.backup.undoNone')}</p>
    {:else}
      <ul class="versions">
        {#each entries as { version, index, changes } (index)}
          <li>
            <details>
              <summary>
                <span>{t('settings.backup.version', { time: new Date(version.replacedAt).toLocaleString() })}</span>
                <span class="muted small">{changes.length === 0 ? t('settings.backup.identical') : t('settings.changes.count', { n: changes.length })}</span>
              </summary>
              <ChangeList {changes} />
            </details>
            <button class="btn" disabled={busy || changes.length === 0} onclick={() => void restore(index)}>{t('settings.backup.restore')}</button>
          </li>
        {/each}
      </ul>
    {/if}
  </SettingsBlock>
  <PreUpgradeBackups />
</SettingsCard>

<style>
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }

  h4 {
    margin: 0.875rem 0 0.25rem;
    font-size: 0.8125rem;
    font-weight: 600;
  }

  .result {
    margin-top: 0.5rem;
    padding: 0.25rem 0.75rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--r-control);
    background: var(--surface-2);
  }

  .versions {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .versions li {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.375rem 0;
    border-bottom: 1px solid var(--border);
  }

  .versions li:last-child {
    border-bottom: none;
  }

  details {
    flex: 1 1 auto;
    min-width: 0;
  }

  summary {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 0.75rem;
    cursor: pointer;
  }
</style>
