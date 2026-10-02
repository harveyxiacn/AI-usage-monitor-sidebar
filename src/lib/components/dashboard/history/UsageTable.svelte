<!--
  History → Usage: the bucket table with sorting, paging, CSV copy and export.
  Its "Sessions" view is only a lightweight summary of the same range; the full
  session table, search and efficiency insights live in the Sessions tab, which
  it deep-links to with the same range / provider / project. [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { exportUsageCsv, getUsageSessions } from '$lib/api';
  import { formatBucket, formatEstimatedCost, formatInt, formatTokens } from '$lib/format';
  import { historyCsv, localDateInput, modelVariantLabel, sessionsCsv, type HistoryRange } from '$lib/history';
  import { requestDashboardTab } from '$lib/dashboard-nav';
  import { sessionFilters } from '$lib/session-insights';
  import { sessionViewState } from '$lib/session-ui-state';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { st } from '$lib/session-labels.svelte';
  import type { Bucket, HistoryRow, ProviderId, SessionRow, SessionsResult, TokenTotals } from '$lib/types';
  import Segmented from './Segmented.svelte';

  interface Props {
    range: HistoryRange | null;
    provider: ProviderId | '';
    project: string | null;
    rows: HistoryRow[];
    /** the bucket query has no result yet */
    loading: boolean;
    /** a result (possibly empty) has arrived */
    hasResult: boolean;
    bucket: Bucket;
    groupByModel: boolean;
    showProject: boolean;
    /** which table is shown; kept by the page so the choice survives tab switches */
    tableView: 'buckets' | 'sessions';
    providerName: (id: ProviderId) => string;
    projectLabel: (path: string) => string;
    onclearproject: () => void;
  }

  let { range, provider, project, rows, loading, hasResult, bucket, groupByModel, showProject, tableView = $bindable(), providerName, projectLabel, onclearproject }: Props = $props();

  let tablePage = $state(0);
  const TABLE_PAGE_SIZE = 100;

  let sessions = $state<SessionsResult | null>(null);
  let sessionsLoading = $state(false);
  let sessionsError = $state<string | null>(null);
  let sessionsId = 0;
  let sessionsFilterKey = '';

  let copied = $state(false);
  let exported = $state<string | null>(null);
  let exporting = $state(false);
  let actionError = $state<string | null>(null);
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;

  let sortKey = $state<'bucketStart' | 'provider' | 'model' | 'project' | keyof TokenTotals>('bucketStart');
  let sortDir = $state<1 | -1>(-1);

  // keep the sort on a column that is still on screen
  $effect(() => {
    if ((sortKey === 'model' && !groupByModel) || (sortKey === 'project' && !showProject)) sortKey = 'bucketStart';
  });

  // a new result invalidates what "copied" / "saved" referred to
  $effect(() => {
    void rows;
    copied = false;
    exported = null;
  });

  async function loadSessions() {
    const id = ++sessionsId;
    const activeRange = range;
    if (!activeRange || tableView !== 'sessions') {
      sessions = null;
      sessionsLoading = false;
      return;
    }
    const filterKey = JSON.stringify([activeRange.from, provider, project]);
    if (filterKey !== sessionsFilterKey) sessions = null;
    sessionsFilterKey = filterKey;
    sessionsLoading = true;
    sessionsError = null;
    try {
      const next = await getUsageSessions({
        from: new Date(activeRange.from).toISOString(),
        to: new Date(activeRange.to).toISOString(),
        provider: provider === '' ? null : provider,
        project,
      });
      if (id === sessionsId && !disposed) sessions = next;
    } catch (e) {
      if (id === sessionsId && !disposed) {
        sessions = null;
        sessionsError = String(e);
      }
    } finally {
      if (id === sessionsId && !disposed) sessionsLoading = false;
    }
  }

  $effect(() => {
    void [range, provider, project, tableView];
    sessionsId++;
    const timer = setTimeout(() => void loadSessions(), 80);
    return () => clearTimeout(timer);
  });

  onMount(() => () => {
    disposed = true;
    sessionsId++;
    clearTimeout(copiedTimer);
  });

  /** Sorted copy for the table; the chart always uses chronological order. */
  const sortedRows = $derived.by(() => {
    const list = [...rows];
    const key = sortKey;
    const dir = sortDir;
    list.sort((a, b) => {
      let cmp: number;
      if (key === 'bucketStart') cmp = Date.parse(a.bucketStart) - Date.parse(b.bucketStart);
      else if (key === 'provider') cmp = a.provider.localeCompare(b.provider);
      else if (key === 'model') cmp = (a.model ?? '').localeCompare(b.model ?? '') || (a.reasoningEffort ?? '').localeCompare(b.reasoningEffort ?? '');
      else if (key === 'project') cmp = (a.project ?? '').localeCompare(b.project ?? '');
      else if (key === 'estimatedCostUsd') cmp = (a.estimatedCostUsd ?? a.knownCostUsd ?? 0) - (b.estimatedCostUsd ?? b.knownCostUsd ?? 0);
      else cmp = (a[key] ?? 0) - (b[key] ?? 0);
      return cmp * dir || Date.parse(a.bucketStart) - Date.parse(b.bucketStart);
    });
    return list;
  });

  function sortBy(key: typeof sortKey) {
    if (sortKey === key) sortDir = sortDir === 1 ? -1 : 1;
    else {
      sortKey = key;
      sortDir = key === 'provider' || key === 'model' || key === 'project' ? 1 : -1;
    }
  }

  /** The server already orders sessions by tokens; the summary shows the head of that list. */
  const SUMMARY_ROWS = 5;
  const topSessions = $derived((sessions?.rows ?? []).slice(0, SUMMARY_ROWS));
  const tableCount = $derived(tableView === 'sessions' ? 0 : sortedRows.length);
  $effect(() => { void [range, provider, project, bucket, tableView, sortKey, sortDir]; tablePage = 0; });
  $effect(() => { if (tablePage * TABLE_PAGE_SIZE >= tableCount) tablePage = Math.max(0, Math.ceil(tableCount / TABLE_PAGE_SIZE) - 1); });

  const sessionLabel = (row: SessionRow) => row.sessionId || t('history.sessions.unassigned');
  const modelLabel = (model: string | null, effort?: string | null) =>
    modelVariantLabel(model, effort, t('history.modelUnknown'), t('history.effortUnknown'));

  /** Hand the current History filters to the Sessions tab, optionally opening one session. */
  function openSessions(view: 'browse' | 'insights', target?: SessionRow) {
    sessionViewState.query = sessionFilters(range, provider, project);
    sessionViewState.view = view;
    if (target && target.sessionId) sessionViewState.selected = { provider: target.provider, sessionId: target.sessionId };
    requestDashboardTab('sessions');
  }

  /** The active table view decides what "copy" and "export" produce. */
  const csvText = () => (tableView === 'sessions' ? sessionsCsv(sessions?.rows ?? []) : historyCsv(sortedRows));
  const csvEmpty = $derived(tableView === 'sessions' ? (sessions?.rows.length ?? 0) === 0 : sortedRows.length === 0);
  const csvBusy = $derived(tableView === 'sessions' ? sessionsLoading : loading);

  async function copyCsv() {
    const csv = csvText();
    actionError = null;
    copied = false;
    let success = false;
    try {
      await navigator.clipboard.writeText(csv);
      success = true;
    } catch {
      // clipboard API is unavailable in some WebKitGTK builds — fall back to
      // a hidden textarea + execCommand, which still works there.
      const ta = document.createElement('textarea');
      const focused = document.activeElement;
      ta.value = csv;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      try {
        success = document.execCommand('copy');
      } catch {
        /* nothing else we can do */
      }
      ta.remove();
      if (focused instanceof HTMLElement) focused.focus();
    }
    if (success) {
      copied = true;
      clearTimeout(copiedTimer);
      copiedTimer = setTimeout(() => (copied = false), 1500);
    } else actionError = t('history.copyFailed');
  }

  async function exportCsv() {
    if (!range || exporting) return;
    exporting = true;
    actionError = null;
    exported = null;
    const kind = tableView === 'sessions' ? 'sessions-' : '';
    try {
      exported = await exportUsageCsv(csvText(), `ai-usage-${kind}${localDateInput(range.from)}-${localDateInput(range.to - 1)}.csv`);
    } catch (e) {
      actionError = String(e);
    } finally {
      exporting = false;
    }
  }

  const tableViewOptions = $derived([['buckets', t('history.view.buckets')], ['sessions', t('history.view.sessions')]] as const);
</script>

<div class="card panel">
  <header class="panel-head">
    <h3>{tableView === 'sessions' ? t('history.sessions.title') : t('history.table.title')}</h3>
    <div class="head-controls">
      <Segmented options={tableViewOptions} value={tableView} label={t('history.view')} onchange={(v) => (tableView = v)} />
      <div class="export-actions">
        <button class="btn" onclick={() => void copyCsv()} disabled={csvBusy || csvEmpty}>
          {copied ? t('common.copied') : t('common.copy')}
        </button>
        <button class="btn" onclick={() => void exportCsv()} disabled={csvBusy || exporting || csvEmpty}>
          {exporting ? t('common.saving') : t('history.exportCsv')}
        </button>
      </div>
    </div>
  </header>

  {#if tableView === 'sessions'}
    {#if sessionsError}
      <div class="feedback" role="alert">
        <p class="err">{t('common.error', { message: sessionsError })}</p>
        <button class="btn" onclick={() => void loadSessions()}>{t('common.retry')}</button>
      </div>
    {:else if sessionsLoading && !sessions}
      <p class="muted">{t('common.loading')}</p>
    {:else if sessions && sessions.rows.length === 0}
      <p class="muted">{t('history.sessions.none')}</p>
    {:else if sessions}
      <p class="muted small">
        {sessions.truncated
          ? t('history.sessions.capped', { shown: formatInt(sessions.rows.length), total: formatInt(sessions.totalSessions) })
          : t('history.sessions.count', { n: formatInt(sessions.totalSessions) })}
      </p>
      <dl class="totals" aria-label={t('history.sessions.summary')}>
        <div><dt>{t('history.sessions.title')}</dt><dd class="mono">{formatInt(sessions.totalSessions)}</dd></div>
        <div><dt>{t('history.table.total')}</dt><dd class="mono">{formatTokens(sessions.totals.totalTokens)}</dd></div>
        <div><dt>{t('history.estCost')}</dt><dd class="mono">{formatEstimatedCost(sessions.totals)}</dd></div>
      </dl>
      <h4>{t('history.sessions.largest')}</h4>
      <ol class="top-sessions">
        {#each topSessions as s (JSON.stringify([s.provider, s.sessionId]))}
          <li>
            <button class="link" disabled={!s.sessionId} onclick={() => openSessions('browse', s)}>
              <span class="session-id" title={s.sessionId}>{sessionLabel(s)}</span>
              <span class="muted">{providerName(s.provider)} · {projectLabel(s.project)}</span>
              <span class="mono num">{formatTokens(s.totalTokens)} · {formatEstimatedCost(s)}</span>
            </button>
          </li>
        {/each}
      </ol>
      <p class="muted small">{t('history.sessions.linkNote')}</p>
      <div class="export-actions">
        <button class="btn btn-primary" onclick={() => openSessions('browse')}>{t('history.sessions.open')}</button>
        <button class="btn" onclick={() => openSessions('insights')}>{t('history.sessions.insights')}</button>
      </div>
    {/if}
  {:else if loading && !hasResult}
    <p class="muted">{t('common.loading')}</p>
  {:else if hasResult && sortedRows.length === 0}
    <p class="muted">{t('history.noRows')}</p>
    {#if project !== null}<button class="btn clear-project" onclick={onclearproject}>{t('history.project.clear')}</button>{/if}
  {:else if sortedRows.length > 0}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            {#each [['bucketStart', 'history.table.bucket'], ['provider', 'history.table.provider'], ...(groupByModel ? [['model', 'history.table.model']] : []), ...(showProject ? [['project', 'history.project']] : []), ['inputTokens', 'history.input'], ['cacheReadTokens', 'history.cacheRead'], ['cacheWriteTokens', 'history.cacheWrite'], ['outputTokens', 'history.output'], ['requests', 'history.requests'], ['totalTokens', 'history.table.total'], ['estimatedCostUsd', 'history.estCost']] as [key, label] (key)}
              <th
                class:num={key !== 'bucketStart' && key !== 'provider' && key !== 'model' && key !== 'project'}
                aria-sort={sortKey === key ? (sortDir === 1 ? 'ascending' : 'descending') : 'none'}
              >
                <button onclick={() => sortBy(key as typeof sortKey)}>
                  {tDyn(label)}
                  {#if sortKey === key}<span class="caret">{sortDir === 1 ? '▲' : '▼'}</span>{/if}
                </button>
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each sortedRows.slice(tablePage * TABLE_PAGE_SIZE, (tablePage + 1) * TABLE_PAGE_SIZE) as r (JSON.stringify([r.bucketStart, r.provider, r.model, r.reasoningEffort, r.project]))}
            <tr>
              <td>{formatBucket(r.bucketStart, bucket)}</td>
              <td>{providerName(r.provider)}</td>
              {#if groupByModel}<td class="model" title={modelLabel(r.model, r.reasoningEffort)}>{modelLabel(r.model, r.reasoningEffort)}</td>{/if}
              {#if showProject}<td class="project-name" title={r.project || t('history.project.unassigned')}>{projectLabel(r.project ?? '')}</td>{/if}
              <td class="num mono">{formatTokens(r.inputTokens)}</td>
              <td class="num mono">{formatTokens(r.cacheReadTokens)}</td>
              <td class="num mono">{formatTokens(r.cacheWriteTokens)}</td>
              <td class="num mono">{formatTokens(r.outputTokens)}</td>
              <td class="num mono">{formatInt(r.requests)}</td>
              <td class="num mono strong">{formatTokens(r.totalTokens)}</td>
              <td class="num mono">{formatEstimatedCost(r)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

{#if tableCount > TABLE_PAGE_SIZE}
  <nav class="table-pager" aria-label={t('history.table.title')}>
    <button class="btn" disabled={tablePage === 0} onclick={() => tablePage--}>{st('previous')}</button>
    <span class="muted">{tablePage * TABLE_PAGE_SIZE + 1}–{Math.min(tableCount, (tablePage + 1) * TABLE_PAGE_SIZE)} / {tableCount}</span>
    <button class="btn" disabled={(tablePage + 1) * TABLE_PAGE_SIZE >= tableCount} onclick={() => tablePage++}>{st('next')}</button>
  </nav>
{/if}

{#if exported}
  <p class="muted export-path" role="status">{t('history.exported', { path: exported })}</p>
{/if}
{#if actionError}
  <p class="err" role="alert">{t('common.error', { message: actionError })}</p>
{/if}

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    padding: 1rem;
    min-width: 0;
  }

  .panel-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 1rem;
    flex-wrap: wrap;
  }

  h3 {
    margin: 0;
    font-size: 0.875rem;
    font-weight: 600;
  }

  .head-controls {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  .small {
    font-size: 0.75rem;
  }

  .feedback,
  .export-actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  .export-path {
    overflow-wrap: anywhere;
    margin: 0;
  }

  .table-pager {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.75rem;
    font-size: 0.8rem;
  }

  .clear-project {
    align-self: flex-start;
  }

  h4 {
    margin: 0;
    font-size: 0.8125rem;
    font-weight: 600;
  }

  .totals {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem 2rem;
    margin: 0;
  }

  .totals dt {
    color: var(--muted);
    font-size: 0.75rem;
  }

  .totals dd {
    margin: 0;
    font-size: 1.1rem;
    font-weight: 600;
  }

  .top-sessions {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .top-sessions li + li {
    border-top: 1px solid var(--border);
  }

  .top-sessions .link {
    width: 100%;
    display: grid;
    grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr) auto;
    gap: 0.75rem;
    align-items: baseline;
    padding: 0.4rem 0.25rem;
    text-align: left;
    color: inherit;
    text-decoration: none;
  }

  .top-sessions .link:hover:not(:disabled) {
    background: var(--hover);
  }

  .top-sessions .muted {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.75rem;
  }

  .project-name {
    max-width: 18rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .session-id {
    max-width: 14rem;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: var(--font-mono);
    font-size: 0.75rem;
  }

  .table-wrap {
    /* the only element allowed to scroll sideways on a narrow window */
    overflow-x: auto;
    max-height: 26rem;
    overflow-y: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.8125rem;
    white-space: nowrap;
  }

  th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--surface);
    text-align: left;
    font-weight: 500;
    color: var(--muted);
    border-bottom: 1px solid var(--border);
    padding: 0;
  }

  th button {
    width: 100%;
    padding: 0.375rem 0.5rem;
    text-align: inherit;
    color: inherit;
    font: inherit;
  }

  th.num button {
    text-align: right;
  }

  th button:hover {
    color: var(--text);
  }

  .caret {
    font-size: 0.625rem;
    margin-left: 0.25rem;
  }

  td {
    padding: 0.3125rem 0.5rem;
    border-bottom: 1px solid var(--border);
  }

  tbody tr:hover td {
    background: var(--hover);
  }

  .num {
    text-align: right;
  }

  .strong {
    font-weight: 600;
  }

  .model {
    min-width: 12rem;
    max-width: 22rem;
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .err {
    margin: 0;
    color: var(--critical);
    font-size: 0.8125rem;
  }
</style>
