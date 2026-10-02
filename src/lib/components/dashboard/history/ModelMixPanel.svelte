<!--
  Model mix over time (100 % stacked area, one layer per model, the long tail
  folded into "Other") next to the overall reasoning-effort share. [FRONTEND]

  The mix needs rows grouped by model but not by project. When the main query
  already is that, its rows are reused (`mainRows`); otherwise the panel asks
  for them itself with the same range, bucket and filters.
-->
<script lang="ts">
  import {
    CategoryScale,
    Chart,
    Filler,
    Legend,
    LinearScale,
    LineController,
    LineElement,
    PointElement,
    Tooltip,
    type ChartConfiguration,
  } from 'chart.js';
  import { onDestroy, onMount } from 'svelte';
  import { getUsageHistory } from '$lib/api';
  import { buildMix, OTHER_KEY, sharePercent } from '$lib/analytics';
  import { cssVar, HEAT_ACCENT, resolveColor, seriesColor } from '$lib/colors';
  import { formatBucket, formatTokens } from '$lib/format';
  import type { HistoryRange } from '$lib/history';
  import { t } from '$lib/i18n/i18n.svelte';
  import type { Bucket, HistoryRow, ProviderId } from '$lib/types';
  import ChartDataTable from './ChartDataTable.svelte';

  Chart.register(LineController, LineElement, PointElement, CategoryScale, LinearScale, Filler, Tooltip, Legend);

  interface Props {
    range: HistoryRange | null;
    bucket: Bucket;
    provider: ProviderId | '';
    project: string | null;
    /** null = every account, '' = the primary account, else an extra account id */
    account?: string | null;
    /** rows of the page's own query when they already are per model (not per project) */
    mainRows: HistoryRow[] | null;
    dataVersion: number;
    themeKey: string;
    height?: number;
  }

  let { range, bucket, provider, project, account = null, mainRows, dataVersion, themeKey, height = 240 }: Props = $props();

  const MAX_MODELS = 6;

  let fetched = $state<HistoryRow[] | null>(null);
  let error = $state<string | null>(null);
  let requestId = 0;
  let disposed = false;
  let filterKey = '';
  let canvas = $state<HTMLCanvasElement | null>(null);
  let chart: Chart<'line', number[], string> | null = null;

  const rows = $derived(mainRows ?? fetched ?? []);
  const needsFetch = $derived(mainRows === null);

  async function load() {
    const id = ++requestId;
    const active = range;
    if (!active || !needsFetch) return;
    const key = JSON.stringify([active.from, bucket, provider, project, account]);
    if (key !== filterKey) fetched = null;
    filterKey = key;
    error = null;
    try {
      const result = await getUsageHistory({
        from: new Date(active.from).toISOString(),
        to: new Date(active.to).toISOString(),
        bucket,
        groupByModel: true,
        groupByProject: false,
        project,
        provider: provider === '' ? null : provider,
        account,
      });
      if (id === requestId && !disposed) fetched = result.rows;
    } catch (e) {
      if (id === requestId && !disposed) error = String(e);
    }
  }

  $effect(() => {
    void [range, bucket, provider, project, account, dataVersion, needsFetch];
    requestId++;
    if (!needsFetch) return;
    const timer = setTimeout(() => void load(), 80);
    return () => clearTimeout(timer);
  });

  onMount(() => () => {
    disposed = true;
    requestId++;
  });

  const mix = $derived(buildMix(rows, MAX_MODELS, (count) => t('chart.other', { count }), t('history.modelUnknown')));
  const percent = (v: number) => `${v < 10 && v > 0 ? v.toFixed(1) : Math.round(v)}%`;
  const effortLabel = (effort: string) => effort || t('history.effortUnrecorded');
  const bucketTotals = $derived(mix.buckets.map((_, i) => mix.series.reduce((sum, s) => sum + s.values[i], 0)));

  const tableColumns = $derived([t('history.table.bucket'), ...mix.series.map((s) => s.label)]);
  const tableRows = $derived(mix.buckets.map((b, i) => [
    formatBucket(b, bucket),
    ...mix.series.map((s) => `${percent(sharePercent(s.values[i], bucketTotals[i]))} · ${formatTokens(s.values[i])}`),
  ]));

  function render() {
    if (!canvas || mix.series.length === 0 || mix.buckets.length === 0) { chart?.destroy(); chart = null; return; }
    const text = cssVar('--muted', '#9a9aa3');
    const grid = cssVar('--grid', 'rgba(255,255,255,0.07)');
    const surface = cssVar('--surface-2', '#232329');
    const strong = cssVar('--text', '#f5f5f7');

    const configuration: ChartConfiguration<'line', number[], string> = {
      type: 'line',
      data: {
        labels: mix.buckets.map((b) => formatBucket(b, bucket)),
        datasets: mix.series.map((s, index) => {
          const color = s.key === OTHER_KEY ? resolveColor(cssVar('--faint', '#6f6f78')) : resolveColor(seriesColor(index));
          return {
            label: s.label,
            data: s.values.map((v, i) => sharePercent(v, bucketTotals[i])),
            borderColor: color,
            backgroundColor: color,
            borderWidth: 1.5,
            pointRadius: mix.buckets.length > 30 ? 0 : 2,
            pointHoverRadius: 4,
            fill: true,
            tension: 0.15,
          };
        }),
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        animation: false,
        interaction: { mode: 'index', intersect: false },
        scales: {
          x: {
            grid: { display: false },
            border: { color: grid },
            ticks: { color: text, maxRotation: 0, autoSkipPadding: 16, font: { size: 11 } },
          },
          y: {
            stacked: true,
            min: 0,
            max: 100,
            grid: { color: grid },
            border: { display: false },
            ticks: { color: text, font: { size: 11 }, callback: (v) => `${v}%` },
          },
        },
        plugins: {
          legend: {
            position: 'bottom',
            labels: { color: text, boxWidth: 10, boxHeight: 10, usePointStyle: true, pointStyle: 'rect', font: { size: 11 } },
          },
          tooltip: {
            backgroundColor: surface,
            titleColor: strong,
            bodyColor: strong,
            borderColor: grid,
            borderWidth: 1,
            padding: 10,
            callbacks: {
              label: (ctx) => ` ${ctx.dataset.label}: ${percent(ctx.parsed.y ?? 0)} · ${formatTokens(mix.series[ctx.datasetIndex]?.values[ctx.dataIndex] ?? 0)}`,
              footer: (items) => `${t('history.totalShort')}: ${formatTokens(bucketTotals[items[0]?.dataIndex ?? 0] ?? 0)}`,
            },
          },
        },
      },
    };
    if (chart?.canvas === canvas) {
      chart.data = configuration.data;
      chart.options = configuration.options ?? {};
      chart.update('none');
    } else {
      chart?.destroy();
      chart = new Chart(canvas, configuration);
    }
  }

  $effect(() => {
    void [mix, bucket, themeKey, canvas];
    render();
  });

  onDestroy(() => {
    chart?.destroy();
    chart = null;
  });

  const barColor = $derived(HEAT_ACCENT[themeKey === 'light' ? 'light' : 'dark']);
</script>

{#if error}
  <p class="err" role="alert">{t('common.error', { message: error })}</p>
{:else if mix.total === 0}
  <p class="muted">{t('chart.noData')}</p>
{:else}
  <div class="layout">
    <div class="area">
      <div class="chart" style:height={`${height / 16}rem`}>
        <canvas bind:this={canvas} aria-label={t('history.mix.accessible')}></canvas>
      </div>
      <ChartDataTable caption={t('history.mix.title')} columns={tableColumns} rows={tableRows} />
    </div>

    <section class="efforts" aria-label={t('history.effort.title')}>
      <h4>{t('history.effort.title')}</h4>
      <ul>
        {#each mix.efforts as e (e.effort)}
          <li>
            <span class="effort-name" title={effortLabel(e.effort)}>{effortLabel(e.effort)}</span>
            <span class="track" aria-hidden="true"><span class="fill" style:width={`${Math.max(e.share, 1)}%`} style:background={barColor}></span></span>
            <span class="effort-value">{percent(e.share)} · {formatTokens(e.tokens)}</span>
          </li>
        {/each}
      </ul>
      <p class="note">{t('history.effort.note')}</p>
    </section>
  </div>
{/if}

<style>
  .layout {
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(12rem, 1fr);
    gap: 1.25rem;
    align-items: start;
  }

  @media (max-width: 1000px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  .area {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-width: 0;
  }

  .chart {
    position: relative;
    width: 100%;
    min-width: 0;
  }

  canvas {
    display: block;
  }

  h4 {
    margin: 0 0 0.5rem;
    font-size: 0.8125rem;
    font-weight: 600;
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    grid-template-areas: 'name value' 'track track';
    gap: 0.125rem 0.5rem;
    font-size: 0.75rem;
  }

  .effort-name {
    grid-area: name;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .effort-value {
    grid-area: value;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .track {
    grid-area: track;
    height: 0.5rem;
    border-radius: 0.25rem;
    background: var(--surface-2);
    overflow: hidden;
  }

  .fill {
    display: block;
    height: 100%;
    border-radius: 0.25rem;
  }

  .note {
    margin: 0.5rem 0 0;
    font-size: 0.6875rem;
    color: var(--faint);
  }

  .muted {
    margin: 0;
  }

  .err {
    margin: 0;
    color: var(--critical);
    font-size: 0.8125rem;
  }
</style>
