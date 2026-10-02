<!--
  Sessions → Insights: KPIs, distributions, scatter, ranked lists and tool
  usage for the sessions matching the tab's filters. One backend call
  (`get_session_insights`); metrics and metadata only, never content.
  [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { getSessionInsights } from '$lib/api';
  import { formatDuration, formatInt } from '$lib/format';
  import { st } from '$lib/session-labels.svelte';
  import { formatRate, formatUsdCompact } from '$lib/session-insights';
  import type { InsightSession, SessionInsights, SessionListQuery } from '$lib/session-types';
  import InsightHistogram from './InsightHistogram.svelte';
  import InsightScatter from './InsightScatter.svelte';
  import InsightTopList from './InsightTopList.svelte';

  interface Props {
    /** the applied Sessions filters; paging and sort are ignored */
    query: SessionListQuery;
    onopen: (provider: string, sessionId: string) => void;
  }

  let { query, onopen }: Props = $props();

  let data = $state<SessionInsights | null>(null);
  let loading = $state(true);
  let error = $state('');
  let themeKey = $state('dark');
  let version = 0;
  let disposed = false;

  async function load() {
    const request = ++version;
    loading = true;
    error = '';
    const { search, provider, project, from, to, account } = $state.snapshot(query);
    try {
      const next = await getSessionInsights({ search, provider, project, from, to, ...(account === undefined ? {} : { account }) });
      if (!disposed && request === version) data = next;
    } catch (e) {
      if (!disposed && request === version) { data = null; error = String(e); }
    } finally {
      if (!disposed && request === version) loading = false;
    }
  }

  $effect(() => {
    void [query.search, query.provider, query.project, query.from, query.to, query.account];
    void load();
  });

  onMount(() => {
    const root = document.documentElement;
    const sync = () => (themeKey = root.dataset.theme ?? 'dark');
    sync();
    const observer = new MutationObserver(sync);
    observer.observe(root, { attributes: true, attributeFilter: ['data-theme'] });
    return () => { disposed = true; version++; observer.disconnect(); };
  });

  const k = $derived(data?.kpis ?? null);
  const kpis = $derived(k && data ? [
    { id: 'sessions', label: st('kpiSessions'), value: formatInt(k.sessions), sub: k.pricedSessions < k.sessions ? st('kpiPriced', { n: k.pricedSessions }) : '' },
    { id: 'cost', label: st('kpiMedianCost'), value: k.medianCostUsd == null ? '—' : formatUsdCompact(k.medianCostUsd), sub: '' },
    { id: 'active', label: st('kpiMedianActive'), value: k.medianActiveMs == null ? '—' : formatDuration(Math.round(k.medianActiveMs)), sub: '' },
    { id: 'turns', label: st('kpiMedianTurns'), value: k.medianTurns == null ? '—' : String(Math.round(k.medianTurns * 10) / 10), sub: '' },
    { id: 'failure', label: st('kpiFailureRate'), value: formatRate(k.failureRate), sub: k.toolCalls ? st('kpiOfCalls', { n: formatInt(k.toolFailures), total: formatInt(k.toolCalls) }) : '' },
    { id: 'repeat', label: st('kpiRepeatRate'), value: formatRate(k.repeatRate), sub: k.toolCalls ? st('kpiOfCalls', { n: formatInt(k.repeatedToolCalls), total: formatInt(k.toolCalls) }) : '' },
  ] : []);

  const cost = (r: InsightSession) => (r.costUsd == null ? '—' : formatUsdCompact(r.costUsd));
  const active = (r: InsightSession) => (r.activeDurationMs == null ? '—' : formatDuration(r.activeDurationMs));
  const toolFacts = (r: InsightSession) => `${st('tools')}: ${r.toolCalls} · ${st('failures')}: ${r.toolFailures} · ${st('repeated')}: ${r.repeatedToolCalls}`;
  const maxToolCalls = $derived(Math.max(1, ...(data?.tools.map((t) => t.calls) ?? [1])));
</script>

<div class="insights" aria-busy={loading}>
  <p class="muted small note">{st('insightsNote')}</p>
  {#if error}
    <p class="error" role="alert">{error} <button class="btn" onclick={() => void load()}>{st('retry')}</button></p>
  {/if}
  {#if !data}
    {#if loading}<p class="empty">{st('loading')}</p>{/if}
  {:else if data.kpis.sessions === 0}
    <p class="empty">{st('insightsEmpty')}</p>
  {:else}
    {#if data.truncated}<p class="callout" role="status">{st('insightsCapped', { shown: formatInt(data.kpis.sessions), total: formatInt(data.totalSessions) })}</p>{/if}

    <ul class="kpis" aria-label={st('insights')}>
      {#each kpis as kpi (kpi.id)}
        <li class="tile card"><span class="name">{kpi.label}</span><strong class="value">{kpi.value}</strong><span class="muted small">{kpi.sub}</span></li>
      {/each}
    </ul>

    <div class="dists card">
      <InsightHistogram title={st('costDist')} histogram={data.costHistogram} format={formatUsdCompact} {themeKey} />
      <InsightHistogram title={st('durationDist')} histogram={data.durationHistogram} format={(v) => formatDuration(Math.round(v))} {themeKey} />
      <p class="muted small span">{st('distNote')}</p>
    </div>

    <div class="card panel"><InsightScatter points={data.points} {themeKey} {onopen} /></div>

    <div class="tops">
      <InsightTopList title={st('topCost')} rows={data.topCost} value={cost} detail={(r) => `${st('turns')}: ${r.userTurns}`} thresholds={data.thresholds} {onopen} />
      <InsightTopList title={st('topDuration')} rows={data.topDuration} value={active} detail={(r) => `${st('turns')}: ${r.userTurns}`} thresholds={data.thresholds} {onopen} />
      <InsightTopList title={st('topFailures')} rows={data.topFailures} value={(r) => formatRate(r.failureRate)} detail={toolFacts} thresholds={data.thresholds} explain="failures" {onopen} />
      <InsightTopList title={st('topRepeats')} rows={data.topRepeats} value={(r) => String(r.repeatedToolCalls)} detail={toolFacts} thresholds={data.thresholds} explain="repeats" {onopen} />
    </div>

    <section class="card panel" aria-label={st('toolsTitle')}>
      <h4>{st('toolsTitle')}</h4>
      {#if data.tools.length === 0}
        <p class="muted small">{st('toolsNone')}</p>
      {:else}
        <div class="table-wrap">
          <table>
            <thead><tr><th scope="col">{st('toolName')}</th><th scope="col" class="num">{st('toolCalls')}</th><th scope="col" class="num">{st('toolFailRate')}</th><th scope="col" class="num">{st('toolSessions')}</th></tr></thead>
            <tbody>
              {#each data.tools as tool (tool.tool)}
                <tr>
                  <th scope="row"><span class="tool-name" title={tool.tool}>{tool.tool}</span><span class="bar" aria-hidden="true"><span style:width={`${(tool.calls / maxToolCalls) * 100}%`}></span></span></th>
                  <td class="num mono">{formatInt(tool.calls)}</td>
                  <td class="num mono" class:bad={tool.failureRate >= data.thresholds.failureRate}>{formatRate(tool.failureRate)} <span class="muted">({formatInt(tool.failures)})</span></td>
                  <td class="num mono">{formatInt(tool.sessions)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </section>
  {/if}
</div>

<style>
  .insights { display: grid; gap: 1rem; min-width: 0; }
  p, h4 { margin: 0; }
  h4 { font-size: 0.85rem; }
  .small { font-size: 0.72rem; line-height: 1.6; }
  .error { color: var(--critical); font-size: 0.8rem; overflow-wrap: anywhere; }
  .empty { padding: 2rem 1rem; text-align: center; color: var(--muted); font-size: 0.85rem; }
  .callout { padding: 0.75rem; border-left: 2px solid var(--focus); background: var(--surface-2); font-size: 0.8rem; }
  .kpis { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fit, minmax(10rem, 1fr)); gap: 0.75rem; }
  .tile { padding: 0.8rem 0.9rem; display: grid; gap: 0.25rem; align-content: start; }
  .tile .name { color: var(--muted); font-size: 0.72rem; }
  .tile .value { font-size: 1.4rem; letter-spacing: -0.03em; font-variant-numeric: tabular-nums; }
  .dists { padding: 1rem; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 1.25rem; }
  .span { grid-column: 1 / -1; }
  .panel { padding: 1rem; display: grid; gap: 0.6rem; min-width: 0; }
  .tops { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0.75rem; align-items: start; }
  .table-wrap { overflow-x: auto; }
  table { width: 100%; border-collapse: collapse; font-size: 0.8rem; }
  th, td { padding: 0.35rem 0.5rem; border-bottom: 1px solid var(--border); text-align: left; font-weight: 400; }
  thead th { color: var(--muted); font-weight: 500; }
  .num { text-align: right; white-space: nowrap; }
  tbody th { display: grid; gap: 0.25rem; min-width: 8rem; }
  .tool-name { overflow-wrap: anywhere; }
  .bar { display: block; height: 3px; background: var(--surface-2); border-radius: 2px; }
  .bar span { display: block; height: 100%; background: var(--focus); border-radius: 2px; }
  .bad { color: var(--warn); }
  @media (max-width: 900px) { .dists, .tops { grid-template-columns: 1fr; } }
</style>
