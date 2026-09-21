<!--
  Stacked bar chart of token usage / estimated cost. [FRONTEND]
  chart.js v4, tree-shaken registration (bar controller + the two scales + the
  tooltip/legend plugins only).

  The chart lives outside Svelte's reactivity, so the whole instance is rebuilt
  from scratch whenever the data, the metric or the theme changes — the data
  sets are small (≤ ~400 bars) and this keeps the colour resolution honest:
  chart.js needs concrete colour strings, and our palette lives in CSS custom
  properties that change with the theme.
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
  } from 'chart.js';
  import { onDestroy } from 'svelte';
  import { cssVar, lighten, providerAccent, resolveColor } from '$lib/colors';
  import { formatBucket, formatCost, formatTokens } from '$lib/format';
  import { historySeries, projectLabels, shortenLabel } from '$lib/history';
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
    /** changes whenever the palette changes, forcing a rebuild */
    themeKey: string;
    height?: number;
  }

  let { rows, bucket, groupByModel, groupByProject = false, projectNames, metric, themeKey, height = 260 }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let chart: Chart<'bar', (number | null)[], string> | null = null;

  const missingPrices = $derived(metric === 'cost' && rows.some((row) => row.estimatedCostUsd == null));
  const fmt = (v: number) => (metric === 'cost' ? formatCost(v) : formatTokens(v));

  interface Built {
    labels: string[];
    datasets: ChartDataset<'bar', (number | null)[]>[];
  }

  function build(): Built {
    const { buckets, series } = historySeries(rows, groupByModel, groupByProject, metric);
    const unassigned = t('history.project.unassigned');
    const own = projectLabels(series.map((entry) => entry.project ?? ''), unassigned);
    const projects = projectNames ?? own;
    const providerIndex: Record<string, number> = {};
    return {
      labels: buckets.map((b) => formatBucket(b, bucket)),
      datasets: series.map((entry) => {
        const path = entry.project ?? '';
        const label = [providerDisplayName(entry.provider)];
        if (groupByModel) label.push(entry.model || t('history.modelUnknown'));
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

  function render() {
    chart?.destroy();
    chart = null;
    if (!canvas || rows.length === 0) return;

    const text = cssVar('--muted', '#9a9aa3');
    const grid = cssVar('--grid', 'rgba(255,255,255,0.07)');
    const surface = cssVar('--surface-2', '#232329');
    const strong = cssVar('--text', '#f5f5f7');
    const { labels, datasets } = build();

    chart = new Chart<'bar', (number | null)[], string>(canvas, {
      type: 'bar',
      data: { labels, datasets },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        animation: { duration: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 320 },
        interaction: { mode: 'index', intersect: false },
        scales: {
          x: {
            stacked: true,
            grid: { display: false },
            border: { color: grid },
            ticks: { color: text, maxRotation: 0, autoSkipPadding: 16, font: { size: 11 } },
          },
          y: {
            stacked: true,
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
              label: (ctx) => ` ${ctx.dataset.label}: ${ctx.parsed.y == null ? '—' : fmt(ctx.parsed.y)}`,
            },
          },
        },
      },
    });
  }

  // one $effect that reads every input: any change rebuilds the chart
  $effect(() => {
    // touched explicitly so the effect re-runs on each of them
    void [rows, bucket, groupByModel, groupByProject, projectNames, metric, themeKey, canvas];
    render();
  });

  onDestroy(() => {
    chart?.destroy();
    chart = null;
  });
</script>

{#if missingPrices}<p class="muted cost-note" role="status">{t('chart.missingPrices')}</p>{/if}
<div class="chart" style:height={`${height / 16}rem`}>
  {#if rows.length === 0}
    <p class="empty">{t('chart.noData')}</p>
  {:else}
    <canvas bind:this={canvas} aria-label={t('chart.accessible')}></canvas>
  {/if}
</div>

<style>
  .cost-note { margin: 0; font-size: 0.75rem; }
  .chart {
    position: relative;
    width: 100%;
    min-width: 0;
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
