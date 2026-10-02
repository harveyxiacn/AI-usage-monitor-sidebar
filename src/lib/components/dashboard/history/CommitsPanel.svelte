<!--
  History → Commits: for the project chosen in the filter bar, each commit with
  the tokens, estimated cost and sessions attributed to it (usage between the
  previous commit and this one, at most 6 h back). Opt-in (`gitAttribution`):
  while it is off nothing is run; the panel only offers to turn it on. The
  heuristic and its limits are in src-tauri/src/advisor/git.rs. [FRONTEND]
-->
<script lang="ts">
  import { onDestroy } from 'svelte';
  import { getProjectCommits } from '$lib/api';
  import { formatEstimatedCost, formatInt, formatTokens } from '$lib/format';
  import type { HistoryRange } from '$lib/history';
  import { intlLocale, t, tDyn } from '$lib/i18n/i18n.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import type { CommitsResult, ProviderId } from '$lib/types';

  interface Props {
    range: HistoryRange | null;
    provider: ProviderId | '';
    /** exact cwd chosen in the History filter; null = none picked */
    project: string | null;
    projectLabel: (path: string) => string;
    /** bumps when new usage was ingested */
    refreshKey: number;
  }
  let { range, provider, project, projectLabel, refreshKey }: Props = $props();

  let result = $state<CommitsResult | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let requestId = 0;
  let disposed = false;

  const enabled = $derived(settings.value.gitAttribution);

  async function load(refresh = false) {
    const id = ++requestId;
    const active = range;
    if (!project || !active || !enabled) {
      result = null;
      loading = false;
      return;
    }
    loading = true;
    try {
      const next = await getProjectCommits({
        project,
        from: new Date(active.from).toISOString(),
        to: new Date(active.to).toISOString(),
        provider: provider || null,
        refresh,
      });
      if (disposed || id !== requestId) return;
      result = next;
      error = null;
    } catch (e) {
      if (!disposed && id === requestId) { error = String(e); result = null; }
    } finally {
      if (!disposed && id === requestId) loading = false;
    }
  }

  $effect(() => {
    void [project, range?.from, range?.to, provider, enabled, refreshKey];
    const timer = setTimeout(() => void load(), 150);
    return () => clearTimeout(timer);
  });
  onDestroy(() => { disposed = true; requestId++; });

  const stamp = (iso: string) =>
    new Date(iso).toLocaleString(intlLocale(), { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' });

  const summary = $derived.by(() => {
    if (!result || result.status !== 'ok' || result.commits.length === 0) return null;
    const tokens = result.commits.reduce((sum, c) => sum + c.totalTokens, 0);
    const costs = result.commits.map((c) => c.estimatedCostUsd);
    const cost = costs.every((c) => c !== null) ? costs.reduce<number>((sum, c) => sum + (c ?? 0), 0) : null;
    return { n: result.commits.length, tokens, cost };
  });
  const cost = (value: number | null) => (value === null ? '—' : `≈ $${value.toFixed(2)}`);
</script>

<section class="card commits" aria-label={t('commits.title')}>
  <header>
    <h3>{t('commits.title')}</h3>
    {#if enabled && project}
      <button class="btn" disabled={loading} onclick={() => void load(true)}>{t('commits.refresh')}</button>
    {/if}
  </header>

  {#if !enabled}
    <div class="off" role="status">
      <p><strong>{t('commits.disabled.title')}</strong></p>
      <p class="muted">{t('commits.disabled.detail')}</p>
      <button class="btn" onclick={() => void settings.patch({ gitAttribution: true })}>{t('commits.enable')}</button>
    </div>
  {:else if !project}
    <p class="muted" role="status">{t('commits.pick')}</p>
  {:else}
    <p class="project muted" title={project}>{projectLabel(project)}</p>
    {#if error}
      <p class="error" role="alert">{error}</p>
    {:else if result === null}
      <p class="muted" role="status">{t('common.loading')}</p>
    {:else if result.status !== 'ok'}
      <p class="muted" role="status">{tDyn(`commits.status.${result.status}`, { message: result.message ?? '' })}</p>
    {:else}
      {#if summary}
        <p class="summary" role="status">
          {t('commits.summary', { n: formatInt(summary.n), tokens: formatTokens(summary.tokens), cost: cost(summary.cost) })}
          {#if result.cached}<span class="tag">{t('commits.cached')}</span>{/if}
        </p>
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th>{t('commits.col.time')}</th>
                <th>{t('commits.col.commit')}</th>
                <th class="num">{t('commits.col.tokens')}</th>
                <th class="num">{t('commits.col.cost')}</th>
                <th class="num">{t('commits.col.sessions')}</th>
              </tr>
            </thead>
            <tbody>
              {#each result.commits as c (c.hash)}
                <tr>
                  <td class="time" title={t('commits.window', { since: stamp(c.windowStart) })}>{stamp(c.ts)}</td>
                  <td class="subject"><code>{c.shortHash}</code> {c.subject}</td>
                  <td class="num">{c.totalTokens > 0 ? formatTokens(c.totalTokens) : '—'}</td>
                  <td class="num">{c.requests > 0 ? formatEstimatedCost(c) : '—'}</td>
                  <td class="num">{c.sessions > 0 ? formatInt(c.sessions) : '—'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <p class="muted" role="status">{t('commits.empty')}</p>
      {/if}
      {#if result.unattributed.totalTokens > 0}
        <p class="muted">{t('commits.unattributed', { tokens: formatTokens(result.unattributed.totalTokens), cost: formatEstimatedCost(result.unattributed) })}</p>
      {/if}
      {#if result.truncated}<p class="muted">{t('commits.truncated', { n: 500 })}</p>{/if}
    {/if}
  {/if}
  <p class="muted note">{t('commits.note')}</p>
</section>

<style>
  .commits { display: flex; flex-direction: column; gap: 0.75rem; padding: 1rem; min-width: 0; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 0.75rem; }
  h3, p { margin: 0; }
  h3 { font-size: 0.9375rem; }
  .off { display: flex; flex-direction: column; align-items: flex-start; gap: 0.5rem; }
  .muted { color: var(--muted); font-size: 0.75rem; line-height: 1.6; }
  .project { overflow-wrap: anywhere; }
  .summary { font-size: 0.8125rem; display: flex; gap: 0.5rem; align-items: baseline; flex-wrap: wrap; }
  .tag { font-size: 0.6875rem; color: var(--muted); border: 1px solid var(--border); border-radius: 999px; padding: 0 0.375rem; }
  .table-wrap { overflow-x: auto; max-height: 28rem; }
  table { border-collapse: collapse; width: 100%; font-size: 0.75rem; }
  th, td { padding: 0.4rem 0.625rem; text-align: left; border-bottom: 1px solid var(--border); vertical-align: top; }
  th { font-weight: 500; color: var(--muted); white-space: nowrap; }
  .num { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .time { white-space: nowrap; color: var(--muted); }
  .subject { overflow-wrap: anywhere; min-width: 12rem; }
  code { font-size: 0.6875rem; color: var(--muted); margin-right: 0.375rem; }
  .error { color: var(--danger, #ff6b6b); font-size: 0.8125rem; }
</style>
