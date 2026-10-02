<!--
  Overview → "Last week": tokens, estimated cost, busiest day and limits hit
  for the last completed Monday–Sunday (the same numbers as the Monday
  notification, from `get_weekly_summary`). [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { getWeeklySummary } from '$lib/api';
  import { formatCost, formatInt, formatTokens } from '$lib/format';
  import { getLocale, t } from '$lib/i18n/i18n.svelte';
  import { accountName } from '$lib/accounts';
  import { providerDisplayName } from '$lib/providers';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import type { WeeklySummary } from '$lib/types';

  let summary = $state<WeeklySummary | null>(null);
  let error = $state<string | null>(null);
  let request = 0;

  async function load() {
    const id = ++request;
    try {
      const next = await getWeeklySummary();
      if (id === request) {
        summary = next;
        error = null;
      }
    } catch (e) {
      if (id === request) error = String(e);
    }
  }

  onMount(() => {
    void load();
    return () => request++;
  });

  /** "Wed, Sep 24" in the UI language; the stored date is a plain local YYYY-MM-DD. */
  function dayLabel(date: string): string {
    const [y, m, d] = date.split('-').map(Number);
    if (!y || !m || !d) return date;
    return new Date(y, m - 1, d).toLocaleDateString(getLocale(), { weekday: 'short', month: 'short', day: 'numeric' });
  }

  const empty = $derived(summary !== null && summary.requests === 0);

  /** "Claude Code · Work" for `claude@work`, plain "Claude Code" for the primary account. */
  function accountTitle(key: string): string {
    const at = key.indexOf('@');
    const provider = providerDisplayName(at > 0 ? key.slice(0, at) : key);
    return at > 0 ? `${provider} · ${accountName(settings.value.accounts, key.slice(at + 1)) ?? key.slice(at + 1)}` : provider;
  }
</script>

{#if error}
  <p class="muted small" role="alert">{t('week.error', { message: error })}</p>
{:else if summary}
  <article class="card week" aria-labelledby="week-title">
    <header>
      <h3 id="week-title">{t('week.title')}</h3>
      <span class="range muted">{t('week.range', { from: dayLabel(summary.weekStart), to: dayLabel(summary.weekEnd) })}</span>
    </header>

    {#if empty}
      <p class="muted none">{t('week.none')}</p>
    {:else}
      <dl>
        <div>
          <dt>{t('week.tokens')}</dt>
          <dd>{formatTokens(summary.totalTokens)}<span class="sub muted">{t('week.requests', { count: formatInt(summary.requests) })}</span></dd>
        </div>
        <div>
          <dt>{t('week.cost')}</dt>
          <dd>{summary.estimatedCostUsd === null ? '—' : `~${formatCost(summary.estimatedCostUsd)}`}</dd>
        </div>
        <div>
          <dt>{t('week.busiest')}</dt>
          <dd>
            {summary.busiestDay ? dayLabel(summary.busiestDay) : '—'}
            {#if summary.busiestDay}<span class="sub muted">{formatTokens(summary.busiestDayTokens)}</span>{/if}
          </dd>
        </div>
        <div>
          <dt>{t('week.limits')}</dt>
          <dd class:hit={summary.limitsHit > 0}>{summary.limitsHit}</dd>
        </div>
      </dl>
      {#if summary.accounts?.length}
        <ul class="accounts" aria-label={t('week.byAccount')}>
          {#each summary.accounts as a (a.key)}
            <li><span>{accountTitle(a.key)}</span><span class="muted">{formatTokens(a.totalTokens)}{a.estimatedCostUsd === null ? '' : ` · ~${formatCost(a.estimatedCostUsd)}`}</span></li>
          {/each}
        </ul>
      {/if}
      <p class="note muted">{t('week.note')}</p>
    {/if}
  </article>
{/if}

<style>
  .week {
    padding: 1rem;
  }

  .accounts {
    list-style: none;
    margin: 0.5rem 0 0;
    padding: 0;
    display: grid;
    gap: 0.25rem;
    font-size: 0.8125rem;
  }

  .accounts li {
    display: flex;
    justify-content: space-between;
    gap: 0.75rem;
  }

  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  h3 {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
  }

  .range,
  .small {
    font-size: 0.75rem;
  }

  dl {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(8rem, 1fr));
    gap: 0.75rem;
    margin: 0.75rem 0 0;
  }

  dt {
    font-size: 0.6875rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }

  dd {
    display: flex;
    flex-direction: column;
    margin: 0.125rem 0 0;
    font-size: 1.125rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  dd.hit {
    color: var(--warn);
  }

  .sub {
    font-size: 0.75rem;
    font-weight: 400;
  }

  .none,
  .note {
    margin: 0.625rem 0 0;
    font-size: 0.75rem;
  }
</style>
