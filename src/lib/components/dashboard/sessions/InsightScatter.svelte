<!--
  Sessions → Insights: user turns vs. estimated cost. Point size = tool
  failures, colour = provider; selecting a point opens that session.
  [FRONTEND] chart.js v4 bubble chart. The collapsed data table below and the
  top lists are the keyboard / screen-reader route to the same sessions.
-->
<script lang="ts">
  import { BubbleController, Chart, Legend, LinearScale, PointElement, Tooltip, type ChartConfiguration } from 'chart.js';
  import { onDestroy } from 'svelte';
  import { cssVar, providerAccent, resolveColor } from '$lib/colors';
  import { formatInt } from '$lib/format';
  import { providerDisplayName } from '$lib/providers';
  import { st } from '$lib/session-labels.svelte';
  import { bubbleRadius, formatUsdCompact } from '$lib/session-insights';
  import type { InsightSession } from '$lib/session-types';
  import ChartDataTable from '../history/ChartDataTable.svelte';

  Chart.register(BubbleController, PointElement, LinearScale, Tooltip, Legend);

  interface Props {
    points: InsightSession[];
    themeKey: string;
    onopen: (provider: string, sessionId: string) => void;
    height?: number;
  }

  let { points, themeKey, onopen, height = 300 }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let chart: Chart | null = null;

  const priced = $derived(points.filter((p) => p.costUsd != null));
  const providers = $derived([...new Set(priced.map((p) => p.provider))].sort());
  const unpriced = $derived(points.length - priced.length);
  const tableRows = $derived(
    [...priced].sort((a, b) => (b.costUsd ?? 0) - (a.costUsd ?? 0)).slice(0, 100)
      .map((p) => [`${p.title} · ${providerDisplayName(p.provider)}`, formatInt(p.userTurns), formatUsdCompact(p.costUsd ?? 0), formatInt(p.toolFailures)]),
  );

  function render() {
    if (!canvas || priced.length === 0) { chart?.destroy(); chart = null; return; }
    const text = cssVar('--muted', '#9a9aa3');
    const grid = cssVar('--grid', 'rgba(255,255,255,0.07)');
    const surface = cssVar('--surface-2', '#232329');
    const strong = cssVar('--text', '#f5f5f7');
    const groups = providers.map((provider) => priced.filter((p) => p.provider === provider));
    const configuration: ChartConfiguration<'bubble'> = {
      type: 'bubble',
      data: {
        datasets: groups.map((rows, i) => {
          const color = resolveColor(providerAccent(providers[i] as 'claude'));
          return {
            label: providerDisplayName(providers[i]),
            data: rows.map((p) => ({ x: p.userTurns, y: p.costUsd ?? 0, r: bubbleRadius(p.toolFailures) })),
            backgroundColor: /^#[0-9a-f]{6}$/i.test(color) ? `${color}99` : color,
            borderColor: color,
            borderWidth: 1.5,
            hoverBorderWidth: 2.5,
          };
        }),
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        animation: false,
        onClick: (event, _elements, c) => {
          const hit = c.getElementsAtEventForMode(event as unknown as Event, 'nearest', { intersect: true }, true)[0];
          const row = hit ? groups[hit.datasetIndex]?.[hit.index] : undefined;
          if (row) onopen(row.provider, row.sessionId);
        },
        onHover: (event, elements) => {
          const target = event.native?.target as HTMLElement | undefined;
          if (target) target.style.cursor = elements.length ? 'pointer' : 'default';
        },
        scales: {
          x: { beginAtZero: true, grace: '5%', grid: { color: grid }, border: { display: false }, ticks: { color: text, precision: 0, font: { size: 11 } }, title: { display: true, text: st('turnsAxis'), color: text, font: { size: 11 } } },
          y: { beginAtZero: true, grace: '8%', grid: { color: grid }, border: { display: false }, ticks: { color: text, font: { size: 11 }, callback: (v) => formatUsdCompact(Number(v)) }, title: { display: true, text: st('costAxis'), color: text, font: { size: 11 } } },
        },
        plugins: {
          legend: { position: 'bottom', labels: { color: text, boxWidth: 10, boxHeight: 10, usePointStyle: true, font: { size: 11 } } },
          tooltip: {
            backgroundColor: surface, titleColor: strong, bodyColor: strong, borderColor: grid, borderWidth: 1, padding: 10,
            callbacks: {
              title: (items) => { const p = groups[items[0].datasetIndex]?.[items[0].dataIndex]; return p?.title ?? ''; },
              label: (ctx) => {
                const p = groups[ctx.datasetIndex]?.[ctx.dataIndex];
                return p ? [` ${st('turnsAxis')}: ${p.userTurns}`, ` ${st('estimatedCost')}: ${formatUsdCompact(p.costUsd ?? 0)}`, ` ${st('failures')}: ${p.toolFailures}`] : '';
              },
            },
          },
        },
      },
    };
    chart?.destroy();
    chart = new Chart(canvas, configuration as ChartConfiguration);
  }

  $effect(() => {
    void [priced, themeKey, canvas];
    render();
  });

  onDestroy(() => {
    chart?.destroy();
    chart = null;
  });
</script>

<figure class="scatter">
  <figcaption><strong>{st('scatterTitle')}</strong><span class="muted">{st('scatterNote')}</span></figcaption>
  {#if priced.length === 0}
    <p class="muted">{st('topNone')}</p>
  {:else}
    <div class="chart" style:height={`${height / 16}rem`}>
      <canvas bind:this={canvas} aria-label={`${st('scatterTitle')}. ${st('scatterNote')}`}></canvas>
    </div>
    {#if unpriced > 0}<p class="muted small">{st('scatterUnpriced', { n: unpriced })}</p>{/if}
    <ChartDataTable caption={st('scatterTitle')} columns={[st('title'), st('turnsAxis'), st('costAxis'), st('failures')]} rows={tableRows} />
  {/if}
</figure>

<style>
  .scatter { margin: 0; display: grid; gap: 0.5rem; min-width: 0; }
  figcaption { display: flex; flex-wrap: wrap; justify-content: space-between; gap: 0.25rem 0.75rem; font-size: 0.85rem; }
  figcaption .muted { font-size: 0.72rem; }
  .chart { position: relative; min-width: 0; }
  .small { font-size: 0.72rem; margin: 0; }
  p { margin: 0; }
</style>
