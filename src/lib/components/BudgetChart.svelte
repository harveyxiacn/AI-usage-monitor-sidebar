<!--
  Month-to-date burn-up of the *estimated* cost against the monthly budget.
  [FRONTEND] chart.js v4, same tree-shaken registration style as UsageChart.

  One measure, one axis, one series: the cumulative spend. The budget is a flat
  dashed reference in the muted ink rather than a second colour — it is chrome,
  not data — and the headline numbers live next to the chart as text, so the
  chart never has to carry a value label on every point.
-->
<script lang="ts">
  import {
    CategoryScale,
    Chart,
    Legend,
    LinearScale,
    LineController,
    LineElement,
    PointElement,
    Tooltip,
  } from 'chart.js';
  import { onDestroy } from 'svelte';
  import { cssVar, HEAT_ACCENT } from '$lib/colors';
  import { formatCost } from '$lib/format';
  import { intlLocale, t } from '$lib/i18n/i18n.svelte';

  Chart.register(LineController, LineElement, PointElement, CategoryScale, LinearScale, Tooltip, Legend);

  interface Props {
    /** cumulative spend per elapsed local day of the current month */
    series: Array<{ date: string; cumulativeUsd: number }>;
    budgetUsd: number;
    /** changes whenever the palette changes, forcing a rebuild */
    themeKey: string;
    height?: number;
  }

  let { series, budgetUsd, themeKey, height = 200 }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let chart: Chart<'line', (number | null)[], string> | null = null;

  function render() {
    chart?.destroy();
    chart = null;
    if (!canvas || series.length === 0) return;

    const text = cssVar('--muted', '#9a9aa3');
    const grid = cssVar('--grid', 'rgba(255,255,255,0.07)');
    const surface = cssVar('--surface-2', '#232329');
    const strong = cssVar('--text', '#f5f5f7');
    const accent = HEAT_ACCENT[themeKey === 'light' ? 'light' : 'dark'];
    const day = new Intl.DateTimeFormat(intlLocale(), { month: 'numeric', day: 'numeric' });

    chart = new Chart<'line', (number | null)[], string>(canvas, {
      type: 'line',
      data: {
        labels: series.map((point) => day.format(new Date(`${point.date}T00:00:00`))),
        datasets: [
          {
            label: t('history.budget.spent'),
            data: series.map((point) => point.cumulativeUsd),
            borderColor: accent,
            backgroundColor: accent,
            borderWidth: 2,
            pointRadius: 0,
            pointHoverRadius: 4,
            tension: 0.15,
          },
          {
            label: t('history.budget.line'),
            data: series.map(() => budgetUsd),
            borderColor: text,
            backgroundColor: text,
            borderWidth: 1.5,
            borderDash: [5, 4],
            pointRadius: 0,
            pointHoverRadius: 0,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        animation: { duration: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 320 },
        interaction: { mode: 'index', intersect: false },
        scales: {
          x: {
            grid: { display: false },
            border: { color: grid },
            ticks: { color: text, maxRotation: 0, autoSkipPadding: 16, font: { size: 11 } },
          },
          y: {
            beginAtZero: true,
            // the budget must stay on screen even on a quiet month
            suggestedMax: budgetUsd * 1.05,
            grid: { color: grid },
            border: { display: false },
            ticks: { color: text, font: { size: 11 }, callback: (v) => formatCost(Number(v)) },
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
              label: (ctx) => ` ${ctx.dataset.label}: ${formatCost(ctx.parsed.y)}`,
            },
          },
        },
      },
    });
  }

  $effect(() => {
    void [series, budgetUsd, themeKey, canvas];
    render();
  });

  onDestroy(() => {
    chart?.destroy();
    chart = null;
  });
</script>

<div class="chart" style:height={`${height / 16}rem`}>
  {#if series.length === 0}
    <p class="empty">{t('chart.noData')}</p>
  {:else}
    <canvas bind:this={canvas} aria-label={t('history.budget.accessible')}></canvas>
  {/if}
</div>

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
