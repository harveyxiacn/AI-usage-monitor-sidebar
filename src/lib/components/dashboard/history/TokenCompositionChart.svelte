<!--
  Token composition: per bucket, the share of input / cache read / cache write /
  output / reasoning tokens as a 100 % stacked bar, with the cache hit rate as
  a line on a second axis. [FRONTEND] chart.js v4, tree-shaken registration.

  Reasoning is carved out of the output counter (see `buildComposition`), so
  the five segments add up to the bucket's total. The chart instance is reused
  across data and palette changes, like UsageChart.
-->
<script lang="ts">
  import {
    BarController,
    BarElement,
    CategoryScale,
    Chart,
    Legend,
    LinearScale,
    LineController,
    LineElement,
    PointElement,
    Tooltip,
    type ChartConfiguration,
  } from 'chart.js';
  import { onDestroy } from 'svelte';
  import { buildComposition, sharePercent } from '$lib/analytics';
  import { COMPOSITION_COLORS, cssVar } from '$lib/colors';
  import { formatBucket, formatTokens } from '$lib/format';
  import { t } from '$lib/i18n/i18n.svelte';
  import type { Bucket, HistoryRow } from '$lib/types';
  import ChartDataTable from './ChartDataTable.svelte';

  Chart.register(BarController, BarElement, LineController, LineElement, PointElement, CategoryScale, LinearScale, Tooltip, Legend);

  interface Props {
    rows: HistoryRow[];
    bucket: Bucket;
    themeKey: string;
    height?: number;
  }

  let { rows, bucket, themeKey, height = 260 }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let chart: Chart | null = null;

  const composition = $derived(buildComposition(rows));
  const percent = (v: number) => `${Math.round(v)}%`;

  const SEGMENTS = [
    ['input', 'history.input'],
    ['cacheRead', 'history.cacheRead'],
    ['cacheWrite', 'history.cacheWrite'],
    ['output', 'history.output'],
    ['reasoning', 'history.reasoning'],
  ] as const;

  const tableColumns = $derived([
    t('history.table.bucket'),
    ...SEGMENTS.map(([, key]) => t(key)),
    t('history.kpi.cacheHit'),
  ]);
  const tableRows = $derived(composition.map((b) => [
    formatBucket(b.bucketStart, bucket),
    ...SEGMENTS.map(([id]) => `${percent(sharePercent(b[id], b.total))} · ${formatTokens(b[id])}`),
    b.hitRate == null ? '—' : percent(b.hitRate * 100),
  ]));

  function render() {
    if (!canvas || composition.length === 0) { chart?.destroy(); chart = null; return; }

    const text = cssVar('--muted', '#9a9aa3');
    const grid = cssVar('--grid', 'rgba(255,255,255,0.07)');
    const surface = cssVar('--surface-2', '#232329');
    const strong = cssVar('--text', '#f5f5f7');
    const palette = COMPOSITION_COLORS[themeKey === 'light' ? 'light' : 'dark'];

    const configuration: ChartConfiguration<'bar' | 'line', (number | null)[], string> = {
      type: 'bar',
      data: {
        labels: composition.map((b) => formatBucket(b.bucketStart, bucket)),
        datasets: [
          ...SEGMENTS.map(([id, key]) => ({
            type: 'bar' as const,
            label: t(key),
            data: composition.map((b) => sharePercent(b[id], b.total)),
            backgroundColor: palette[id],
            borderWidth: 0,
            stack: 'mix',
            maxBarThickness: 44,
            yAxisID: 'y',
            order: 1,
          })),
          {
            type: 'line' as const,
            label: t('history.kpi.cacheHit'),
            data: composition.map((b) => (b.hitRate == null ? null : b.hitRate * 100)),
            borderColor: strong,
            backgroundColor: strong,
            borderWidth: 2,
            borderDash: [5, 3],
            pointStyle: 'rectRot',
            pointRadius: 3.5,
            pointHoverRadius: 5,
            tension: 0.2,
            spanGaps: true,
            yAxisID: 'y1',
            order: 0,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        animation: false,
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
            min: 0,
            max: 100,
            grid: { color: grid },
            border: { display: false },
            title: { display: true, text: t('history.composition.shareAxis'), color: text, font: { size: 11 } },
            ticks: { color: text, font: { size: 11 }, callback: (v) => `${v}%` },
          },
          y1: {
            position: 'right',
            min: 0,
            max: 100,
            grid: { display: false },
            border: { display: false },
            title: { display: true, text: t('history.kpi.cacheHit'), color: text, font: { size: 11 } },
            ticks: { color: text, font: { size: 11 }, callback: (v) => `${v}%` },
          },
        },
        plugins: {
          legend: {
            position: 'bottom',
            labels: { color: text, boxWidth: 10, boxHeight: 10, usePointStyle: true, pointStyle: 'rectRounded', font: { size: 11 } },
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
                if (ctx.dataset.type === 'line') {
                  return ` ${ctx.dataset.label}: ${ctx.parsed.y == null ? '—' : percent(ctx.parsed.y)}`;
                }
                const segment = SEGMENTS[ctx.datasetIndex];
                const count = segment ? composition[ctx.dataIndex]?.[segment[0]] ?? 0 : 0;
                return ` ${ctx.dataset.label}: ${percent(ctx.parsed.y ?? 0)} · ${formatTokens(count)}`;
              },
              footer: (items) => {
                const total = composition[items[0]?.dataIndex]?.total;
                return total == null ? '' : `${t('history.totalShort')}: ${formatTokens(total)}`;
              },
            },
          },
        },
      },
    };
    if (chart?.canvas === canvas) {
      chart.data = configuration.data as typeof chart.data;
      chart.options = (configuration.options ?? {}) as typeof chart.options;
      chart.update('none');
    } else {
      chart?.destroy();
      chart = new Chart(canvas, configuration as ChartConfiguration);
    }
  }

  $effect(() => {
    void [composition, bucket, themeKey, canvas];
    render();
  });

  onDestroy(() => {
    chart?.destroy();
    chart = null;
  });
</script>

<div class="chart" style:height={`${height / 16}rem`}>
  {#if composition.length === 0}
    <p class="empty">{t('chart.noData')}</p>
  {:else}
    <canvas bind:this={canvas} aria-label={t('history.composition.accessible')}></canvas>
  {/if}
</div>
<ChartDataTable caption={t('history.composition.title')} columns={tableColumns} rows={tableRows} />

<style>
  .chart {
    position: relative;
    width: 100%;
    min-width: 0;
  }

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
