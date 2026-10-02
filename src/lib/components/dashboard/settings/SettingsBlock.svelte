<!--
  A free-form block inside a settings card (a list, a table, a button row) that
  takes part in the Settings search like a `Field` does: `text` is what the
  search looks at, in the current language. Children stay mounted while hidden,
  so an editor keeps its state. [FRONTEND]
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { matchesQuery } from '$lib/settings-search';
  import { controlShown, getCardScope, getSettingsSearch } from '$lib/settings-scope.svelte';

  interface Props {
    /** searchable text of the block (titles, labels, hints) */
    text: string;
    /** hidden until "Show advanced settings" is on or a search matches it */
    advanced?: boolean;
    children: Snippet;
  }

  let { text, advanced = false, children }: Props = $props();

  const search = getSettingsSearch();
  const scope = getCardScope();
  const matches = $derived(!search?.active || !!scope?.headMatches || matchesQuery(search.query, text));
  const shown = $derived(controlShown(search, advanced || !!scope?.advanced, matches));
  const id = Symbol('block');
  $effect(() => {
    if (!scope) return;
    scope.fields.set(id, shown);
    return () => scope.fields.delete(id);
  });
</script>

<div class="block" hidden={!shown}>{@render children()}</div>

<style>
  .block[hidden] {
    display: none;
  }
</style>
