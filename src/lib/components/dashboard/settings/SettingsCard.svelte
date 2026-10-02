<!--
  Shell of one Settings-tab card: title, optional "Reset to defaults" and the
  search scope for the controls inside. The shared control styles (.num, .note,
  .err, …) live here so every card looks alike. [FRONTEND]
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import { t } from '$lib/i18n/i18n.svelte';
  import { cardAdvancedOnly, isCardModified, resetPatch, type CardId } from '$lib/settings-cards';
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
  const modified = $derived(reset ? isCardModified(reset, settings.value, defaultSettings) : false);
  const fields = new SvelteMap<symbol, boolean>();
  const headMatches = $derived(!search?.active || matchesQuery(search.query, title, keywords));
  // a card made only of advanced settings has none of its rows showing until
  // advanced is on, so it drops out of the page and the index by itself; the
  // `size === 0` case is the first paint, before the rows have registered
  const anyShown = $derived([...fields.values()].some(Boolean));
  const visible = $derived(search?.active ? headMatches || anyShown : fields.size === 0 || anyShown);
  provideCardScope({
    get headMatches() {
      return headMatches;
    },
    get advanced() {
      // a card of advanced settings stays on the page once the user has changed
      // something in it (a configured extra account, a custom colour, a shortcut)
      return reset ? cardAdvancedOnly(reset) && !modified : false;
    },
    fields,
  });
  $effect(() => {
    if (!search) return;
    search.cards.set(id, visible);
    return () => search.cards.delete(id);
  });

  /** Resets every key of the card, the advanced ones hidden right now included, so it says so. */
  function resetCard() {
    if (!reset) return;
    if (!window.confirm(t('settings.resetCard.confirm', { card: title }))) return;
    void settings.patch(resetPatch(reset, defaultSettings));
  }

</script>

<article class="card group settings-card" class:wide {id} hidden={!visible} data-settings-card>
  <header class="head">
    <h3>{title}</h3>
    {#if reset}
      <button
        class="btn reset"
        disabled={!modified}
        aria-label={t('settings.resetCard', { card: title })}
        onclick={resetCard}
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
