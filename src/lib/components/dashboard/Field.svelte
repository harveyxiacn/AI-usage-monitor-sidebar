<!--
  One settings row: label (+ hint) on the left, control on the right. [FRONTEND]
  Inside the Settings tab the row also hides itself when it does not match the
  search box (label and hint, in the current language) and, when `advanced`,
  until "Show advanced settings" is on or a search matches it (then it carries
  an "advanced" badge).
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { t } from '$lib/i18n/i18n.svelte';
  import { matchesQuery } from '$lib/settings-search';
  import { controlShown, getCardScope, getSettingsSearch } from '$lib/settings-scope.svelte';

  interface Props {
    label: string;
    hint?: string;
    /** stretch the control to the full row width (sliders, editors) */
    wide?: boolean;
    /** an advanced-tier control (see `settings-tiers.ts`); a card made only of those marks all of its rows */
    advanced?: boolean;
    children: Snippet;
  }

  let { label, hint, wide = false, advanced = false, children }: Props = $props();

  const search = getSettingsSearch();
  const scope = getCardScope();
  const isAdvanced = $derived(advanced || !!scope?.advanced);
  const matches = $derived(!search?.active || !!scope?.headMatches || matchesQuery(search.query, label, hint));
  const shown = $derived(controlShown(search, isAdvanced, matches));
  /** revealed by the search only, so the user can tell it is normally tucked away */
  const badge = $derived(isAdvanced && !!search && !search.showAdvanced);
  const id = Symbol('field');
  $effect(() => {
    if (!scope) return;
    scope.fields.set(id, shown);
    return () => scope.fields.delete(id);
  });
</script>

<div class="setting-field" class:wide hidden={!shown}>
  <div class="text">
    <span class="label">{label}{#if badge} <span class="adv-badge">{t('settings.advanced.badge')}</span>{/if}</span>
    {#if hint}<span class="hint">{hint}</span>{/if}
  </div>
  <div class="control">{@render children()}</div>
</div>

<style>
  .setting-field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--border);
  }

  .setting-field[hidden] {
    display: none;
  }

  .setting-field:last-child {
    border-bottom: none;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    min-width: 0;
  }

  .label {
    color: var(--text);
  }

  .adv-badge {
    display: inline-block;
    margin-left: 0.25rem;
    padding: 0 0.375rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 0.625rem;
    line-height: 1.4;
    color: var(--muted);
    vertical-align: 0.0625rem;
  }

  .hint {
    font-size: 0.75rem;
    color: var(--muted);
  }

  .control {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex: none;
    min-width: 0;
    max-width: 100%;
    flex-wrap: wrap;
  }

  .control :global(select) { max-width: 100%; }

  .wide {
    flex-direction: column;
    align-items: stretch;
  }

  .wide .control {
    flex: 1 1 auto;
  }

  @media (max-width: 620px) {
    .setting-field {
      flex-direction: column;
      align-items: stretch;
    }
  }
</style>
