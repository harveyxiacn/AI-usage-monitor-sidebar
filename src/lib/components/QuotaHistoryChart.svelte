<script lang="ts">
  import { Chart, LinearScale, LineController, LineElement, PointElement, Tooltip, type ChartConfiguration } from 'chart.js';
  import { onDestroy } from 'svelte';
  import { cssVar, providerAccent, resolveColor } from '$lib/colors';
  import { intlLocale, t, tDyn } from '$lib/i18n/i18n.svelte';
  import type { QuotaHistorySeries } from '$lib/quota-history';

  Chart.register(LineController, LineElement, PointElement, LinearScale, Tooltip);
  interface Props { series: QuotaHistorySeries; remaining: boolean; themeKey: string }
  let { series, remaining, themeKey }: Props = $props();
  let canvas = $state<HTMLCanvasElement | null>(null);
  let chart: Chart<'line', { x: number; y: number }[]> | null = null;

  $effect(() => {
    void themeKey;
    if (!canvas) { chart?.destroy(); chart = null; return; }
    const entries = series.entries;
    const firstTime = Date.parse(entries[0].sample.ts);
    const lastTime = Date.parse(entries.at(-1)!.sample.ts);
    const text = cssVar('--muted', '#9a9aa3');
    const grid = cssVar('--grid', 'rgba(255,255,255,0.07)');
    const accent = resolveColor(providerAccent(series.provider));
    const pct = new Intl.NumberFormat(intlLocale(), { maximumFractionDigits: 2 });
    const date = new Intl.DateTimeFormat(intlLocale(), { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' });
    const label = t(remaining ? 'history.quota.remaining' : 'history.quota.used');
    const configuration: ChartConfiguration<'line', { x: number; y: number }[]> = {
      type: 'line',
      data: { datasets: [{
        label,
        data: entries.map(({ sample }) => ({ x: Date.parse(sample.ts), y: remaining ? 100 - sample.usedPercent : sample.usedPercent })),
        borderColor: accent,
        backgroundColor: accent,
        borderWidth: 2,
        stepped: 'before',
        pointRadius: entries.length === 1 ? 4 : 0,
        pointHoverRadius: 4,
        // Cycle/plan transitions are distinct from ordinary usage increases.
        segment: { borderDash: (ctx) => ['reset', 'plan_change'].includes(entries[ctx.p1DataIndex]?.event) ? [4, 4] : undefined },
      }] },
      options: {
        responsive: true, maintainAspectRatio: false, animation: false,
        parsing: false, normalized: true,
        interaction: { mode: 'nearest', axis: 'x', intersect: false },
        scales: {
          x: { type: 'linear', min: firstTime === lastTime ? firstTime - 60_000 : firstTime,
            max: firstTime === lastTime ? lastTime + 60_000 : lastTime,
            grid: { display: false }, border: { color: grid },
            ticks: { color: text, maxRotation: 0, maxTicksLimit: 6, callback: (v) => date.format(Number(v)) } },
          y: { min: 0, max: 100, border: { display: false }, grid: { color: grid },
            ticks: { color: text, callback: (v) => `${v}%` } },
        },
        plugins: {
          legend: { display: false },
          tooltip: {
            backgroundColor: cssVar('--surface-2', '#232329'),
            titleColor: cssVar('--text', '#f5f5f7'), bodyColor: cssVar('--text', '#f5f5f7'),
            borderColor: grid, borderWidth: 1,
            callbacks: {
              title: (items) => items[0]?.parsed.x != null ? new Date(items[0].parsed.x).toLocaleString(intlLocale()) : '',
              label: (ctx) => `${label}: ${pct.format(ctx.parsed.y ?? 0)}%`,
              afterLabel: (ctx) => tDyn(`history.quota.event.${entries[ctx.dataIndex].event}`),
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

<div class="quota-chart">
  <canvas bind:this={canvas} aria-label={t('history.quota.accessible')}></canvas>
</div>

<style>
  .quota-chart { position: relative; height: 15rem; min-width: 0; width: 100%; }
  canvas { display: block; }
</style>
