<!--
  Log-ingestion status and the manual rescan. [FRONTEND]
-->
<script lang="ts">
  import { t } from '$lib/i18n/i18n.svelte';
  import type { IngestStats } from '$lib/types';

  interface Props {
    ingest: IngestStats | null;
    rescanning: boolean;
    onrescan: () => void;
  }

  let { ingest, rescanning, onrescan }: Props = $props();
</script>

<div class="card panel">
  <header class="panel-head">
    <h3>{t('history.ingestion')}</h3>
    <button class="btn" onclick={onrescan} disabled={rescanning || ingest?.running}>
      {rescanning || ingest?.running ? t('history.ingestRunning') : t('history.rescan')}
    </button>
  </header>
  <p class="muted">
    {#if ingest}
      {ingest.running ? t('history.ingestRunning') : ''}
      {t('history.ingestStats', {
        files: ingest.filesScanned,
        updated: ingest.filesUpdated,
        events: ingest.eventsAdded,
        ms: ingest.durationMs,
      })}
      {#if ingest.errors.length > 0}
        · <span class="err-inline">{t('history.ingestErrors', { n: ingest.errors.length })}</span>
      {/if}
    {:else}
      {t('history.ingestIdle')}
    {/if}
  </p>
  {#if ingest && ingest.errors.length > 0}
    <details class="scan-errors">
      <summary>{t('history.ingestErrors', { n: ingest.errors.length })}</summary>
      <ul>{#each ingest.errors as message, i (i)}<li>{message}</li>{/each}</ul>
    </details>
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    padding: 1rem;
    min-width: 0;
  }

  .panel-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 1rem;
    flex-wrap: wrap;
  }

  h3 {
    margin: 0;
    font-size: 0.875rem;
    font-weight: 600;
  }

  p {
    margin: 0;
    font-size: 0.75rem;
  }

  .err-inline {
    color: var(--warn);
  }

  .scan-errors {
    overflow-wrap: anywhere;
  }
</style>
