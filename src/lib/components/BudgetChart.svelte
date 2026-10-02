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
    type ChartConfiguration,
  } from 'chart.js';
  import { onDestroy } from 'svelte';
  import { cssVar, HEAT_ACCENT } from '$lib/colors';
  import { formatCost } from '$lib/format';
  import { daysInMonthOf, projectToMonthEnd } from '$lib/subscription';
  import { intlLocale, t } from '$lib/i18n/i18n.svelte';

  Chart.register(LineController, LineElement, PointElement, CategoryScale, LinearScale, Tooltip, Legend);

  interface Props {
    /** cumulative spend per elapsed local day of the current month */
    series: Array<{ date: string; cumulativeUsd: number }>;
    /** monthly budget; 0 = no budget line */
    budgetUsd: number;
    /** summed monthly subscription price; 0 = no subscription line (and no month-end projection) */
    subscriptionUsd?: number;
    /** changes whenever the palette changes, forcing a rebuild */
    themeKey: string;
    height?: number;
  }

  let { series, budgetUsd, subscriptionUsd = 0, themeKey, height = 200 }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let chart: Chart<'line', (number | null)[], string> | null = null;

  function render() {
    if (!canvas || series.length === 0) { chart?.destroy(); chart = null; return; }

    const text = cssVar('--muted', '#9a9aa3');
    const grid = cssVar('--grid', 'rgba(255,255,255,0.07)');
    const surface = cssVar('--surface-2', '#232329');
    const strong = cssVar('--text', '#f5f5f7');
    const accent = HEAT_ACCENT[themeKey === 'light' ? 'light' : 'dark'];
    const day = new Intl.DateTimeFormat(intlLocale(), { month: 'numeric', day: 'numeric' });
    // with a subscription the axis runs to month end and the current pace is projected onto it
    const first = new Date(`${series[0].date}T00:00:00`);
    const days = subscriptionUsd > 0 ? Math.max(series.length, daysInMonthOf(first.getTime())) : series.length;
    const labels = Array.from({ length: days }, (_, i) => day.format(new Date(first.getFullYear(), first.getMonth(), first.getDate() + i)));
    const pad = <T,>(values: T[]): (T | null)[] => [...values, ...Array.from({ length: days - values.length }, () => null)];
    const sub = cssVar('--warn', '#f5c542');

    const configuration: ChartConfiguration<'line', (number | null)[], string> = {
      type: 'line',
      data: {
        labels,
        datasets: [
          {
            label: t('history.budget.spent'),
            data: pad(series.map((point) => point.cumulativeUsd)),
            borderColor: accent,
            backgroundColor: accent,
            borderWidth: 2,
            pointRadius: 0,
            pointHoverRadius: 4,
            tension: 0.15,
          },
          ...(budgetUsd > 0 ? [{
            label: t('history.budget.line'),
            data: labels.map(() => budgetUsd),
            borderColor: text,
            backgroundColor: text,
            borderWidth: 1.5,
            borderDash: [5, 4],
            pointRadius: 0,
            pointHoverRadius: 0,
          }] : []),
          ...(subscriptionUsd > 0 ? [{
            label: t('history.roi.line'),
            data: labels.map(() => subscriptionUsd),
            borderColor: sub,
            backgroundColor: sub,
            borderWidth: 1.5,
            borderDash: [2, 3],
            pointRadius: 0,
            pointHoverRadius: 0,
          }, {
            label: t('history.roi.projectedLine'),
            data: projectToMonthEnd(series, days),
            borderColor: accent,
            backgroundColor: accent,
            borderWidth: 1.5,
            borderDash: [6, 4],
            pointRadius: 0,
            pointHoverRadius: 0,
          }] : []),
        ],
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
            beginAtZero: true,
            // the budget must stay on screen even on a quiet month
            suggestedMax: Math.max(budgetUsd, subscriptionUsd) * 1.05,
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
            filter: (item) => item.parsed.y != null,
            backgroundColor: surface,
            titleColor: strong,
            bodyColor: strong,
            borderColor: grid,
            borderWidth: 1,
            padding: 10,
            callbacks: {
              label: (ctx) => ctx.parsed.y == null ? '' : ` ${ctx.dataset.label}: ${formatCost(ctx.parsed.y)}`,
            },
          },
        },
      },
    };
    if (chart?.canvas === canvas) {
      chart.data = configuration.data;
      chart.options = configuration.options ?? {};
      chart.update('none');
    } else { chart?.destroy(); chart = new Chart(canvas, configuration); }
  }

  $effect(() => {
    void [series, budgetUsd, subscriptionUsd, themeKey, canvas];
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
