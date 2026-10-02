<!--
  Overview → "Where to work next": the one routing recommendation of
  `get_routing_advice` (switch to the provider that has room, or "no switch
  needed"), with the numbers it is based on and its confidence. Renders nothing
  when the backend has nothing worth saying. All figures are estimates. [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { isSwitch } from '$lib/advice';
  import { basisRow, confidenceName, routingSentence } from '$lib/advice-text';
  import { t } from '$lib/i18n/i18n.svelte';
  import { advice } from '$lib/stores/advice.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';

  const generatedAt = $derived(snapshot.value?.generatedAt);
  $effect(() => {
    void generatedAt;
    void advice.refresh();
  });
  onMount(() => () => advice.stop());

  const current = $derived(advice.value);
</script>

{#if current}
  <article class="card routing" aria-labelledby="routing-title" data-kind={current.kind}>
    <header>
      <h3 id="routing-title">{t('advisor.routing.title')}</h3>
      <span class="conf muted">{t('advisor.routing.confidence', { level: confidenceName(current.confidence) })}</span>
    </header>
    {#if isSwitch(current)}
      <p class="line" role="status">{routingSentence(current)}</p>
    {:else}
      <p class="line" role="status">{t('advisor.routing.none')}</p>
    {/if}
    {#if current.basis.length > 0}
      <details>
        <summary>{t('advisor.routing.basis')}</summary>
        <ul>
          {#each current.basis as w (w.key)}
            <li>{basisRow(w)}</li>
          {/each}
        </ul>
      </details>
    {/if}
    <p class="note muted">{t('advisor.routing.note')}</p>
  </article>
{/if}

<style>
  .routing {
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  h3,
  p {
    margin: 0;
  }

  h3 {
    font-size: 1rem;
    font-weight: 600;
  }

  .line {
    font-size: 0.875rem;
    line-height: 1.6;
  }

  .conf,
  .note,
  summary,
  li {
    font-size: 0.75rem;
  }

  .muted {
    color: var(--muted);
  }

  summary {
    cursor: pointer;
    color: var(--muted);
  }

  ul {
    margin: 0.375rem 0 0;
    padding-left: 1.125rem;
    line-height: 1.6;
    font-variant-numeric: tabular-nums;
  }

  .note {
    line-height: 1.6;
  }
</style>
