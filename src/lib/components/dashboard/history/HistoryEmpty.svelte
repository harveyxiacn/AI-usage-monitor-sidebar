<!--
  Why History is empty, and what to do about it. [FRONTEND]
  The cause is picked by the parent from what it knows: ingestion switched
  off, no session logs found by the last scan, filters that match nothing, or
  simply a quiet range.
-->
<script lang="ts" module>
  export type EmptyKind = 'ingestDisabled' | 'noLogs' | 'filtered' | 'range';
</script>

<script lang="ts">
  import { t } from '$lib/i18n/i18n.svelte';

  interface Props {
    kind: EmptyKind;
    rescanning: boolean;
    onrescan: () => void;
    onclear: () => void;
  }

  let { kind, rescanning, onrescan, onclear }: Props = $props();
</script>

<div class="empty card" role="status" data-kind={kind}>
  <h3>{t(`history.empty.${kind}.title` as 'history.empty.range.title')}</h3>
  <p class="muted">{t(`history.empty.${kind}.hint` as 'history.empty.range.hint')}</p>
  <div class="actions">
    {#if kind === 'filtered'}
      <button class="btn" onclick={onclear}>{t('history.filters.clear')}</button>
    {/if}
    {#if kind !== 'ingestDisabled'}
      <button class="btn" onclick={onrescan} disabled={rescanning}>
        {rescanning ? t('history.ingestRunning') : t('history.rescan')}
      </button>
    {/if}
  </div>
</div>

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 1.25rem 1.5rem;
  }

  h3 {
    margin: 0;
    font-size: 0.9375rem;
    font-weight: 600;
  }

  p {
    margin: 0;
    font-size: 0.8125rem;
    max-width: 44rem;
  }

  .actions {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
</style>
