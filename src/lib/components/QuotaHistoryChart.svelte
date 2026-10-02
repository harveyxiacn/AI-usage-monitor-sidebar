<script lang="ts">
  import { Chart, LinearScale, LineController, LineElement, PointElement, Tooltip, type ChartConfiguration, type Plugin } from 'chart.js';
  import { onDestroy } from 'svelte';
  import { cssVar, providerAccent, resolveColor } from '$lib/colors';
  import { intlLocale, t, tDyn } from '$lib/i18n/i18n.svelte';
  import type { QuotaHistorySeries } from '$lib/quota-history';
  import { withResetGaps } from '$lib/quota-cycles';
  import type { Thresholds } from '$lib/types';

  Chart.register(LineController, LineElement, PointElement, LinearScale, Tooltip);
  interface Props {
    series: QuotaHistorySeries;
    remaining: boolean;
    themeKey: string;
    /** warn/critical lines; omitted = none */
    thresholds?: Thresholds | null;
    /** dashed "at this pace" segment in *used* percent, from the last sample */
    forecast?: Array<{ x: number; y: number }> | null;
  }
  let { series, remaining, themeKey, thresholds = null, forecast = null }: Props = $props();
  let canvas = $state<HTMLCanvasElement | null>(null);
  type Point = { x: number; y: number | null };
  let chart: Chart<'line', Point[]> | null = null;

  $effect(() => {
    void [themeKey, thresholds, forecast, remaining, series];
    if (!canvas) { chart?.destroy(); chart = null; return; }
    const entries = series.entries;
    const firstTime = Date.parse(entries[0].sample.ts);
    const lastSample = Date.parse(entries.at(-1)!.sample.ts);
    const lastTime = forecast ? Math.max(lastSample, forecast.at(-1)!.x) : lastSample;
    const toShown = (used: number) => (remaining ? 100 - used : used);
    const used = entries.map(({ sample }) => ({ x: Date.parse(sample.ts), y: toShown(sample.usedPercent) }));
    const breaks = new Set(entries.flatMap((entry, i) => entry.event === 'reset' ? [i] : []));
    // a reset starts a new line instead of a vertical drop; keep the event of each drawn point aligned
    const points = withResetGaps(used, breaks);
    const eventAt = withResetGaps(entries.map((_, i) => ({ x: i, y: i })), breaks).map((p) => p.y === null ? null : entries[p.y].event);
    const warnColor = cssVar('--warn', '#f5c542');
    const criticalColor = cssVar('--critical', '#ff3b30');
    const text = cssVar('--muted', '#9a9aa3');
    const grid = cssVar('--grid', 'rgba(255,255,255,0.07)');
    const accent = resolveColor(providerAccent(series.provider));
    const pct = new Intl.NumberFormat(intlLocale(), { maximumFractionDigits: 2 });
    const date = new Intl.DateTimeFormat(intlLocale(), { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' });
    const label = t(remaining ? 'history.quota.remaining' : 'history.quota.used');
    const guides: Plugin<'line'> = {
      id: 'quotaThresholds',
      afterDatasetsDraw(c) {
        if (!thresholds) return;
        const { ctx, chartArea, scales } = c;
        ctx.save();
        ctx.lineWidth = 1;
        ctx.setLineDash([2, 4]);
        for (const [value, color] of [[thresholds.warn, warnColor], [thresholds.critical, criticalColor]] as const) {
          const y = scales.y.getPixelForValue(toShown(value));
          ctx.strokeStyle = color;
          ctx.beginPath(); ctx.moveTo(chartArea.left, y); ctx.lineTo(chartArea.right, y); ctx.stroke();
        }
        ctx.restore();
      },
    };
    const configuration: ChartConfiguration<'line', Point[]> = {
      type: 'line',
      plugins: [guides],
      data: { datasets: [{
        label,
        data: points,
        borderColor: accent,
        backgroundColor: accent,
        borderWidth: 2,
        stepped: 'before',
        pointRadius: entries.length === 1 ? 4 : 0,
        pointHoverRadius: 4,
        // Cycle/plan transitions are distinct from ordinary usage increases.
        spanGaps: false,
        segment: { borderDash: (ctx) => eventAt[ctx.p1DataIndex] === 'plan_change' ? [4, 4] : undefined },
      }, ...(forecast ? [{
        label: t('history.quota.forecast'),
        data: forecast.map((p) => ({ x: p.x, y: toShown(p.y) })),
        borderColor: accent,
        backgroundColor: accent,
        borderWidth: 2,
        borderDash: [6, 5],
        pointRadius: 0,
        pointHoverRadius: 0,
        pointHitRadius: 0,
      }] : [])] },
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
            filter: (item) => item.datasetIndex === 0,
            backgroundColor: cssVar('--surface-2', '#232329'),
            titleColor: cssVar('--text', '#f5f5f7'), bodyColor: cssVar('--text', '#f5f5f7'),
            borderColor: grid, borderWidth: 1,
            callbacks: {
              title: (items) => items[0]?.parsed.x != null ? new Date(items[0].parsed.x).toLocaleString(intlLocale()) : '',
              label: (ctx) => `${label}: ${pct.format(ctx.parsed.y ?? 0)}%`,
              afterLabel: (ctx) => { const event = eventAt[ctx.dataIndex]; return event ? tDyn(`history.quota.event.${event}`) : ''; },
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
