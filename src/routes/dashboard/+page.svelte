<!--
  Window "dashboard" (route "/dashboard"). [FRONTEND]

  A normal decorated window with an opaque themed surface — the only one of the
  three that scrolls, selects text and shows a caret. The platform layer emits
  `dashboard-navigate` when the tray menu / the sidebar asks for a specific tab.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import HistoryTab from '$lib/components/dashboard/HistoryTab.svelte';
  import OverviewTab from '$lib/components/dashboard/OverviewTab.svelte';
  import SettingsTab from '$lib/components/dashboard/SettingsTab.svelte';
  import { onDashboardNavigate, type Unlisten } from '$lib/api';
  import { t } from '$lib/i18n/i18n.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import { applyTheme, markWindow } from '$lib/stores/theme.svelte';
  import type { DashboardTab } from '$lib/types';

  const TABS: DashboardTab[] = ['overview', 'history', 'settings'];

  let tab = $state<DashboardTab>('overview');
  /** resolved palette name; the chart canvas needs to rebuild when it changes */
  let themeKey = $state('dark');

  onMount(() => {
    markWindow('dashboard');
    const disposers: Array<() => void> = [settings.init(), snapshot.init()];

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
</script>

<svelte:head><title>{t('app.name')}</title></svelte:head>

<div class="app">
  <header class="topbar">
    <span class="brand">{t('app.name')}</span>
    <nav class="tabs" aria-label={t('app.name')}>
      {#each TABS as id (id)}
        <button
          class:active={tab === id}
          aria-current={tab === id ? 'page' : undefined}
          onclick={() => (tab = id)}
        >
          {t(`tab.${id}` as 'tab.overview')}
        </button>
      {/each}
    </nav>
  </header>

  <main>
    {#if tab === 'overview'}
      <OverviewTab />
    {:else if tab === 'history'}
      <HistoryTab {themeKey} />
    {:else}
      <SettingsTab />
    {/if}
  </main>
</div>

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

  .tabs {
    display: flex;
    gap: 0.25rem;
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

    main {
      padding: 1rem;
    }
  }
</style>
