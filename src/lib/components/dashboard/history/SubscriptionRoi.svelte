<!--
  Subscription value: this month's API-equivalent estimate compared with the
  subscription price the user entered in Settings (0 = unknown). Always an
  estimate, never billing. When no monthly budget is set (so HistoryTab draws
  no BudgetChart) it draws its own burn-up with the subscription line. [FRONTEND]
-->
<script lang="ts">
  import { onDestroy } from 'svelte';
  import { getUsageCalendar, getUsageHistory } from '$lib/api';
  import BudgetChart from '$lib/components/BudgetChart.svelte';
  import { formatCost } from '$lib/format';
  import { budgetProgress } from '$lib/history';
  import { t } from '$lib/i18n/i18n.svelte';
  import { providerDisplayName } from '$lib/providers';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import { subscriptionRoi, subscriptionTotal } from '$lib/subscription';
  import type { CalendarDay, ProviderId, TokenTotals } from '$lib/types';

  interface Props {
    /** history provider filter; null = every provider */
    provider: ProviderId | null;
    themeKey: string;
    /** bumps when new usage was ingested */
    refreshKey: number;
  }
  let { provider, themeKey, refreshKey }: Props = $props();

  let byProvider = $state<Record<string, TokenTotals>>({});
  let days = $state<CalendarDay[]>([]);
  let error = $state<string | null>(null);
  let requestId = 0;
  let disposed = false;

  const prices = $derived(settings.value.subscriptionUsd);
  const monthlyBudget = $derived(settings.value.monthlyBudgetUsd);
  const total = $derived(subscriptionTotal(prices, provider));
  const rows = $derived.by(() => {
    const ids = (snapshot.value?.providers ?? []).map((p) => p.provider).filter((id) => !provider || id === provider);
    return ids.map((id) => ({ id, price: prices[id] ?? 0, spent: byProvider[id]?.estimatedCostUsd ?? null, known: byProvider[id]?.knownCostUsd ?? null }));
  });
  /** the same month for the chart and the ROI text (null before the calendar arrives) */
  const progress = $derived(days.length > 0 && total > 0 ? budgetProgress(days, total) : null);
  const elapsed = $derived(progress?.elapsed ?? monthElapsed(Date.now()));

  function monthElapsed(now: number): number {
    const start = new Date(now); start.setHours(0, 0, 0, 0); start.setDate(1);
    const end = new Date(start); end.setMonth(end.getMonth() + 1);
    return (now - start.getTime()) / (end.getTime() - start.getTime());
  }

  async function load() {
    const id = ++requestId;
    const now = Date.now();
    const start = new Date(now); start.setHours(0, 0, 0, 0); start.setDate(1);
    const range = { from: start.toISOString(), to: new Date(now + 1000).toISOString() };
    try {
      const [history, calendar] = await Promise.all([
        getUsageHistory({ ...range, bucket: 'month', groupByModel: false, provider: null }),
        getUsageCalendar({ ...range, provider }),
      ]);
      if (disposed || id !== requestId) return;
      byProvider = history.byProvider;
      days = calendar.days;
      error = null;
    } catch (e) {
      if (!disposed && id === requestId) error = String(e);
    }
  }

  $effect(() => {
    void [provider, refreshKey];
    const timer = setTimeout(() => void load(), 100);
    return () => clearTimeout(timer);
  });
  onDestroy(() => { disposed = true; requestId++; });
</script>

{#if rows.length > 0}
  <div class="card roi" aria-label={t('history.roi.title')}>
    <header><h3>{t('history.roi.title')}</h3></header>
    {#each rows as row (row.id)}
      {@const name = providerDisplayName(row.id)}
      {@const roi = row.price > 0 && row.spent !== null ? subscriptionRoi(row.spent, row.price, elapsed) : null}
      <p class="line" role="status">
        {#if row.price <= 0}
          <span class="muted">{t('history.roi.unset', { provider: name })}</span>
        {:else if roi}
          {t('history.roi.month', { provider: name, spent: formatCost(roi.spentUsd), multiple: roi.multiple.toFixed(1), price: formatCost(row.price) })}
          {#if roi.projectedUsd !== null && roi.projectedMultiple !== null}
            <span class="muted">{t('history.roi.projected', { projected: formatCost(roi.projectedUsd), multiple: roi.projectedMultiple.toFixed(1) })}</span>
          {/if}
        {:else if row.known !== null}
          <span class="muted">{t('history.roi.incomplete')} ({name}: {formatCost(row.known)})</span>
        {:else}
          <span class="muted">{name}: {t('common.loading')}</span>
        {/if}
      </p>
    {/each}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if monthlyBudget <= 0 && total > 0 && progress}
      <BudgetChart series={progress.series} budgetUsd={0} subscriptionUsd={total} {themeKey} />
    {/if}
    <p class="muted note">{t('history.roi.note')}</p>
  </div>
{/if}

<style>
  .roi { display: flex; flex-direction: column; gap: 0.625rem; padding: 1rem; min-width: 0; }
  h3, p { margin: 0; }
  h3 { font-size: 0.9375rem; }
  .line { font-size: 0.8125rem; line-height: 1.6; display: flex; flex-direction: column; }
  .muted { color: var(--muted); font-size: 0.75rem; }
  .note { line-height: 1.6; }
  .error { color: var(--danger, #ff6b6b); font-size: 0.8125rem; }
</style>
