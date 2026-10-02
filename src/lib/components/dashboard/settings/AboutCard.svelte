<!--
  Settings → About: version, platform, folders (with copy and open buttons),
  diagnostics for bug reports, GitHub and Quit. [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Field from '../Field.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import WhatsNew from '../WhatsNew.svelte';
  import { getAppInfo, getDiagnostics, openExternal, openFolder, quitApp } from '$lib/api';
  import { formatDiagnostics } from '$lib/diagnostics-format';
  import { t } from '$lib/i18n/i18n.svelte';
  import type { AppInfo } from '$lib/types';

  const GITHUB_URL = 'https://github.com/harveyxiacn/AI-usage-monitor-sidebar';

  let appInfo = $state<AppInfo | null>(null);
  let logDir = $state<string | null>(null);
  let error = $state<string | null>(null);
  let message = $state<string | null>(null);
  let copiedPath = $state<string | null>(null);
  let showWhatsNew = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    void getAppInfo().then((i) => (appInfo = i)).catch((e) => (error = String(e)));
    // the log folder is only reported with the diagnostics
    void getDiagnostics().then((d) => (logDir = d.logDir)).catch(() => {});
    return () => clearTimeout(copyTimer);
  });

  async function run(action: () => Promise<unknown>) {
    error = null;
    message = null;
    try { await action(); } catch (e) { error = String(e); }
  }

  async function writeClipboard(text: string) {
    await navigator.clipboard.writeText(text);
  }

  async function copyPath(path: string | null) {
    if (!path) return;
    await run(async () => {
      await writeClipboard(path);
      copiedPath = path;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copiedPath = null), 1500);
    });
  }

  const copyDiagnostics = () => run(async () => {
    await writeClipboard(formatDiagnostics(await getDiagnostics()));
    message = t('settings.about.diagnosticsCopied');
  });
</script>

<SettingsCard id="about" title={t('settings.about')} keywords={t('settings.about.diagnosticsHint')}>
  {#if error}<p class="err" role="alert">{t('common.error', { message: error })}</p>{/if}

  <Field label={t('settings.about.version')}>
    <span class="mono">{appInfo?.version ?? '—'}</span>
  </Field>
  <Field label={t('settings.about.platform')}>
    <span class="mono">{appInfo?.platform ?? '—'}</span>
  </Field>
  <Field label={t('settings.about.backend')}>
    <span class="mono">{appInfo?.backend ?? '—'}</span>
  </Field>

  {#each [
    { label: t('settings.about.dataDir'), path: appInfo?.dataDir ?? null },
    { label: t('settings.about.configDir'), path: appInfo?.configDir ?? null },
    { label: t('settings.about.logDir'), path: logDir },
  ] as row (row.label)}
    <Field label={row.label}>
      <span class="mono path" title={row.path ?? ''}>{row.path ?? '—'}</span>
      <button class="btn" disabled={!row.path} aria-label={`${t('settings.about.copyPath')}: ${row.label}`} onclick={() => void copyPath(row.path)}>
        {copiedPath !== null && copiedPath === row.path ? t('common.copied') : t('settings.about.copyPath')}
      </button>
    </Field>
  {/each}

  <Field label={t('settings.about.copyDiagnostics')} hint={t('settings.about.diagnosticsHint')}>
    <button class="btn" onclick={() => void copyDiagnostics()}>{t('settings.about.copyDiagnostics')}</button>
  </Field>
  {#if message}<p class="ok" role="status">{message}</p>{/if}

  <div class="actions">
    <button class="btn" aria-expanded={showWhatsNew} onclick={() => (showWhatsNew = !showWhatsNew)}>{t('whatsnew.open')}</button>
    <button class="btn" onclick={() => void run(() => openFolder('log'))}>{t('settings.about.openLogs')}</button>
    <button class="btn" onclick={() => void run(() => openFolder('config'))}>{t('settings.about.openConfig')}</button>
    <button class="btn" onclick={() => void run(() => openExternal(GITHUB_URL))}>{t('settings.about.github')}</button>
    <button class="btn danger" onclick={() => void run(quitApp)}>{t('settings.about.quit')}</button>
  </div>
  {#if showWhatsNew}<WhatsNew forced ondismiss={() => (showWhatsNew = false)} />{/if}
</SettingsCard>

<style>
  .path {
    max-width: 18rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.75rem;
  }
</style>
