<!--
  Dashboard → History. Token usage recorded from the providers' local session
  logs: range/bucket/provider/group controls, a stacked bar chart, per-provider
  comparison cards, a sortable table with CSV export and the ingestion line.
  [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import UsageChart from '$lib/components/UsageChart.svelte';
  import { exportUsageCsv, getUsageHistory, onIngestProgress, reingestLogs, type Unlisten } from '$lib/api';
  import { historyCsv, historyRange, localDateInput, projectLabels, projectName, type HistoryPreset } from '$lib/history';
  import { formatBucket, formatCost, formatInt, formatTokens } from '$lib/format';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { providerDisplayName } from '$lib/providers';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import type {
    Bucket,
    HistoryResult,
    IngestStats,
    ProviderId,
    TokenTotals,
  } from '$lib/types';

  interface Props {
    /** flips whenever the palette changes so the canvas re-renders */
    themeKey: string;
  }

  let { themeKey }: Props = $props();

  const DAY = 86_400_000;
  let preset = $state<HistoryPreset>('7d');
  let customFrom = $state(localDateInput(Date.now() - 6 * DAY));
  let customTo = $state(localDateInput(Date.now()));
  let queryTime = $state(Date.now());
  let bucket = $state<Bucket>('day');
  let provider = $state<ProviderId | ''>('');
  let groupByModel = $state(false);
  let groupByProject = $state(false);
  let project = $state<string | null>(null);
  let projects = $state<string[]>([]);
  let metric = $state<'tokens' | 'cost'>('tokens');

  let result = $state<HistoryResult | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  let ingest = $state<IngestStats | null>(null);
  let rescanning = $state(false);
  let copied = $state(false);
  let exported = $state<string | null>(null);
  let exporting = $state(false);
  let actionError = $state<string | null>(null);
  let requestId = 0;
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;

  let sortKey = $state<'bucketStart' | 'provider' | 'model' | 'project' | keyof TokenTotals>('bucketStart');
  let sortDir = $state<1 | -1>(-1);

  /** [fromMs, toMs) for the active preset; custom uses whole local days. */
  const range = $derived(historyRange(preset, customFrom, customTo, queryTime));

  /** Hour buckets only make sense for short ranges (contract: ≤ 2 days). */
  const hourAllowed = $derived(range !== null && range.to - range.from <= 2 * DAY + 3_600_000);
  const buckets = $derived<Bucket[]>(
    hourAllowed ? ['hour', 'day', 'week', 'month'] : ['day', 'week', 'month']
  );

  // keep the bucket legal when the range shrinks/grows underneath it
  $effect(() => {
    if (!hourAllowed && bucket === 'hour') bucket = 'day';
  });

  // …and keep the sort on a column that is still on screen
  $effect(() => {
    if ((sortKey === 'model' && !groupByModel) || (sortKey === 'project' && !showProject)) sortKey = 'bucketStart';
  });

  function pickPreset(next: HistoryPreset) {
    preset = next;
    queryTime = Date.now();
    // a sensible default granularity per preset; the user can still override
    if (next === 'today') bucket = 'hour';
    else if (next === '90d') bucket = 'week';
    else bucket = 'day';
  }

  async function load() {
    const id = ++requestId;
    const activeRange = range;
    if (!activeRange) {
      result = null;
      error = null;
      loading = false;
      return;
    }
    loading = true;
    error = null;
    result = null;
    copied = false;
    exported = null;
    try {
      const next = await getUsageHistory({
        from: new Date(activeRange.from).toISOString(),
        to: new Date(activeRange.to).toISOString(),
        bucket,
        groupByModel,
        groupByProject,
        project,
        provider: provider === '' ? null : provider,
      });
      if (id === requestId && !disposed) {
        result = next;
        projects = next.projects;
      }
    } catch (e) {
      if (id === requestId && !disposed) error = String(e);
    } finally {
      if (id === requestId && !disposed) loading = false;
    }
  }

  // refetch whenever a query input changes
  $effect(() => {
    void [range, bucket, groupByModel, groupByProject, provider, project];
    void load();
  });

  onMount(() => {
    let un: Unlisten | null = null;
    void onIngestProgress((stats) => {
      ingest = stats;
      // a finished scan may have added events — refresh the view
      if (!stats.running && !rescanning) queryTime = Date.now();
    }).then((u) => (disposed ? u() : (un = u))).catch((e) => { actionError = String(e); });
    const timer = setInterval(() => { if (!document.hidden && !rescanning) queryTime = Date.now(); }, 60_000);
    return () => {
      disposed = true;
      requestId++;
      un?.();
      clearInterval(timer);
      clearTimeout(copiedTimer);
    };
  });

  async function rescan() {
    if (rescanning || ingest?.running) return;
    rescanning = true;
    actionError = null;
    try {
      ingest = await reingestLogs();
    } catch (e) {
      actionError = String(e);
    } finally {
      rescanning = false;
      queryTime = Date.now();
    }
  }

  const rows = $derived(result?.rows ?? []);
  const totals = $derived(result?.totals ?? null);
  // the active filter stays selectable even when the new range no longer lists it
  const projectOptions = $derived(
    [...new Set(project === null ? projects : [...projects, project])].sort((a, b) => a.localeCompare(b))
  );
  const projectNames = $derived(projectLabels(projectOptions, t('history.project.unassigned')));
  const showProject = $derived(groupByProject || project !== null);
  const projectLabel = (value: string) => projectNames.get(value) ?? projectName(value, t('history.project.unassigned'));

  const providerTotals = $derived.by(() => {
    const by = result?.byProvider ?? {};
    return (Object.keys(by) as ProviderId[])
      .sort()
      .map((id) => ({ id, totals: by[id] }));
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
      else if (key === 'model' || key === 'project') cmp = (a[key] ?? '').localeCompare(b[key] ?? '');
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

  async function copyCsv() {
    const csv = historyCsv(sortedRows);
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
    try {
      exported = await exportUsageCsv(historyCsv(sortedRows), `ai-usage-${localDateInput(range.from)}-${localDateInput(range.to - 1)}.csv`);
    } catch (e) {
      actionError = String(e);
    } finally {
      exporting = false;
    }
  }

  const metricValue = (tt: TokenTotals) =>
    metric === 'cost' ? formatCost(tt.estimatedCostUsd) : formatTokens(tt.totalTokens);

  /** Backend display name when the snapshot knows the provider, else a monogram-style label. */
  const providerName = (id: ProviderId) =>
    snapshot.value?.providers.find((p) => p.provider === id)?.displayName ?? providerDisplayName(id);

  /** Filter options: every provider the backend knows or the settings list, in display order. */
  const providerOptions = $derived.by(() => {
    const cfg = settings.value.providers;
    const ids = new Set<ProviderId>((snapshot.value?.providers ?? []).map((p) => p.provider));
    for (const id of Object.keys(cfg)) ids.add(id);
    return [...ids].sort((a, b) => (cfg[a]?.order ?? 0) - (cfg[b]?.order ?? 0));
  });

  const PRESETS: Array<[HistoryPreset, string]> = [
    ['today', 'history.range.today'],
    ['7d', 'history.range.7d'],
    ['30d', 'history.range.30d'],
    ['90d', 'history.range.90d'],
    ['custom', 'history.range.custom'],
  ];
</script>

<section class="history">
  <div class="controls card">
    <div class="group">
      <span class="ctl-label">{t('history.range')}</span>
      <div class="segmented" role="group" aria-label={t('history.range')}>
        {#each PRESETS as [id, key] (id)}
          <button class:active={preset === id} aria-pressed={preset === id} onclick={() => pickPreset(id)}>{tDyn(key)}</button>
        {/each}
      </div>
    </div>

    {#if preset === 'custom'}
      <div class="group">
        <label class="ctl-label" for="from">{t('history.from')}</label>
        <input id="from" class="field" type="date" bind:value={customFrom} max={customTo} aria-invalid={!range} aria-describedby={!range ? 'range-error' : undefined} />
        <label class="ctl-label" for="to">{t('history.to')}</label>
        <input id="to" class="field" type="date" bind:value={customTo} min={customFrom} aria-invalid={!range} aria-describedby={!range ? 'range-error' : undefined} />
      </div>
    {/if}

    <div class="group">
      <label class="ctl-label" for="bucket">{t('history.bucket')}</label>
      <select id="bucket" class="field" bind:value={bucket}>
        {#each buckets as b (b)}
          <option value={b}>{tDyn(`history.bucket.${b}`)}</option>
        {/each}
      </select>

      <label class="ctl-label" for="provider">{t('history.provider')}</label>
      <select id="provider" class="field" bind:value={provider}>
        <option value="">{t('common.all')}</option>
        {#each providerOptions as id (id)}
          <option value={id}>{providerName(id)}</option>
        {/each}
      </select>

      <label class="ctl-label" for="project">{t('history.project')}</label>
      <select id="project" class="field project-select"
        value={project === null ? 'all' : `project:${project}`}
        title={project === null ? t('history.project.all') : project || t('history.project.unassigned')}
        onchange={(e) => (project = e.currentTarget.value === 'all' ? null : e.currentTarget.value.slice('project:'.length))}>
        <option value="all">{t('history.project.all')}</option>
        {#each projectOptions as path (path)}
          <option value={`project:${path}`} title={path || t('history.project.unassigned')}>{projectLabel(path)}</option>
        {/each}
      </select>

      <label class="ctl-label" for="group">{t('history.groupBy')}</label>
      <select
        id="group"
        class="field"
        value={groupByModel ? 'model' : 'provider'}
        onchange={(e) => (groupByModel = e.currentTarget.value === 'model')}
      >
        <option value="provider">{t('history.groupBy.provider')}</option>
        <option value="model">{t('history.groupBy.model')}</option>
      </select>

      <label class="project-group">
        <input type="checkbox" bind:checked={groupByProject} />
        {t('history.groupByProject')}
      </label>

      <div class="segmented" role="group" aria-label={t('history.metric')}>
        <button class:active={metric === 'tokens'} aria-pressed={metric === 'tokens'} onclick={() => (metric = 'tokens')}>
          {t('history.metric.tokens')}
        </button>
        <button class:active={metric === 'cost'} aria-pressed={metric === 'cost'} onclick={() => (metric = 'cost')}>
          {t('history.metric.cost')}
        </button>
      </div>
    </div>
  </div>

  {#if error}
    <div class="feedback" role="alert">
      <p class="err">{t('common.error', { message: error })}</p>
      <button class="btn" onclick={() => void load()}>{t('common.retry')}</button>
    </div>
  {/if}
  {#if !range}
    <p id="range-error" class="err" role="alert">{t('history.invalidRange')}</p>
  {/if}
  {#if actionError}
    <p class="err" role="alert">{t('common.error', { message: actionError })}</p>
  {/if}
  {#if exported}
    <p class="muted export-path" role="status">{t('history.exported', { path: exported })}</p>
  {/if}

  <div class="card panel" aria-busy={loading}>
    <header class="panel-head">
      <h3>{t('history.chartTitle')}</h3>
      {#if totals}
        <span class="muted">
          {t('history.totalTokens')}: {formatTokens(totals.totalTokens)} ·
          {t('history.requests')}: {formatInt(totals.requests)} ·
          {t('history.estCost')}: {formatCost(totals.estimatedCostUsd)}
        </span>
      {/if}
    </header>
    {#if loading}
      <p class="muted" role="status">{t('common.loading')}</p>
    {:else if result}
      <UsageChart {rows} {bucket} {groupByModel} {groupByProject} {projectNames} {metric} {themeKey} />
    {/if}
  </div>

  {#if providerTotals.length > 0}
    <div class="compare">
      {#each providerTotals as p (p.id)}
        <article class="card cmp">
          <header class="cmp-head">
            <span class="cmp-name">{providerName(p.id)}</span>
            <span class="cmp-total">{metricValue(p.totals)}</span>
          </header>
          <dl>
            <div><dt>{t('history.input')}</dt><dd>{formatTokens(p.totals.inputTokens)}</dd></div>
            <div><dt>{t('history.cacheRead')}</dt><dd>{formatTokens(p.totals.cacheReadTokens)}</dd></div>
            <div><dt>{t('history.cacheWrite')}</dt><dd>{formatTokens(p.totals.cacheWriteTokens)}</dd></div>
            <div><dt>{t('history.output')}</dt><dd>{formatTokens(p.totals.outputTokens)}</dd></div>
            <div><dt>{t('history.reasoning')}</dt><dd>{formatTokens(p.totals.reasoningTokens)}</dd></div>
            <div><dt>{t('history.requests')}</dt><dd>{formatInt(p.totals.requests)}</dd></div>
            <div><dt>{t('history.estCost')}</dt><dd>{formatCost(p.totals.estimatedCostUsd)}</dd></div>
          </dl>
          <p class="note">{t('history.costNote')}</p>
        </article>
      {/each}
    </div>
  {/if}

  <div class="card panel">
    <header class="panel-head">
      <h3>{t('history.table.title')}</h3>
      <div class="export-actions">
        <button class="btn" onclick={() => void copyCsv()} disabled={loading || sortedRows.length === 0}>
          {copied ? t('common.copied') : t('common.copy')}
        </button>
        <button class="btn" onclick={() => void exportCsv()} disabled={loading || exporting || sortedRows.length === 0}>
          {exporting ? t('common.saving') : t('history.exportCsv')}
        </button>
      </div>
    </header>

    {#if loading}
      <p class="muted">{t('common.loading')}</p>
    {:else if result && sortedRows.length === 0}
      <p class="muted">{t('history.noRows')}</p>
      {#if project !== null}<button class="btn clear-project" onclick={() => (project = null)}>{t('history.project.clear')}</button>{/if}
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
            {#each sortedRows as r, i (JSON.stringify([r.bucketStart, r.provider, r.model, r.project, i]))}
              <tr>
                <td>{formatBucket(r.bucketStart, bucket)}</td>
                <td>{providerName(r.provider)}</td>
                {#if groupByModel}<td class="model" title={r.model ?? ''}>{r.model ?? '—'}</td>{/if}
                {#if showProject}<td class="project-name" title={r.project || t('history.project.unassigned')}>{projectLabel(r.project ?? '')}</td>{/if}
                <td class="num mono">{formatTokens(r.inputTokens)}</td>
                <td class="num mono">{formatTokens(r.cacheReadTokens)}</td>
                <td class="num mono">{formatTokens(r.cacheWriteTokens)}</td>
                <td class="num mono">{formatTokens(r.outputTokens)}</td>
                <td class="num mono">{formatInt(r.requests)}</td>
                <td class="num mono strong">{formatTokens(r.totalTokens)}</td>
                <td class="num mono">{formatCost(r.estimatedCostUsd)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>

  <div class="card panel ingest">
    <header class="panel-head">
      <h3>{t('history.ingestion')}</h3>
      <button class="btn" onclick={() => void rescan()} disabled={rescanning || ingest?.running}>
        {rescanning || ingest?.running ? t('history.ingestRunning') : t('history.rescan')}
      </button>
    </header>
    <p class="muted">
      {#if ingest}
        {ingest.running ? t('history.ingestRunning') : ''}
        {t('history.ingestStats', {
          files: ingest.filesScanned,
          updated: ingest.filesUpdated,
          events: ingest.eventsAdded,
          ms: ingest.durationMs,
        })}
        {#if ingest.errors.length > 0}
          · <span class="err-inline">{t('history.ingestErrors', { n: ingest.errors.length })}</span>
        {/if}
      {:else}
        {t('history.ingestIdle')}
      {/if}
    </p>
    {#if ingest && ingest.errors.length > 0}
      <details class="scan-errors">
        <summary>{t('history.ingestErrors', { n: ingest.errors.length })}</summary>
        <ul>{#each ingest.errors as message, i (i)}<li>{message}</li>{/each}</ul>
      </details>
    {/if}
  </div>
</section>

<style>
  .history {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .controls {
    display: flex;
    flex-direction: column;
    gap: 0.625rem;
    padding: 0.75rem 1rem;
  }

  .group {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  .ctl-label {
    font-size: 0.75rem;
    color: var(--muted);
    white-space: nowrap;
  }

  .project-select {
    min-width: 8rem;
    /* a deep path must never widen the controls row past the card */
    max-width: min(100%, 22rem);
    text-overflow: ellipsis;
  }

  .project-group {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
    cursor: pointer;
  }

  .project-group input { accent-color: var(--focus); }
  .clear-project { align-self: flex-start; }

  .project-name {
    max-width: 18rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .segmented {
    display: inline-flex;
    padding: 0.125rem;
    gap: 0.125rem;
    border-radius: var(--r-control);
    background: var(--surface-2);
    border: 1px solid var(--border);
    flex-wrap: wrap;
  }

  .segmented button {
    padding: 0.1875rem 0.625rem;
    border-radius: calc(var(--r-control) - 0.125rem);
    font-size: 0.8125rem;
    color: var(--muted);
    white-space: nowrap;
    transition: background var(--dur-ui) var(--ease-out), color var(--dur-ui) var(--ease-out);
  }

  .segmented button:hover {
    color: var(--text);
  }

  .segmented button.active {
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow-card);
  }

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

  .panel-head .muted {
    font-size: 0.75rem;
  }

  .compare {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 17rem), 1fr));
    gap: 1rem;
  }

  .cmp {
    display: flex;
    flex-direction: column;
    gap: 0.625rem;
    padding: 1rem;
    min-width: 0;
  }

  .cmp-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .cmp-name {
    font-weight: 600;
  }

  .cmp-total {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }

  dl {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 11rem), 1fr));
    gap: 0.25rem 1rem;
    margin: 0;
    font-size: 0.8125rem;
  }

  dl > div {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
    min-width: 0;
  }

  dt {
    color: var(--muted);
    white-space: nowrap;
  }

  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }

  .note {
    margin: 0;
    font-size: 0.6875rem;
    color: var(--faint);
  }

  .feedback,
  .export-actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  .export-path,
  .scan-errors {
    overflow-wrap: anywhere;
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
    max-width: 14rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .err {
    margin: 0;
    color: var(--critical);
    font-size: 0.8125rem;
  }

  .err-inline {
    color: var(--warn);
  }

  .ingest p {
    margin: 0;
    font-size: 0.75rem;
  }
</style>
