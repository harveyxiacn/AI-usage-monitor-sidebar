<!--
  KPI tiles at the top of History → Usage: total tokens, estimated cost,
  requests, active days, cache hit rate and the daily average. Each one shows
  its change against the previous period of equal length (in calendar days)
  and a tiny trend line. [FRONTEND]

  One calendar query covers both periods: `[previous start, range end)`. The
  calendar groups by local day, so a still-running last day is compared with a
  complete one — the footnote says so.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { getUsageCalendar } from '$lib/api';
  import CostNote from '$lib/components/CostNote.svelte';
  import { buildKpis, localDays, previousPeriod, splitPeriods, type Kpi, type KpiId, type KpiSet } from '$lib/analytics';
  import { HEAT_ACCENT } from '$lib/colors';
  import { formatEstimatedCost, formatInt, formatTokens } from '$lib/format';
  import type { HistoryRange } from '$lib/history';
  import { t } from '$lib/i18n/i18n.svelte';
  import type { ProviderId } from '$lib/types';
  import MiniSpark from './MiniSpark.svelte';

  interface Props {
    range: HistoryRange | null;
    provider: ProviderId | '';
    project: string | null;
    /** bumped when a log scan added events */
    dataVersion: number;
    themeKey: string;
  }

  let { range, provider, project, dataVersion, themeKey }: Props = $props();

  let kpis = $state<KpiSet | null>(null);
  let error = $state<string | null>(null);
  let requestId = 0;
  let disposed = false;
  let filterKey = '';

  async function load() {
    const id = ++requestId;
    const active = range;
    if (!active) {
      kpis = null;
      return;
    }
    const before = previousPeriod(active);
    const key = JSON.stringify([active.from, provider, project]);
    // a real filter change clears stale tiles; a live tick updates in place
    if (key !== filterKey) kpis = null;
    filterKey = key;
    error = null;
    try {
      const result = await getUsageCalendar({
        from: new Date(before.from).toISOString(),
        to: new Date(active.to).toISOString(),
        provider: provider === '' ? null : provider,
        project,
      });
      if (id !== requestId || disposed) return;
      const { current, previous } = splitPeriods(result.days, active);
      kpis = buildKpis(current, previous, localDays(active));
    } catch (e) {
      if (id === requestId && !disposed) error = String(e);
    }
  }

  $effect(() => {
    void [range, provider, project, dataVersion];
    requestId++;
    const timer = setTimeout(() => void load(), 80);
    return () => clearTimeout(timer);
  });

  onMount(() => () => {
    disposed = true;
    requestId++;
  });

  const sparkColor = $derived(HEAT_ACCENT[themeKey === 'light' ? 'light' : 'dark']);
  const wholePercent = (value: number) => `${Math.round(value * 100)}%`;

  function valueText(kpi: Kpi, set: KpiSet): string {
    switch (kpi.id) {
      case 'tokens': return formatTokens(kpi.value);
      case 'cost': return formatEstimatedCost(set.totals);
      case 'requests': return formatInt(kpi.value ?? 0);
      case 'activeDays': return `${formatInt(kpi.value ?? 0)} / ${formatInt(set.days)}`;
      case 'cacheHit': return kpi.value == null ? '—' : wholePercent(kpi.value);
      case 'dailyAverage': return formatTokens(kpi.value);
    }
  }

  /** The change as text with a direction glyph, so colour never carries it alone. */
  function deltaText(kpi: Kpi): { text: string; arrow: string; label: string } {
    if (kpi.delta == null) return { text: t('history.kpi.noBaseline'), arrow: '', label: t('history.kpi.noBaselineLabel') };
    const rounded = Math.abs(kpi.delta) < 0.5 ? 0 : Math.round(kpi.delta);
    const arrow = rounded > 0 ? '▲' : rounded < 0 ? '▼' : '■';
    const sign = rounded > 0 ? '+' : rounded < 0 ? '−' : '';
    const text = `${sign}${Math.abs(rounded)}${kpi.deltaUnit === 'points' ? ' pp' : '%'}`;
    return { text, arrow, label: t('history.kpi.deltaLabel', { delta: text }) };
  }

  const TITLES: Record<KpiId, string> = {
    tokens: 'history.totalTokens',
    cost: 'history.estCost',
    requests: 'history.requests',
    activeDays: 'history.kpi.activeDays',
    cacheHit: 'history.kpi.cacheHit',
    dailyAverage: 'history.kpi.dailyAverage',
  };
</script>

<section class="kpis" aria-label={t('history.kpi.title')}>
  {#if error}
    <p class="err" role="alert">{t('common.error', { message: error })}</p>
  {:else}
    <ul>
      {#if kpis}
        {#each kpis.items as kpi (kpi.id)}
          {@const delta = deltaText(kpi)}
          <li class="tile card">
            <span class="name">{t(TITLES[kpi.id] as 'history.totalTokens')}</span>
            <span class="value">{valueText(kpi, kpis)}</span>
            <span class="delta" class:none={kpi.delta == null} aria-label={delta.label}>
              {#if delta.arrow}<span class="arrow" aria-hidden="true">{delta.arrow}</span>{/if}{delta.text}
            </span>
            <MiniSpark values={kpi.spark} color={sparkColor} />
          </li>
        {/each}
      {:else}
        <li class="tile card placeholder" aria-busy="true"><span class="muted">{t('common.loading')}</span></li>
      {/if}
    </ul>
    <p class="note">{t('history.kpi.note')} <CostNote /></p>
  {/if}
</section>

<style>
  .kpis {
    display: flex;
    flex-direction: column;
    gap: 0.375rem;
    min-width: 0;
  }

  ul {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 9.5rem), 1fr));
    gap: 0.75rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    padding: 0.75rem 0.875rem 0.625rem;
    min-width: 0;
  }

  .name {
    font-size: 0.6875rem;
    color: var(--muted);
  }

  .value {
    font-size: 1.375rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.01em;
    overflow-wrap: anywhere;
  }

  .delta {
    font-size: 0.75rem;
    color: var(--text);
    font-variant-numeric: tabular-nums;
    margin-bottom: 0.25rem;
  }

  .delta.none {
    color: var(--faint);
  }

  .arrow {
    font-size: 0.625rem;
    margin-right: 0.25rem;
  }

  .placeholder {
    grid-column: 1 / -1;
  }

  .note {
    margin: 0;
    font-size: 0.6875rem;
    color: var(--faint);
  }

  .err {
    margin: 0;
    color: var(--critical);
    font-size: 0.8125rem;
  }
</style>
