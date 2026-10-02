<!--
  Dashboard → Settings. Every control applies immediately through
  `settings.patch()` (which persists via `update_settings` and, for the
  window-related keys, calls `apply_window_settings`). [FRONTEND]

  This file is only the frame: a sticky section index on the left (a select on
  narrow windows), the search box, "Reset all", and the cards in
  `./settings/`, which own their controls, their "Reset to defaults" and their
  data. A card hides itself when the search matches none of its controls.

  Controls are basic or advanced (`$lib/settings-tiers`). Only the basic ones
  show until "Show advanced settings" is switched on; a search always finds the
  advanced ones too. The switch is per viewer (localStorage), not a setting.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import AboutCard from './settings/AboutCard.svelte';
  import AccountsCard from './settings/AccountsCard.svelte';
  import AppearanceCard from './settings/AppearanceCard.svelte';
  import BackupCard from './settings/BackupCard.svelte';
  import BehaviourCard from './settings/BehaviourCard.svelte';
  import DataCard from './settings/DataCard.svelte';
  import IntegrationsPanel from './settings/IntegrationsPanel.svelte';
  import AdvisorSettings from './settings/AdvisorSettings.svelte';
  import NotificationsCard from './settings/NotificationsCard.svelte';
  import PositionCard from './settings/PositionCard.svelte';
  import PresetsCard from './settings/PresetsCard.svelte';
  import PrivacyCard from './settings/PrivacyCard.svelte';
  import ProvidersCard from './settings/ProvidersCard.svelte';
  import ShortcutsCard from './settings/ShortcutsCard.svelte';
  import SizeColourCard from './settings/SizeColourCard.svelte';
  import SidebarItemsCard from './settings/SidebarItemsCard.svelte';
  import UpdatesCard from './settings/UpdatesCard.svelte';
  import Toggle from './Toggle.svelte';
  import { t } from '$lib/i18n/i18n.svelte';
  import { resetAllPatch } from '$lib/settings-cards';
  import { SECTIONS } from '$lib/settings-sections';
  import { SettingsSearch, provideSettingsSearch } from '$lib/settings-scope.svelte';
  import { advancedSettings } from '$lib/stores/advanced.svelte';
  import { defaultSettings, settings } from '$lib/stores/settings.svelte';

  const search = new SettingsSearch();
  provideSettingsSearch(search);

  // a card with nothing to show (all advanced and advanced is off, or no search hit) leaves the index
  const shown = $derived(SECTIONS.filter((section) => search.cards.get(section.id) !== false));

  let root = $state<HTMLElement | null>(null);
  let current = $state<string>(SECTIONS[0].id);

  /** The element that scrolls the page (the dashboard's <main>). */
  function scrollParent(el: HTMLElement): HTMLElement | null {
    for (let node = el.parentElement; node; node = node.parentElement) {
      const overflow = getComputedStyle(node).overflowY;
      if (overflow === 'auto' || overflow === 'scroll') return node;
    }
    return null;
  }

  function jump(id: string) {
    const target = root?.querySelector<HTMLElement>(`#${id}`);
    if (!target) return;
    current = id;
    const calm = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    target.scrollIntoView({ behavior: calm ? 'auto' : 'smooth', block: 'start' });
  }

  /** Highlight the last visible section whose top has reached the top of the page. */
  function spy(container: HTMLElement) {
    const line = container.getBoundingClientRect().top + 96;
    let found: string | null = null;
    for (const section of shown) {
      const el = root?.querySelector<HTMLElement>(`#${section.id}`);
      if (el && el.getBoundingClientRect().top <= line) found = section.id;
    }
    current = found ?? shown[0]?.id ?? current;
  }

  onMount(() => {
    if (!root) return;
    const container = scrollParent(root);
    if (!container) return;
    const onScroll = () => spy(container);
    container.addEventListener('scroll', onScroll, { passive: true });
    return () => container.removeEventListener('scroll', onScroll);
  });

  function resetAll() {
    if (!window.confirm(t('settings.resetAll.confirm'))) return;
    void settings.patch(resetAllPatch(defaultSettings));
  }

  function onSearchKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && search.query !== '') {
      e.preventDefault();
      search.query = '';
    }
  }
</script>

<div class="settings" bind:this={root}>
  <aside class="rail">
    <div class="search">
      <input
        class="field"
        type="search"
        bind:value={search.query}
        onkeydown={onSearchKey}
        placeholder={t('settings.search.placeholder')}
        aria-label={t('settings.search')}
        spellcheck="false"
      />
      {#if search.query !== ''}
        <button class="btn clear" aria-label={t('settings.search.clear')} onclick={() => (search.query = '')}>×</button>
      {/if}
    </div>

    <label class="advanced">
      <span class="advanced-text">
        <span>{t('settings.advanced.show')}</span>
        <span class="advanced-hint">{t('settings.advanced.hint')}</span>
      </span>
      <Toggle checked={advancedSettings.on} label={t('settings.advanced.show')} onchange={(v) => advancedSettings.set(v)} />
    </label>

    <nav aria-label={t('settings.nav')}>
      <ul class="index">
        {#each shown as section (section.id)}
          <li>
            <a
              href={`#${section.id}`}
              class:current={current === section.id}
              aria-current={current === section.id ? 'location' : undefined}
              onclick={(e) => { e.preventDefault(); jump(section.id); }}
            >{t(section.title)}</a>
          </li>
        {/each}
      </ul>
      <select
        class="field jump"
        aria-label={t('settings.nav.jump')}
        value={current}
        onchange={(e) => jump(e.currentTarget.value)}
      >
        {#each shown as section (section.id)}<option value={section.id}>{t(section.title)}</option>{/each}
      </select>
    </nav>

    <button class="btn reset-all" onclick={resetAll}>{t('settings.resetAll')}</button>
  </aside>

  <section class="cards">
    {#if settings.error}
      <p class="err" role="alert">{t('common.error', { message: settings.error })}</p>
    {/if}
    {#if settings.saving}
      <p class="save-state muted" role="status">{t('common.saving')}</p>
    {/if}
    {#if !search.anyVisible}
      <p class="empty muted" role="status">{t('settings.search.none', { query: search.query.trim() })}</p>
    {/if}

    <AppearanceCard />
    <PresetsCard />
    <SidebarItemsCard />
    <SizeColourCard />
    <PositionCard />
    <BehaviourCard />
    <NotificationsCard />
    <ShortcutsCard />
    <ProvidersCard />
    <AccountsCard />
    <DataCard />
    <IntegrationsPanel />
    <AdvisorSettings />
    <UpdatesCard />
    <PrivacyCard />
    <BackupCard />
    <AboutCard />

    <!-- on a narrow window the rail is only the search box and the select -->
    <button class="btn reset-all-end" onclick={resetAll}>{t('settings.resetAll')}</button>
  </section>
</div>

<style>
  .settings {
    display: grid;
    grid-template-columns: 12.5rem minmax(0, 1fr);
    align-items: start;
    gap: 1.25rem;
  }

  .rail {
    position: sticky;
    top: 0;
    display: flex;
    flex-direction: column;
    gap: 0.625rem;
    max-height: calc(100vh - 6rem);
    overflow-y: auto;
  }

  .search {
    position: relative;
    display: flex;
  }

  .search .field {
    flex: 1 1 auto;
    min-width: 0;
    padding-right: 1.75rem;
  }

  .clear {
    position: absolute;
    right: 0.125rem;
    top: 50%;
    transform: translateY(-50%);
    padding: 0 0.4375rem;
    border: none;
    background: none;
    color: var(--muted);
  }

  .advanced {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.25rem 0.625rem;
    font-size: 0.8125rem;
  }

  .advanced-text {
    display: flex;
    flex-direction: column;
    gap: 0.0625rem;
    min-width: 0;
  }

  .advanced-hint {
    font-size: 0.6875rem;
    color: var(--muted);
  }

  .index {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.0625rem;
  }

  .index a {
    display: block;
    padding: 0.3125rem 0.625rem;
    border-radius: var(--r-control);
    border-left: 2px solid transparent;
    color: var(--muted);
    text-decoration: none;
    font-size: 0.8125rem;
    transition: color var(--dur-ui) var(--ease-out), background var(--dur-ui) var(--ease-out);
  }

  .index a:hover {
    color: var(--text);
    background: var(--hover);
  }

  .index a.current {
    color: var(--text);
    background: var(--surface-2);
    border-left-color: var(--focus);
    font-weight: 500;
  }

  .jump {
    display: none;
    width: 100%;
  }

  .reset-all {
    align-self: flex-start;
    color: var(--critical);
  }

  .reset-all-end {
    display: none;
    justify-self: start;
    color: var(--critical);
  }

  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 24rem), 1fr));
    align-items: start;
    gap: 1rem;
    min-width: 0;
  }

  .cards > :global(.err),
  .cards > .save-state,
  .cards > .empty {
    grid-column: 1 / -1;
    margin: 0;
  }

  .err {
    color: var(--critical);
    font-size: 0.8125rem;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .empty {
    padding: 1.5rem 0;
    text-align: center;
  }

  /* narrow: the index becomes a select that stays on screen above the cards */
  @media (max-width: 760px) {
    .settings {
      grid-template-columns: minmax(0, 1fr);
      gap: 0.75rem;
    }

    .rail {
      z-index: 1;
      max-height: none;
      overflow: visible;
      padding: 0.25rem 0 0.5rem;
      background: var(--bg);
    }

    .index {
      display: none;
    }

    .jump {
      display: block;
    }

    .reset-all {
      display: none;
    }

    .reset-all-end {
      display: inline-flex;
    }
  }
</style>
