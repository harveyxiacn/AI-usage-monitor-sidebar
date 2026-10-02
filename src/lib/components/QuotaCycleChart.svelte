<!--
  Peak used percent of each quota cycle as bars; cycles that reached the
  critical threshold are drawn in the critical colour, the running cycle is
  lighter. Same tree-shaken Chart.js registration style as UsageChart. [FRONTEND]
-->
<script lang="ts">
  import { BarController, BarElement, CategoryScale, Chart, LinearScale, Tooltip, type ChartConfiguration } from 'chart.js';
  import { onDestroy } from 'svelte';
  import { cssVar, providerAccent, resolveColor, withAlpha } from '$lib/colors';
  import { formatCost, formatTokens } from '$lib/format';
  import { intlLocale, t } from '$lib/i18n/i18n.svelte';
  import type { CycleUsage } from '$lib/quota-cycles';
  import type { ProviderId, Thresholds } from '$lib/types';

  Chart.register(BarController, BarElement, CategoryScale, LinearScale, Tooltip);

  interface Props {
    provider: ProviderId;
    usages: CycleUsage[];
    thresholds: Thresholds;
    /** 5-hour cycles need the time of day on the axis, weekly ones do not */
    withTime: boolean;
    themeKey: string;
  }
  let { provider, usages, thresholds, withTime, themeKey }: Props = $props();
  let canvas = $state<HTMLCanvasElement | null>(null);
  let chart: Chart<'bar', number[], string> | null = null;

  $effect(() => {
    void themeKey;
    if (!canvas || usages.length === 0) { chart?.destroy(); chart = null; return; }
    const text = cssVar('--muted', '#9a9aa3');
    const grid = cssVar('--grid', 'rgba(255,255,255,0.07)');
    const critical = resolveColor('var(--critical)', '#ff3b30');
    const accent = resolveColor(providerAccent(provider));
    const label = new Intl.DateTimeFormat(intlLocale(), withTime
      ? { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }
      : { month: 'numeric', day: 'numeric' });
    const colors = usages.map(({ cycle }) => {
      const base = cycle.peak >= thresholds.critical ? critical : accent;
      return cycle.completed ? base : withAlpha(base, 0.55);
    });
    const configuration: ChartConfiguration<'bar', number[], string> = {
      type: 'bar',
      data: {
        labels: usages.map(({ cycle }) => label.format(cycle.end)),
        datasets: [{ label: t('history.quota.peak'), data: usages.map(({ cycle }) => cycle.peak), backgroundColor: colors, borderRadius: 2, maxBarThickness: 28 }],
      },
      options: {
        responsive: true, maintainAspectRatio: false, animation: false,
        scales: {
          x: { grid: { display: false }, border: { color: grid }, ticks: { color: text, maxRotation: 0, autoSkipPadding: 12, font: { size: 11 } } },
          y: { min: 0, max: 100, border: { display: false }, grid: { color: grid }, ticks: { color: text, callback: (v) => `${v}%` } },
        },
        plugins: {
          legend: { display: false },
          tooltip: {
            backgroundColor: cssVar('--surface-2', '#232329'),
            titleColor: cssVar('--text', '#f5f5f7'), bodyColor: cssVar('--text', '#f5f5f7'),
            borderColor: grid, borderWidth: 1,
            callbacks: {
              title: (items) => items[0] ? new Date(usages[items[0].dataIndex].cycle.end).toLocaleString(intlLocale()) : '',
              label: (ctx) => ` ${t('history.quota.peak')}: ${Math.round(ctx.parsed.y ?? 0)}%`,
              afterLabel: (ctx) => {
                const totals = usages[ctx.dataIndex]?.totals;
                if (!totals) return '';
                const cost = totals.estimatedCostUsd != null ? ` · ≈ ${formatCost(totals.estimatedCostUsd)}` : '';
                return ` ${t('history.quota.tokens')}: ${formatTokens(totals.totalTokens)}${cost}`;
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
    } else { chart?.destroy(); chart = new Chart(canvas, configuration); }
  });
  onDestroy(() => { chart?.destroy(); chart = null; });
</script>

<div class="cycle-chart">
  <canvas bind:this={canvas} aria-label={t('history.quota.cycles.accessible')}></canvas>
</div>

<style>
  .cycle-chart { position: relative; height: 10rem; min-width: 0; width: 100%; }
  canvas { display: block; }
</style>
