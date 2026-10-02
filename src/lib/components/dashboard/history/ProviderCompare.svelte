<!--
  Per-provider comparison cards: token counters, requests and estimated cost
  for each provider in the current range. [FRONTEND]
-->
<script lang="ts">
  import CostNote from '$lib/components/CostNote.svelte';
  import { formatEstimatedCost, formatInt, formatTokens } from '$lib/format';
  import { t } from '$lib/i18n/i18n.svelte';
  import type { ProviderId, TokenTotals } from '$lib/types';

  interface Props {
    totals: Array<{ id: ProviderId; totals: TokenTotals }>;
    metric: 'tokens' | 'cost';
    costApproximate: boolean;
    providerName: (id: ProviderId) => string;
  }

  let { totals, metric, costApproximate, providerName }: Props = $props();

  const metricValue = (tt: TokenTotals) =>
    metric === 'cost' ? formatEstimatedCost(tt) : formatTokens(tt.totalTokens);
</script>

{#if totals.length > 0}
  <div class="compare">
    {#each totals as p (p.id)}
      <article class="card cmp">
        <header class="cmp-head">
          <span class="cmp-name">{providerName(p.id)}</span>
          <span class="cmp-total">{metricValue(p.totals)}</span>
        </header>
        <dl>
          <div><dt>{t('history.input')}</dt><dd>{formatTokens(p.totals.inputTokens)}</dd></div>
          <div><dt>{t('history.cacheRead')}</dt><dd>{formatTokens(p.totals.cacheReadTokens)}</dd></div>
          <div><dt>{t('history.cacheWrite')}</dt><dd>{formatTokens(p.totals.cacheWriteTokens)}</dd></div>
          <div><dt>{t('history.output')}</dt><dd>{formatTokens(p.totals.outputTokens)}</dd></div>
          <div><dt>{t('history.reasoning')}</dt><dd>{formatTokens(p.totals.reasoningTokens)}</dd></div>
          <div><dt>{t('history.requests')}</dt><dd>{formatInt(p.totals.requests)}</dd></div>
          <div><dt>{t('history.estCost')}</dt><dd>{formatEstimatedCost(p.totals)}</dd></div>
        </dl>
        <p class="note"><CostNote /></p>
        {#if p.totals.estimatedCostUsd == null && p.totals.knownCostUsd != null}
          <p class="note">{t('history.partialCostNote', { count: formatInt(p.totals.unpricedRequests ?? 0) })}</p>
        {:else if p.totals.estimatedCostUsd == null}
          <p class="note">{t('history.costMissingNote')}</p>
        {/if}
        {#if costApproximate}
          <p class="note">{t('history.costApproxNote')}</p>
        {/if}
      </article>
    {/each}
  </div>
{/if}

<style>
  .compare {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 17rem), 1fr));
    gap: 1rem;
  }

  .cmp {
    display: flex;
    flex-direction: column;
    gap: 0.625rem;
    padding: 1rem;
    min-width: 0;
  }

  .cmp-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .cmp-name {
    font-weight: 600;
  }

  .cmp-total {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }

  dl {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 11rem), 1fr));
    gap: 0.25rem 1rem;
    margin: 0;
    font-size: 0.8125rem;
  }

  dl > div {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
    min-width: 0;
  }

  dt {
    color: var(--muted);
    white-space: nowrap;
  }

  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }

  .note {
    margin: 0;
    font-size: 0.6875rem;
    color: var(--faint);
  }
</style>
