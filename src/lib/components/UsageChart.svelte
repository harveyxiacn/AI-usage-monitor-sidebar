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
  import { cssVar, lighten, PROVIDER_ACCENT, resolveColor } from '$lib/colors';
  import { formatBucket, formatCost, formatTokens } from '$lib/format';
  import { t } from '$lib/i18n/i18n.svelte';
  import type { Bucket, HistoryRow, ProviderId } from '$lib/types';

  Chart.register(BarController, BarElement, CategoryScale, LinearScale, Tooltip, Legend);

  interface Props {
    rows: HistoryRow[];
    bucket: Bucket;
    groupByModel: boolean;
    metric: 'tokens' | 'cost';
    /** changes whenever the palette changes, forcing a rebuild */
    themeKey: string;
    height?: number;
  }

  let { rows, bucket, groupByModel, metric, themeKey, height = 260 }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let chart: Chart<'bar', (number | null)[], string> | null = null;

  const value = (r: HistoryRow) => (metric === 'cost' ? r.estimatedCostUsd : r.totalTokens);
  const missingPrices = $derived(metric === 'cost' && rows.some((row) => row.estimatedCostUsd == null));
  const fmt = (v: number) => (metric === 'cost' ? formatCost(v) : formatTokens(v));

  /** series key → legend label; grouping by model prefixes the provider. */
  function seriesOf(r: HistoryRow): { key: string; label: string; provider: ProviderId } {
    if (groupByModel && r.model) {
      return { key: `${r.provider}/${r.model}`, label: r.model, provider: r.provider };
    }
    return { key: r.provider, label: r.provider === 'claude' ? 'Claude' : 'Codex', provider: r.provider };
  }

  interface Built {
    labels: string[];
    datasets: ChartDataset<'bar', (number | null)[]>[];
  }

  function build(): Built {
    // x axis: every distinct bucket in chronological order
    const buckets = [...new Set(rows.map((r) => r.bucketStart))].sort(
      (a, b) => Date.parse(a) - Date.parse(b)
    );
    const xIndex = new Map(buckets.map((b, i) => [b, i]));

    const order: string[] = [];
    const meta = new Map<string, { label: string; provider: ProviderId; data: (number | null)[] }>();
    for (const r of rows) {
      const s = seriesOf(r);
      let m = meta.get(s.key);
      if (!m) {
        m = { label: s.label, provider: s.provider, data: new Array(buckets.length).fill(0) };
        meta.set(s.key, m);
        order.push(s.key);
      }
      const index = xIndex.get(r.bucketStart) ?? 0;
      const amount = value(r);
      const previous = m.data[index];
      m.data[index] = amount == null || previous == null ? null : previous + amount;
    }
    // stable series order: claude first, then alphabetical inside a provider
    order.sort((a, b) => a.localeCompare(b));

    const providerIndex: Record<ProviderId, number> = { claude: 0, codex: 0 };
    return {
      labels: buckets.map((b) => formatBucket(b, bucket)),
      datasets: order.map((key) => {
        const m = meta.get(key)!;
        const base = resolveColor(PROVIDER_ACCENT[m.provider]);
        const color = groupByModel ? lighten(base, (providerIndex[m.provider]++ % 5) * 0.09, 0.75) : base;
        return {
          label: m.label,
          data: m.data,
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
    void [rows, bucket, groupByModel, metric, themeKey, canvas];
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
