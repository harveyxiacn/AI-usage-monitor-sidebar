<!--
  Sessions → Insights: log-scale histogram with median / P90 markers.
  [FRONTEND] chart.js v4, tree-shaken. Bins are uniform on a log axis, so a
  marker's pixel position is linear in its fractional bin index.
-->
<script lang="ts">
  import { BarController, BarElement, CategoryScale, Chart, LinearScale, Tooltip, type ChartConfiguration, type Plugin } from 'chart.js';
  import { onDestroy } from 'svelte';
  import { cssVar } from '$lib/colors';
  import { st } from '$lib/session-labels.svelte';
  import { binPosition } from '$lib/session-insights';
  import type { Histogram } from '$lib/session-types';
  import ChartDataTable from '../history/ChartDataTable.svelte';

  Chart.register(BarController, BarElement, CategoryScale, LinearScale, Tooltip);

  interface Props {
    title: string;
    histogram: Histogram;
    format: (value: number) => string;
    themeKey: string;
    height?: number;
  }

  let { title, histogram, format, themeKey, height = 200 }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let chart: Chart | null = null;

  const binLabel = (b: { from: number; to: number }) => `${format(b.from)}–${format(b.to)}`;
  const tableRows = $derived(histogram.bins.map((b) => [binLabel(b), String(b.count)]));
  const summary = $derived(
    [histogram.median != null ? `${st('median')} ${format(histogram.median)}` : '', histogram.p90 != null ? `${st('p90')} ${format(histogram.p90)}` : '']
      .filter(Boolean).join(' · '),
  );

  function render() {
    if (!canvas || histogram.bins.length === 0) { chart?.destroy(); chart = null; return; }
    const text = cssVar('--muted', '#9a9aa3');
    const grid = cssVar('--grid', 'rgba(255,255,255,0.07)');
    const surface = cssVar('--surface-2', '#232329');
    const strong = cssVar('--text', '#f5f5f7');
    const bar = cssVar('--focus', '#5b8def');
    const accent = cssVar('--warn', '#f5c542');
    const bins = histogram.bins;
    const markers = [
      { value: histogram.median, label: st('median'), color: strong, dash: [] as number[] },
      { value: histogram.p90, label: st('p90'), color: accent, dash: [5, 3] },
    ];
    const plugin: Plugin<'bar'> = {
      id: 'insightMarkers',
      afterDatasetsDraw(c) {
        const { ctx, chartArea, scales } = c;
        const x = scales.x;
        if (!x) return;
        const width = (x.right - x.left) / bins.length;
        ctx.save();
        ctx.font = '11px sans-serif';
        ctx.textBaseline = 'top';
        let row = 0;
        for (const m of markers) {
          const pos = binPosition(m.value, bins);
          if (pos == null || m.value == null) continue;
          const px = x.left + width * pos;
          ctx.strokeStyle = m.color;
          ctx.fillStyle = m.color;
          ctx.lineWidth = 1.5;
          ctx.setLineDash(m.dash);
          ctx.beginPath();
          ctx.moveTo(px, chartArea.top);
          ctx.lineTo(px, chartArea.bottom);
          ctx.stroke();
          ctx.setLineDash([]);
          const label = `${m.label} ${format(m.value)}`;
          const w = ctx.measureText(label).width;
          ctx.textAlign = px + w + 6 > chartArea.right ? 'right' : 'left';
          ctx.fillText(label, px + (ctx.textAlign === 'right' ? -4 : 4), chartArea.top + 2 + row * 14);
          row++;
        }
        ctx.restore();
      },
    };
    const configuration: ChartConfiguration<'bar', number[], string> = {
      type: 'bar',
      data: { labels: bins.map(binLabel), datasets: [{ label: st('sessionsAxis'), data: bins.map((b) => b.count), backgroundColor: bar, borderWidth: 0, categoryPercentage: 1, barPercentage: 0.92 }] },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        animation: false,
        scales: {
          x: { grid: { display: false }, border: { color: grid }, ticks: { color: text, maxRotation: 0, autoSkipPadding: 14, font: { size: 10 } } },
          y: { beginAtZero: true, grid: { color: grid }, border: { display: false }, ticks: { color: text, precision: 0, font: { size: 11 } }, title: { display: true, text: st('sessionsAxis'), color: text, font: { size: 11 } } },
        },
        plugins: {
          legend: { display: false },
          tooltip: { backgroundColor: surface, titleColor: strong, bodyColor: strong, borderColor: grid, borderWidth: 1, padding: 10, callbacks: { label: (ctx) => ` ${st('sessionsAxis')}: ${ctx.parsed.y}` } },
        },
      },
      plugins: [plugin],
    };
    chart?.destroy();
    chart = new Chart(canvas, configuration as ChartConfiguration);
  }

  $effect(() => {
    void [histogram, themeKey, canvas];
    render();
  });

  onDestroy(() => {
    chart?.destroy();
    chart = null;
  });
</script>

<figure class="hist">
  <figcaption><strong>{title}</strong>{#if summary}<span class="muted">{summary}</span>{/if}</figcaption>
  {#if histogram.bins.length === 0}
    <p class="muted">{st('topNone')}</p>
  {:else}
    <div class="chart" style:height={`${height / 16}rem`}>
      <canvas bind:this={canvas} aria-label={`${title}. ${summary}`}></canvas>
    </div>
    {#if histogram.zeroCount > 0}<p class="muted small">{histogram.zeroCount} {st('zeroSessions')}</p>{/if}
    <ChartDataTable caption={title} columns={[st('bin'), st('sessionsAxis')]} rows={tableRows} />
  {/if}
</figure>

<style>
  .hist { margin: 0; display: grid; gap: 0.5rem; min-width: 0; }
  figcaption { display: flex; flex-wrap: wrap; justify-content: space-between; gap: 0.5rem; font-size: 0.85rem; }
  .chart { position: relative; min-width: 0; }
  .small { font-size: 0.72rem; margin: 0; }
  p { margin: 0; }
</style>
