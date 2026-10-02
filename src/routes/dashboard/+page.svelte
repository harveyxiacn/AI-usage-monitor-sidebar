<!--
  Window "dashboard" (route "/dashboard"). [FRONTEND]

  A normal decorated window with an opaque themed surface — the only one of the
  three that scrolls, selects text and shows a caret. The platform layer emits
  `dashboard-navigate` when the tray menu / the sidebar asks for a specific tab.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import CommandPalette from '$lib/components/dashboard/CommandPalette.svelte';
  import OverviewTab from '$lib/components/dashboard/OverviewTab.svelte';
  import OnboardingWizard from '$lib/components/dashboard/OnboardingWizard.svelte';
  import WhatsNew from '$lib/components/dashboard/WhatsNew.svelte';
  import ShareCardModal from '$lib/components/dashboard/ShareCardModal.svelte';
  import { st } from '$lib/session-labels.svelte';
  import { isTauri, onDashboardNavigate, type Unlisten } from '$lib/api';
  import { DASHBOARD_TAB_EVENT } from '$lib/dashboard-nav';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { overlays } from '$lib/stores/overlays.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import { applyTheme, markWindow } from '$lib/stores/theme.svelte';
  import { update } from '$lib/stores/update.svelte';
  import { shouldShowWizard } from '$lib/onboarding';
  import { downloadPercent, formatBytes, updateBannerVisible } from '$lib/update-banner';
  import { pricingUpdate } from '$lib/stores/pricing-update.svelte';
  import { getDbStatus } from '$lib/api';
  import type { DbStatus } from '$lib/types';
  import type { DashboardTab } from '$lib/types';

  // stamped before the first applyTheme() effect so the theme store knows
  // which window it is (the custom text colour is widget-only)
  markWindow('dashboard');

  /** the key the palette hint advertises: Cmd+K on macOS, Ctrl+K elsewhere */
  const paletteKey = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform) ? '⌘K' : 'Ctrl K';

  const TABS: DashboardTab[] = ['overview', 'history', 'sessions', 'settings'];
  let HistoryTab = $state<typeof import('$lib/components/dashboard/HistoryTab.svelte').default | null>(null);
  let SettingsTab = $state<typeof import('$lib/components/dashboard/SettingsTab.svelte').default | null>(null);
  let SessionsTab = $state<typeof import('$lib/components/dashboard/SessionsTab.svelte').default | null>(null);
  let tabError = $state('');
  async function loadTab(target: DashboardTab) {
    tabError = '';
    try {
      if (target === 'history' && !HistoryTab) HistoryTab = (await import('$lib/components/dashboard/HistoryTab.svelte')).default;
      if (target === 'settings' && !SettingsTab) SettingsTab = (await import('$lib/components/dashboard/SettingsTab.svelte')).default;
      if (target === 'sessions' && !SessionsTab) SessionsTab = (await import('$lib/components/dashboard/SessionsTab.svelte')).default;
    } catch (e) { if (target === tab) tabError = String(e); }
  }

  let tab = $state<DashboardTab>('overview');
  /** resolved palette name; the chart canvas needs to rebuild when it changes */
  let themeKey = $state('dark');
  /** the update banner is a nudge, not a modal: one click makes it go away */
  let updateDismissed = $state(false);
  /** why usage.db is not open (a newer app wrote it, a migration was aborted), if so */
  let dbIssue = $state<DbStatus | null>(null);
  /** A new revision resets this acknowledgement; an update is never hidden forever. */
  let pricingUpdateDismissedRevision = $state<string | null>(null);
  const pricingRevision = $derived(pricingUpdate.value?.revision ?? 'unknown');

  onMount(() => {
    const requestedTab = new URLSearchParams(window.location.search).get('tab') as DashboardTab;
    if (TABS.includes(requestedTab)) tab = requestedTab;
    void getDbStatus().then((s) => { if (s.state !== 'ok') dbIssue = s; }).catch(() => {});
    const onTabRequest = (e: Event) => { const next = (e as CustomEvent<DashboardTab>).detail; if (TABS.includes(next)) tab = next; };
    window.addEventListener(DASHBOARD_TAB_EVENT, onTabRequest);
    const disposers: Array<() => void> = [() => window.removeEventListener(DASHBOARD_TAB_EVENT, onTabRequest), settings.init(), snapshot.init(), update.init(), pricingUpdate.init()];

    // applyTheme() writes data-theme on <html>; watching the attribute also
    // catches the prefers-color-scheme listener firing under theme 'auto'.
    const root = document.documentElement;
    const syncTheme = () => (themeKey = root.dataset.theme ?? 'dark');
    syncTheme();
    const mo = new MutationObserver(syncTheme);
    mo.observe(root, { attributes: true, attributeFilter: ['data-theme'] });

    let un: Unlisten | null = null;
    let disposed = false;
    void onDashboardNavigate(({ tab: next }) => {
      if (TABS.includes(next)) tab = next;
    }).then((u) => (disposed ? u() : (un = u)));

    return () => {
      disposed = true;
      un?.();
      mo.disconnect();
      disposers.forEach((d) => d());
    };
  });

  $effect(() => applyTheme(settings.value));
  $effect(() => { void loadTab(tab); });
</script>

<svelte:head><title>{t('app.name')}</title></svelte:head>

<div class="app">
  <header class="topbar">
    <span class="brand">{t('app.name')}</span>
    {#if !isTauri()}<span class="preview-badge">{t('dashboard.preview')}</span>{/if}
    <nav class="tabs" aria-label={t('app.name')}>
      {#each TABS as id (id)}
        <button
          class:active={tab === id}
          aria-current={tab === id ? 'page' : undefined}
          onclick={() => (tab = id)}
        >
          {id === 'sessions' ? st('title') : t(`tab.${id}` as 'tab.overview')}
        </button>
      {/each}
    </nav>
    <div class="topbar-actions">
      <button class="btn" type="button" onclick={() => (overlays.share = true)}>{t('share.open')}</button>
      <button
        class="btn palette-hint"
        type="button"
        aria-label={t('palette.open')}
        aria-keyshortcuts="Control+K Meta+K"
        title={t('palette.open')}
        onclick={() => (overlays.palette = true)}
      ><kbd>{paletteKey}</kbd></button>
    </div>
  </header>

  {#if updateBannerVisible(update.available, settings.value.skippedVersion, updateDismissed)}
    <aside class="update-bar">
      {#if update.value?.installing}
        {@const percent = downloadPercent(update.value.downloaded, update.value.total)}
        <span role="status">
          {#if percent === null}
            {t('update.downloadingUnknown', { done: formatBytes(update.value.downloaded) })}
          {:else}
            {t('update.downloadingSize', { percent: percent ?? 0, done: formatBytes(update.value.downloaded), total: formatBytes(update.value.total ?? 0) })}
          {/if}

        </span>
        <progress max="100" value={percent ?? undefined} aria-label={t('update.installing')}></progress>
      {:else}
        <span>{t('update.banner', { version: update.available ?? '' })}</span>
        <button class="link" onclick={() => (tab = 'settings')}>{t('settings.about.programUpdates')}</button>
        <button class="link" onclick={() => void settings.patch({ skippedVersion: update.available ?? '' })}>{t('update.skip')}</button>
        <button class="link" onclick={() => (updateDismissed = true)}>{t('update.dismiss')}</button>
      {/if}
    </aside>
  {/if}

  {#if dbIssue}
    <aside class="update-bar db-issue-bar" role="alert">
      <span>{tDyn(`dbStatus.${dbIssue.state}`, { found: dbIssue.found ?? '', supported: dbIssue.supported, minReader: dbIssue.minReader ?? '', message: dbIssue.message })}</span>
    </aside>
  {/if}

  {#if pricingUpdate.available && pricingUpdateDismissedRevision !== pricingRevision}
    <aside class="update-bar pricing-update-bar">
      <span>{t('pricingUpdate.banner', { revision: pricingRevision })}</span>
      <button class="link" onclick={() => (tab = 'settings')}>{t('pricingUpdate.openSettings')}</button>
      <button class="link" onclick={() => (pricingUpdateDismissedRevision = pricingRevision)}>{t('pricingUpdate.dismiss')}</button>
    </aside>
  {/if}

  <main>
    <WhatsNew />
    {#if tab === 'overview'}
      <OverviewTab />
    {:else if tab === 'history' && HistoryTab}
      <HistoryTab {themeKey} />
    {:else if tab === 'sessions' && SessionsTab}
      <SessionsTab />
    {:else if tab === 'settings' && SettingsTab}
      <SettingsTab />
    {:else if tabError}
      <p role="alert">{tabError} <button class="btn" onclick={() => void loadTab(tab)}>{t('common.retry')}</button></p>
    {:else}
      <p role="status">{t('common.loading')}</p>
    {/if}
  </main>

  {#if shouldShowWizard(settings.loaded, settings.value.onboarded)}
    <OnboardingWizard />
  {/if}
</div>

<CommandPalette />
<ShareCardModal />

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--bg);
    color: var(--text);
  }

  .topbar {
    display: flex;
    align-items: center;
    gap: 1.25rem;
    flex: none;
    padding: 0.625rem 1.25rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .brand {
    font-weight: 600;
    letter-spacing: -0.01em;
    white-space: nowrap;
  }

  .preview-badge {
    color: var(--muted);
    font-size: 0.6875rem;
  }

  .topbar-actions {
    display: flex;
    gap: 0.5rem;
    margin-left: auto;
  }

  .palette-hint kbd {
    font: inherit;
    font-size: 0.75rem;
    letter-spacing: 0.02em;
  }

  .tabs {
    display: flex;
    gap: 0.25rem;
    flex-wrap: wrap;
  }

  .tabs button {
    position: relative;
    padding: 0.375rem 0.75rem;
    border-radius: var(--r-control);
    color: var(--muted);
    font-weight: 500;
    white-space: nowrap;
    transition: color var(--dur-ui) var(--ease-out), background var(--dur-ui) var(--ease-out);
  }

  .tabs button:hover {
    color: var(--text);
    background: var(--hover);
  }

  .tabs button.active {
    color: var(--text);
    background: var(--surface-2);
  }

  .tabs button.active::after {
    content: '';
    position: absolute;
    left: 0.75rem;
    right: 0.75rem;
    bottom: -0.6875rem;
    height: 2px;
    border-radius: 2px;
    background: var(--focus);
  }

  /* one quiet line under the topbar — no dialog, no colour alarm */
  .update-bar {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex: none;
    flex-wrap: wrap;
    padding: 0.4375rem 1.25rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface-2);
    font-size: 0.8125rem;
  }

  .update-bar .link {
    color: var(--focus);
    text-decoration: underline;
  }

  .update-bar progress {
    width: 10rem;
    max-width: 100%;
    accent-color: var(--focus);
  }


  main {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 1.25rem;
  }

  /* the window minimum is 880×600; below that the topbar stacks */
  @media (max-width: 880px) {
    .topbar {
      flex-direction: column;
      align-items: flex-start;
      gap: 0.5rem;
    }

    .tabs button.active::after {
      display: none;
    }

    .topbar-actions {
      margin-left: 0;
    }

    main {
      padding: 1rem;
    }
  }
</style>
