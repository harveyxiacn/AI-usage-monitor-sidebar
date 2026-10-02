<!--
  Shell of one Settings-tab card: title, optional "Reset to defaults" and the
  search scope for the controls inside. The shared control styles (.num, .note,
  .err, …) live here so every card looks alike. [FRONTEND]
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import { t } from '$lib/i18n/i18n.svelte';
  import { isCardModified, resetPatch, type CardId } from '$lib/settings-cards';
  import { matchesQuery } from '$lib/settings-search';
  import { getSettingsSearch, provideCardScope } from '$lib/settings-scope.svelte';
  import { defaultSettings, settings } from '$lib/stores/settings.svelte';

  interface Props {
    /** anchor id, also the key of the card in the search registry */
    id: string;
    title: string;
    /** extra text the search should match (the card's controls add their own) */
    keywords?: string;
    /** enables the "Reset to defaults" button for this card's settings */
    reset?: CardId;
    wide?: boolean;
    children: Snippet;
  }

  let { id, title, keywords = '', reset, wide = false, children }: Props = $props();

  const search = getSettingsSearch();
  const fields = new SvelteMap<symbol, boolean>();
  const headMatches = $derived(!search?.active || matchesQuery(search.query, title, keywords));
  const visible = $derived(headMatches || [...fields.values()].some(Boolean));
  provideCardScope({
    get headMatches() {
      return headMatches;
    },
    fields,
  });
  $effect(() => {
    if (!search) return;
    search.cards.set(id, visible);
    return () => search.cards.delete(id);
  });

  const modified = $derived(reset ? isCardModified(reset, settings.value, defaultSettings) : false);
</script>

<article class="card group settings-card" class:wide {id} hidden={!visible} data-settings-card>
  <header class="head">
    <h3>{title}</h3>
    {#if reset}
      <button
        class="btn reset"
        disabled={!modified}
        aria-label={t('settings.resetCard', { card: title })}
        onclick={() => void settings.patch(resetPatch(reset, defaultSettings))}
      >{t('settings.resetDefaults')}</button>
    {/if}
  </header>
  {@render children()}
</article>

<style>
  .group {
    padding: 0.75rem 1rem 1rem;
    min-width: 0;
    /* keep the heading clear of the sticky search box when jumped to */
    scroll-margin-top: 0.75rem;
  }

  .group[hidden] {
    display: none;
  }

  /* the editors need the full grid width on a two-column layout */
  .group.wide {
    grid-column: 1 / -1;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    margin: 0 0 0.25rem;
  }

  h3 {
    margin: 0;
    font-size: 0.875rem;
    font-weight: 600;
  }

  .reset {
    padding: 0.125rem 0.5rem;
    font-size: 0.75rem;
  }

  /* shared by the controls the cards put in the slot */
  .settings-card :global(input[type='range']) {
    width: 9rem;
    accent-color: var(--focus);
  }

  .settings-card :global(.num) {
    width: 6rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .settings-card :global(.note) {
    margin: 0.375rem 0;
    font-size: 0.6875rem;
    color: var(--faint);
  }

  .settings-card :global(.small) {
    font-size: 0.75rem;
  }

  .settings-card :global(.err) {
    margin: 0;
    color: var(--critical);
    font-size: 0.8125rem;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .settings-card :global(.ok) {
    margin: 0.25rem 0;
    font-size: 0.75rem;
    color: var(--muted);
    overflow-wrap: anywhere;
  }

  .settings-card :global(.danger) {
    color: var(--critical);
  }

  .settings-card :global(.icon) {
    padding: 0.125rem 0.4375rem;
    line-height: 1.2;
  }

  .settings-card :global(.link) {
    padding-inline: 0;
    border: none;
    background: none;
    color: var(--focus);
    text-decoration: underline;
  }

  .settings-card :global(.actions) {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    padding-top: 0.625rem;
    margin-top: 0.25rem;
    border-top: 1px solid var(--border);
  }
</style>
