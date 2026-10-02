<!--
  Grouped or stacked bar chart of token usage / estimated cost. [FRONTEND]
  chart.js v4, tree-shaken registration (bar controller + the two scales + the
  tooltip/legend plugins only).

  Reuse the canvas instance across data, layout and palette updates. Chart.js
  receives concrete colours resolved from the current CSS custom properties.
-->
<script lang="ts">
  import {
    BarController,
    BarElement,
    CategoryScale,
    Chart,
    LinearScale,
    Legend,
    Tooltip,
    type ChartDataset,
    type ChartConfiguration,
  } from 'chart.js';
  import { onDestroy } from 'svelte';
  import { cssVar, lighten, providerAccent, resolveColor } from '$lib/colors';
  import { foldSeries, OTHER_KEY, sharePercent } from '$lib/analytics';
  import { formatBucket, formatCost, formatInt, formatTokens } from '$lib/format';
  import { historySeries, modelVariantLabel, projectLabels, shortenLabel } from '$lib/history';
  import { t } from '$lib/i18n/i18n.svelte';
  import { providerDisplayName } from '$lib/providers';
  import type { Bucket, HistoryRow } from '$lib/types';

  Chart.register(BarController, BarElement, CategoryScale, LinearScale, Tooltip, Legend);

  interface Props {
    rows: HistoryRow[];
    bucket: Bucket;
    groupByModel: boolean;
    groupByProject?: boolean;
    /** path → label, so the legend matches the table; falls back to our own */
    projectNames?: ReadonlyMap<string, string>;
    metric: 'tokens' | 'cost';
    layout: 'grouped' | 'stacked';
    /** changes whenever the palette changes, refreshing concrete colours */
    themeKey: string;
    height?: number;
    /** series beyond this many are folded into one "Other" series */
    maxSeries?: number;
    /** a bar (bucket) was clicked: RFC 3339 start of that bucket */
    onpickbucket?: (bucketStart: string) => void;
  }

  let { rows, bucket, groupByModel, groupByProject = false, projectNames, metric, layout, themeKey, height = 260, maxSeries = 8, onpickbucket }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let chart: Chart<'bar', (number | null)[], string> | null = null;

  const missingPrices = $derived(metric === 'cost' && rows.some((row) => row.estimatedCostUsd == null));
  const unpricedRequests = $derived(metric === 'cost' ? rows.reduce((sum, row) => sum + (row.unpricedRequests ?? 0), 0) : 0);
  const fmt = (v: number) => (metric === 'cost' ? formatCost(v) : formatTokens(v));
  const rawSeries = $derived(historySeries(rows, groupByModel, groupByProject, metric));
  const folded = $derived(foldSeries(rawSeries.series, maxSeries, (values, count) => ({
    key: OTHER_KEY, provider: '' as string, model: null, reasoningEffort: null, project: null, values, count,
  } as (typeof rawSeries.series)[number] & { count: number })));
  const chartSeries = $derived({ buckets: rawSeries.buckets, series: folded.series as Array<(typeof rawSeries.series)[number] & { count?: number }> });
  const plotWidth = $derived.by(() => {
    if (layout === 'stacked') return 0;
    const { buckets, series } = chartSeries;
    return buckets.length * Math.max(64, series.length * 20 + 28) + 64;
  });

  interface Built {
    labels: string[];
    datasets: ChartDataset<'bar', (number | null)[]>[];
  }

  function build(): Built {
    const { buckets, series } = chartSeries;
    const unassigned = t('history.project.unassigned');
    const own = projectLabels(series.map((entry) => entry.project ?? ''), unassigned);
    const projects = projectNames ?? own;
    const providerIndex: Record<string, number> = {};
    return {
      labels: buckets.map((b) => formatBucket(b, bucket)),
      datasets: series.map((entry) => {
        if (entry.key === OTHER_KEY) {
          return {
            label: t('chart.other', { count: entry.count ?? folded.folded }),
            data: entry.values,
            backgroundColor: resolveColor(cssVar('--faint', '#6f6f78')),
            borderWidth: 0,
            borderRadius: 3,
            borderSkipped: false,
            maxBarThickness: 44,
          } satisfies ChartDataset<'bar', (number | null)[]>;
        }
        const path = entry.project ?? '';
        const label = [providerDisplayName(entry.provider)];
        if (groupByModel) label.push(modelVariantLabel(entry.model, entry.reasoningEffort, t('history.modelUnknown'), t('history.effortUnknown')));
        // a legend entry is a single line: long paths are elided in the middle
        if (groupByProject) label.push(shortenLabel(projects.get(path) ?? own.get(path) ?? unassigned));
        const base = resolveColor(providerAccent(entry.provider));
        providerIndex[entry.provider] ??= 0;
        const color = groupByModel || groupByProject ? lighten(base, (providerIndex[entry.provider]++ % 5) * 0.09, 0.75) : base;
        return {
          label: label.join(' · '),
          data: entry.values,
          backgroundColor: resolveColor(color),
          borderWidth: 0,
          borderRadius: 3,
          borderSkipped: false,
          maxBarThickness: 44,
        } satisfies ChartDataset<'bar', (number | null)[]>;
      }),
    };
  }

  /** Sum over every series of one bucket; null when a cost in it is unknown. */
  function bucketTotal(index: number): number | null {
    let sum = 0;
    for (const entry of chartSeries.series) {
      const value = entry.values[index];
      if (value == null) return null;
      sum += value;
    }
    return sum;
  }

  const requestsByBucket = $derived.by(() => {
    const map = new Map<string, number>();
    for (const row of rows) map.set(row.bucketStart, (map.get(row.bucketStart) ?? 0) + row.requests);
    return map;
  });
  const bucketRequests = (index: number) => requestsByBucket.get(chartSeries.buckets[index]) ?? 0;

  function render() {
    if (!canvas || rows.length === 0) { chart?.destroy(); chart = null; return; }

    const text = cssVar('--muted', '#9a9aa3');
    const grid = cssVar('--grid', 'rgba(255,255,255,0.07)');
    const surface = cssVar('--surface-2', '#232329');
    const strong = cssVar('--text', '#f5f5f7');
    const { labels, datasets } = build();

    const configuration: ChartConfiguration<'bar', (number | null)[], string> = {
      type: 'bar',
      data: { labels, datasets },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        // Local log updates must not replay the bar entrance animation.
        animation: false,
        interaction: { mode: 'index', intersect: false },
        onClick: (_event, elements) => {
          const index = elements[0]?.index;
          const start = index == null ? undefined : chartSeries.buckets[index];
          if (start !== undefined && onpickbucket) onpickbucket(start);
        },
        onHover: (event, elements) => {
          const target = event.native?.target;
          if (target instanceof HTMLElement) target.style.cursor = onpickbucket && elements.length > 0 && bucket !== 'hour' ? 'pointer' : 'default';
        },
        scales: {
          x: {
            stacked: layout === 'stacked',
            grid: { display: false },
            border: { color: grid },
            ticks: { color: text, maxRotation: 0, autoSkipPadding: 16, font: { size: 11 } },
          },
          y: {
            stacked: layout === 'stacked',
            beginAtZero: true,
            grid: { color: grid },
            border: { display: false },
            ticks: { color: text, font: { size: 11 }, callback: (v) => fmt(Number(v)) },
          },
        },
        plugins: {
          legend: {
            position: 'bottom',
            labels: {
              color: text,
              boxWidth: 10,
              boxHeight: 10,
              usePointStyle: true,
              pointStyle: 'circle',
              font: { size: 11 },
            },
          },
          tooltip: {
            backgroundColor: surface,
            titleColor: strong,
            bodyColor: strong,
            borderColor: grid,
            borderWidth: 1,
            padding: 10,
            callbacks: {
              label: (ctx) => {
                const total = bucketTotal(ctx.dataIndex);
                const share = ctx.parsed.y == null || total == null ? '' : ` (${sharePercent(ctx.parsed.y, total).toFixed(ctx.parsed.y > 0 && sharePercent(ctx.parsed.y, total) < 10 ? 1 : 0)}%)`;
                return ` ${ctx.dataset.label}: ${ctx.parsed.y == null ? '—' : fmt(ctx.parsed.y)}${share}`;
              },
              footer: (items) => {
                if (items.length === 0) return '';
                const total = bucketTotal(items[0].dataIndex);
                const lines = [`${t('history.totalShort')}: ${total == null ? '—' : fmt(total)}`];
                if (rows.length > 0) lines.push(`${t('history.requests')}: ${formatInt(bucketRequests(items[0].dataIndex))}`);
                if (onpickbucket && bucket !== 'hour') lines.push(t('chart.clickToNarrow'));
                return lines;
              },
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

  // One effect updates the existing instance for every input.
  $effect(() => {
    // touched explicitly so the effect re-runs on each of them
    void [rows, bucket, groupByModel, groupByProject, projectNames, metric, layout, themeKey, canvas, maxSeries, onpickbucket];
    render();
  });

  onDestroy(() => {
    chart?.destroy();
    chart = null;
  });
</script>

{#if unpricedRequests > 0}
  <p class="muted cost-note" role="status">{t('chart.unpricedRequests', { count: formatInt(unpricedRequests) })}</p>
{:else if missingPrices}<p class="muted cost-note" role="status">{t('chart.missingPrices')}</p>{/if}
<!-- Keyboard focus lets users scroll the chart when its grouped bars overflow. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div class="chart" role="region" aria-label={t('history.chartTitle')} style:height={`${height / 16}rem`} tabindex={layout === 'grouped' && rows.length > 0 ? 0 : undefined}>
  <div class="plot" style:min-width={plotWidth ? `${plotWidth}px` : undefined}>
    {#if rows.length === 0}
      <p class="empty">{t('chart.noData')}</p>
    {:else}
      <canvas bind:this={canvas} aria-label={t('chart.accessible')}></canvas>
    {/if}
  </div>
</div>

<style>
  .cost-note { margin: 0; font-size: 0.75rem; }
  .chart {
    position: relative;
    width: 100%;
    min-width: 0;
    overflow-x: auto;
  }
  .plot {
    position: relative;
    width: 100%;
    height: 100%;
  }

  /* chart.js owns the canvas' inline size (responsive + maintainAspectRatio
     false); it measures `.chart`, which has an explicit height. */
  canvas {
    display: block;
  }

  .empty {
    display: grid;
    place-items: center;
    height: 100%;
    margin: 0;
    color: var(--muted);
    font-size: 0.8125rem;
  }
</style>
