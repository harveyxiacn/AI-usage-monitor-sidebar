<!--
  Text alternative for a canvas chart: the same numbers in a collapsed table.
  [FRONTEND]
-->
<script lang="ts">
  import { t } from '$lib/i18n/i18n.svelte';

  interface Props {
    caption: string;
    columns: string[];
    rows: string[][];
  }

  let { caption, columns, rows }: Props = $props();
</script>

{#if rows.length > 0}
  <details class="data">
    <summary>{t('chart.showData')}</summary>
    <div class="wrap">
      <table>
        <caption class="sr-only">{caption}</caption>
        <thead>
          <tr>{#each columns as column, i (i)}<th scope="col" class:num={i > 0}>{column}</th>{/each}</tr>
        </thead>
        <tbody>
          {#each rows as row, r (r)}
            <tr>{#each row as cell, i (i)}{#if i === 0}<th scope="row">{cell}</th>{:else}<td class="num mono">{cell}</td>{/if}{/each}</tr>
          {/each}
        </tbody>
      </table>
    </div>
  </details>
{/if}

<style>
  .data {
    font-size: 0.75rem;
  }

  summary {
    cursor: pointer;
    color: var(--muted);
    width: fit-content;
  }

  summary:hover {
    color: var(--text);
  }

  .wrap {
    max-height: 14rem;
    overflow: auto;
    margin-top: 0.375rem;
  }

  table {
    border-collapse: collapse;
    white-space: nowrap;
  }

  th,
  td {
    padding: 0.1875rem 0.5rem;
    border-bottom: 1px solid var(--border);
    text-align: left;
    font-weight: 400;
  }

  thead th {
    position: sticky;
    top: 0;
    background: var(--surface);
    color: var(--muted);
  }

  .num {
    text-align: right;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
