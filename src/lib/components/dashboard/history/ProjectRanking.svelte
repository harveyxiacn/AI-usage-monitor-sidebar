<!--
  Project ranking: the ten projects (exact cwd) with the most tokens in the
  range, as horizontal bars with cost, cache hit rate and session count.
  Clicking a row filters the whole History view to that project. [FRONTEND]

  Plain DOM instead of a canvas: each row is a real button with a text label,
  so it is keyboard- and screen-reader-operable and the numbers are never
  encoded in bar length alone. The ranking ignores the project filter (it is
  the list you choose from) but follows the range and provider.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { getUsageHistory, getUsageSessions } from '$lib/api';
  import { countSessionsByProject, rankProjects, sharePercent, type ProjectRank } from '$lib/analytics';
  import { HEAT_ACCENT } from '$lib/colors';
  import { formatEstimatedCost, formatInt, formatTokens } from '$lib/format';
  import type { HistoryRange } from '$lib/history';
  import { t } from '$lib/i18n/i18n.svelte';
  import type { ProviderId } from '$lib/types';

  interface Props {
    range: HistoryRange | null;
    provider: ProviderId | '';
    /** the active project filter, highlighted in the list */
    selected: string | null;
    dataVersion: number;
    themeKey: string;
    projectLabel: (path: string) => string;
    onpick: (project: string) => void;
  }

  let { range, provider, selected, dataVersion, themeKey, projectLabel, onpick }: Props = $props();

  const LIMIT = 10;
  /** the session query's own server-side maximum */
  const SESSION_LIMIT = 1000;

  let ranks = $state<ProjectRank[] | null>(null);
  let projectCount = $state(0);
  let sessionsTruncated = $state(false);
  let error = $state<string | null>(null);
  let requestId = 0;
  let disposed = false;
  let filterKey = '';

  async function load() {
    const id = ++requestId;
    const active = range;
    if (!active) {
      ranks = null;
      return;
    }
    const key = JSON.stringify([active.from, provider]);
    if (key !== filterKey) ranks = null;
    filterKey = key;
    error = null;
    const from = new Date(active.from).toISOString();
    const to = new Date(active.to).toISOString();
    const providerQuery = provider === '' ? null : provider;
    try {
      const [history, sessions] = await Promise.all([
        // month buckets: one row per project and month, summed below
        getUsageHistory({ from, to, bucket: 'month', groupByModel: false, groupByProject: true, project: null, provider: providerQuery }),
        getUsageSessions({ from, to, provider: providerQuery, project: null, limit: SESSION_LIMIT }).catch(() => null),
      ]);
      if (id !== requestId || disposed) return;
      const counts = countSessionsByProject(sessions?.rows ?? []);
      projectCount = new Set(history.rows.map((row) => row.project ?? '')).size;
      sessionsTruncated = sessions?.truncated ?? false;
      ranks = rankProjects(history.rows, counts, LIMIT);
    } catch (e) {
      if (id === requestId && !disposed) error = String(e);
    }
  }

  $effect(() => {
    void [range, provider, dataVersion];
    requestId++;
    const timer = setTimeout(() => void load(), 80);
    return () => clearTimeout(timer);
  });

  onMount(() => () => {
    disposed = true;
    requestId++;
  });

  const top = $derived(Math.max(1, ...(ranks ?? []).map((r) => r.totals.totalTokens)));
  const barColor = $derived(HEAT_ACCENT[themeKey === 'light' ? 'light' : 'dark']);
  const hit = (rank: ProjectRank) => (rank.hitRate == null ? '—' : `${Math.round(rank.hitRate * 100)}%`);
  const sessionText = (rank: ProjectRank) => `${sessionsTruncated ? '≥' : ''}${formatInt(rank.sessions)}`;
</script>

{#if error}
  <p class="err" role="alert">{t('common.error', { message: error })}</p>
{:else if ranks === null}
  <p class="muted" role="status">{t('common.loading')}</p>
{:else if ranks.length === 0}
  <p class="muted">{t('chart.noData')}</p>
{:else}
  <div class="row head" aria-hidden="true">
    <span></span>
    <span>{t('history.project')}</span>
    <span></span>
    <span class="num">{t('history.metric.tokens')}</span>
    <span class="num cost">{t('history.estCost')}</span>
    <span class="num">{t('history.kpi.cacheHitShort')}</span>
    <span class="num sess">{t('history.projects.sessions')}</span>
  </div>
  <ol class="rank">
    {#each ranks as rank, i (rank.project)}
      {@const active = selected === rank.project}
      <li>
        <button
          class="row"
          class:active
          aria-pressed={active}
          title={rank.project || t('history.project.unassigned')}
          aria-label={t('history.projects.rowLabel', {
            name: projectLabel(rank.project),
            tokens: formatTokens(rank.totals.totalTokens),
            cost: formatEstimatedCost(rank.totals),
            hit: hit(rank),
            sessions: sessionText(rank),
          })}
          onclick={() => onpick(rank.project)}
        >
          <span class="pos" aria-hidden="true">{i + 1}</span>
          <span class="name">{projectLabel(rank.project)}</span>
          <span class="bar" aria-hidden="true">
            <span class="fill" style:width={`${Math.max(1.5, sharePercent(rank.totals.totalTokens, top))}%`} style:background={barColor}></span>
          </span>
          <span class="num strong">{formatTokens(rank.totals.totalTokens)}</span>
          <span class="num cost">{formatEstimatedCost(rank.totals)}</span>
          <span class="num">{hit(rank)}</span>
          <span class="num sess">{sessionText(rank)}</span>
        </button>
      </li>
    {/each}
  </ol>
  <p class="note">
    {t('history.projects.header')} · {projectCount > LIMIT ? t('history.projects.topOf', { shown: LIMIT, total: projectCount }) : t('history.projects.all', { n: projectCount })}
    {#if sessionsTruncated} · {t('history.projects.sessionsCapped')}{/if}
  </p>
{/if}

<style>
  .rank {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: grid;
    /* rank · name · bar · tokens · cost · hit rate · sessions */
    grid-template-columns: 1.25rem minmax(7rem, 14rem) minmax(4rem, 1fr) 4.5rem 7.5rem 3.25rem 3.25rem;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.3125rem 0.5rem;
    border-radius: var(--r-control);
    border: 1px solid transparent;
    font-size: 0.8125rem;
    text-align: left;
    color: var(--text);
  }

  .head {
    color: var(--muted);
    font-size: 0.6875rem;
    padding-block: 0;
  }

  .row.head:hover {
    background: none;
  }

  .row:hover {
    background: var(--hover);
  }

  /* the selected row is outlined as well as tinted: not a colour-only cue */
  .row.active {
    border-color: var(--focus);
    background: var(--hover);
  }

  .row:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 1px;
  }

  .pos {
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bar {
    height: 0.625rem;
    border-radius: 0.3125rem;
    background: var(--surface-2);
    overflow: hidden;
  }

  .fill {
    display: block;
    height: 100%;
    border-radius: 0.3125rem;
  }

  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .strong {
    font-weight: 600;
  }

  @media (max-width: 760px) {
    .row {
      grid-template-columns: 1.25rem minmax(0, 1fr) 4.5rem 3.25rem;
    }

    .bar,
    .cost,
    .sess {
      display: none;
    }
  }

  .note {
    margin: 0.375rem 0 0;
    font-size: 0.6875rem;
    color: var(--faint);
  }

  .muted {
    margin: 0;
  }

  .err {
    margin: 0;
    color: var(--critical);
    font-size: 0.8125rem;
  }
</style>
