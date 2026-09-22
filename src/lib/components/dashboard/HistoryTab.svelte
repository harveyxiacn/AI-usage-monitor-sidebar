<!--
  Dashboard → History. Token usage recorded from the providers' local session
  logs: range/bucket/provider/group controls, an activity heatmap, a stacked bar
  chart, an optional monthly-budget burn-up, per-provider comparison cards, a
  sortable bucket/session table with CSV export and the ingestion line.
  [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import BudgetChart from '$lib/components/BudgetChart.svelte';
  import UsageChart from '$lib/components/UsageChart.svelte';
  import UsageHeatmap from '$lib/components/UsageHeatmap.svelte';
  import { exportUsageCsv, getUsageCalendar, getUsageHistory, getUsageSessions, onIngestProgress, reingestLogs, type Unlisten } from '$lib/api';
  import { budgetProgress, historyCsv, historyRange, localDateInput, projectLabels, projectName, sessionsCsv, type HistoryPreset } from '$lib/history';
  import { formatBucket, formatCost, formatEstimatedCost, formatDuration, formatInt, formatTokens } from '$lib/format';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { providerDisplayName } from '$lib/providers';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import type {
    Bucket,
    CalendarResult,
    HistoryResult,
    IngestStats,
    ProviderId,
    SessionRow,
    SessionsResult,
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

  /** Activity heatmap: a fixed 26-week window, independent of the range above. */
  const HEATMAP_WEEKS = 26;
  let heatView = $state<'calendar' | 'punchcard'>('calendar');
  let calendar = $state<CalendarResult | null>(null);
  let calendarError = $state<string | null>(null);
  let calendarId = 0;

  /** Session drill-down; loaded only while its view is on screen. */
  let tableView = $state<'buckets' | 'sessions'>('buckets');
  let sessions = $state<SessionsResult | null>(null);
  let sessionsLoading = $state(false);
  let sessionsError = $state<string | null>(null);
  let sessionsId = 0;
  let sessionSortKey = $state<'sessionId' | 'provider' | 'project' | 'firstTs' | 'lastTs' | 'durationMs' | keyof TokenTotals>('totalTokens');
  let sessionSortDir = $state<1 | -1>(-1);

  let ingest = $state<IngestStats | null>(null);
  let rescanning = $state(false);
  let copied = $state(false);
  let exported = $state<string | null>(null);
  let exporting = $state(false);
  let actionError = $state<string | null>(null);
  let requestId = 0;
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  /** Bumped when a log scan finished, so the slower panels reload once. */
  let dataVersion = $state(0);

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

  let historyFilterKey = '';
  let sessionsFilterKey = '';

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
    // Time ticks and local ingestion refresh the same view in place. A real
    // filter change must still clear stale rows before fetching its result.
    const filterKey = JSON.stringify([preset, customFrom, customTo, activeRange.from, bucket, groupByModel, groupByProject, provider, project]);
    if (filterKey !== historyFilterKey) result = null;
    historyFilterKey = filterKey;
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

  /**
   * The heatmap window: `[start of the day 26 weeks ago, tomorrow)`, local.
   * Kept as two numbers rather than an object so the minute tick that keeps
   * the table fresh does not re-scan half a year of events every minute — the
   * window only really moves at local midnight.
   */
  const heatTo = $derived.by(() => {
    const to = new Date(queryTime);
    to.setHours(0, 0, 0, 0);
    to.setDate(to.getDate() + 1);
    return to.getTime();
  });
  const heatFrom = $derived.by(() => {
    const from = new Date(heatTo);
    // calendar arithmetic, so a DST week is still a week
    from.setDate(from.getDate() - HEATMAP_WEEKS * 7);
    return from.getTime();
  });

  async function loadCalendar() {
    const id = ++calendarId;
    const [from, to] = [heatFrom, heatTo];
    calendarError = null;
    try {
      const next = await getUsageCalendar({
        from: new Date(from).toISOString(),
        to: new Date(to).toISOString(),
        provider: provider === '' ? null : provider,
        project,
      });
      if (id === calendarId && !disposed) calendar = next;
    } catch (e) {
      if (id === calendarId && !disposed) {
        calendar = null;
        calendarError = String(e);
      }
    }
  }

  // `dataVersion` changes only when a scan actually added events
  $effect(() => {
    void [heatFrom, heatTo, provider, project, dataVersion];
    void loadCalendar();
  });

  async function loadSessions() {
    const id = ++sessionsId;
    const activeRange = range;
    if (!activeRange || tableView !== 'sessions') {
      sessions = null;
      sessionsLoading = false;
      return;
    }
    const filterKey = JSON.stringify([preset, customFrom, customTo, activeRange.from, provider, project]);
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
    void loadSessions();
  });

  onMount(() => {
    let un: Unlisten | null = null;
    void onIngestProgress((stats) => {
      ingest = stats;
      // a finished scan may have added events — refresh the view
      if (!stats.running && !rescanning && stats.eventsAdded > 0) {
        queryTime = Date.now();
        dataVersion += 1;
      }
    }).then((u) => (disposed ? u() : (un = u))).catch((e) => { actionError = String(e); });
    const timer = setInterval(() => { if (!document.hidden && !rescanning) queryTime = Date.now(); }, 60_000);
    return () => {
      disposed = true;
      requestId++;
      calendarId++;
      sessionsId++;
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
      dataVersion += 1;
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

  const TEXT_SESSION_KEYS: readonly string[] = ['sessionId', 'provider', 'project'];
  // offsets move across a DST change, so timestamps are compared as instants
  const TIME_SESSION_KEYS: readonly string[] = ['firstTs', 'lastTs'];

  /** Sorted copy of the (already capped) session rows. */
  const sortedSessions = $derived.by(() => {
    const list = [...(sessions?.rows ?? [])];
    const key = sessionSortKey;
    const dir = sessionSortDir;
    list.sort((a, b) => {
      let cmp: number;
      if (TEXT_SESSION_KEYS.includes(key)) cmp = String(a[key as 'sessionId']).localeCompare(String(b[key as 'sessionId']));
      else if (TIME_SESSION_KEYS.includes(key)) cmp = Date.parse(a[key as 'firstTs']) - Date.parse(b[key as 'firstTs']);
      else if (key === 'estimatedCostUsd') cmp = (a.estimatedCostUsd ?? a.knownCostUsd ?? 0) - (b.estimatedCostUsd ?? b.knownCostUsd ?? 0);
      else cmp = ((a[key as 'totalTokens'] ?? 0) as number) - ((b[key as 'totalTokens'] ?? 0) as number);
      return cmp * dir || b.totalTokens - a.totalTokens || a.sessionId.localeCompare(b.sessionId);
    });
    return list;
  });

  function sortSessionsBy(key: typeof sessionSortKey) {
    if (sessionSortKey === key) sessionSortDir = sessionSortDir === 1 ? -1 : 1;
    else {
      sessionSortKey = key;
      sessionSortDir = TEXT_SESSION_KEYS.includes(key) ? 1 : -1;
    }
  }

  const sessionLabel = (row: SessionRow) => row.sessionId || t('history.sessions.unassigned');

  /** Clicking a heatmap day narrows the range below to that single local day. */
  function pickDay(date: string) {
    preset = 'custom';
    customFrom = date;
    customTo = date;
    bucket = 'hour';
  }

  const pickedDay = $derived(preset === 'custom' && customFrom === customTo ? customFrom : null);

  // ---- monthly budget (estimates only; see history.costNote) ----
  const monthlyBudgetUsd = $derived(settings.value.monthlyBudgetUsd);
  const budget = $derived(
    metric === 'cost' && monthlyBudgetUsd > 0 && calendar
      ? budgetProgress(calendar.days, monthlyBudgetUsd, queryTime)
      : null
  );

  /** The active table view decides what "copy" and "export" produce. */
  const csvText = () => (tableView === 'sessions' ? sessionsCsv(sortedSessions) : historyCsv(sortedRows));
  const csvEmpty = $derived(tableView === 'sessions' ? sortedSessions.length === 0 : sortedRows.length === 0);
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

  const metricValue = (tt: TokenTotals) =>
    metric === 'cost' ? formatEstimatedCost(tt) : formatTokens(tt.totalTokens);

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

  <div class="card panel activity">
    <header class="panel-head">
      <h3>{t('history.activity')}</h3>
      <div class="head-controls">
        <span class="muted">{t('history.activity.range', { weeks: HEATMAP_WEEKS })}</span>
        <div class="segmented" role="group" aria-label={t('history.activity.view')}>
          <button class:active={heatView === 'calendar'} aria-pressed={heatView === 'calendar'} onclick={() => (heatView = 'calendar')}>
            {t('history.activity.calendar')}
          </button>
          <button class:active={heatView === 'punchcard'} aria-pressed={heatView === 'punchcard'} onclick={() => (heatView = 'punchcard')}>
            {t('history.activity.punchcard')}
          </button>
        </div>
      </div>
    </header>
    {#if calendarError}
      <div class="feedback" role="alert">
        <p class="err">{t('common.error', { message: calendarError })}</p>
        <button class="btn" onclick={() => void loadCalendar()}>{t('common.retry')}</button>
      </div>
    {:else if calendar}
      <UsageHeatmap
        days={calendar.days}
        slots={calendar.slots}
        from={heatFrom}
        to={heatTo}
        {metric}
        view={heatView}
        selected={pickedDay}
        onpick={pickDay}
      />
    {:else}
      <p class="muted" role="status">{t('common.loading')}</p>
    {/if}
    <p class="note">{t('history.costNote')}</p>
  </div>

  {#if monthlyBudgetUsd > 0}
    <div class="card panel">
      <header class="panel-head">
        <h3>{t('history.budget.title')}</h3>
        {#if budget}
          <span class="budget-stat" class:over={budget.percentUsed > 100} role="status">
            {t('history.budget.stat', {
              percent: `${Math.round(budget.percentUsed)}%`,
              pace: formatCost(budget.paceUsd),
              budget: formatCost(monthlyBudgetUsd),
            })}
          </span>
        {/if}
      </header>
      {#if budget}
        {#if budget.incomplete}<p class="muted cost-note" role="status">{t('history.budget.incomplete')}</p>{/if}
        <BudgetChart series={budget.series} budgetUsd={monthlyBudgetUsd} {themeKey} />
        <p class="note">{t('history.costNote')}</p>
      {:else if metric !== 'cost'}
        <p class="muted">{t('history.budget.tokensHint')}</p>
      {:else}
        <p class="muted" role="status">{t('common.loading')}</p>
      {/if}
    </div>
  {/if}

  <div class="card panel" aria-busy={loading}>
    <header class="panel-head">
      <h3>{t('history.chartTitle')}</h3>
      {#if totals}
        <span class="muted">
          {t('history.totalTokens')}: {formatTokens(totals.totalTokens)} ·
          {t('history.requests')}: {formatInt(totals.requests)} ·
          {t('history.estCost')}: {formatEstimatedCost(totals)}
        </span>
      {/if}
    </header>
    {#if loading && !result}
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
            <div><dt>{t('history.estCost')}</dt><dd>{formatEstimatedCost(p.totals)}</dd></div>
          </dl>
          <p class="note">{t('history.costNote')}</p>
          {#if p.totals.estimatedCostUsd == null && p.totals.knownCostUsd != null}
            <p class="note">{t('history.partialCostNote', { count: formatInt(p.totals.unpricedRequests ?? 0) })}</p>
          {:else if p.totals.estimatedCostUsd == null}
            <p class="note">{t('history.costMissingNote')}</p>
          {/if}
          {#if result?.costApproximate}
            <p class="note">{t('history.costApproxNote')}</p>
          {/if}
        </article>
      {/each}
    </div>
  {/if}

  <div class="card panel">
    <header class="panel-head">
      <h3>{tableView === 'sessions' ? t('history.sessions.title') : t('history.table.title')}</h3>
      <div class="head-controls">
        <div class="segmented" role="group" aria-label={t('history.view')}>
          <button class:active={tableView === 'buckets'} aria-pressed={tableView === 'buckets'} onclick={() => (tableView = 'buckets')}>
            {t('history.view.buckets')}
          </button>
          <button class:active={tableView === 'sessions'} aria-pressed={tableView === 'sessions'} onclick={() => (tableView = 'sessions')}>
            {t('history.view.sessions')}
          </button>
        </div>
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
      {:else if sessions && sortedSessions.length === 0}
        <p class="muted">{t('history.sessions.none')}</p>
      {:else if sessions}
        <p class="muted small">
          {sessions.truncated
            ? t('history.sessions.capped', { shown: formatInt(sessions.rows.length), total: formatInt(sessions.totalSessions) })
            : t('history.sessions.count', { n: formatInt(sessions.totalSessions) })}
        </p>
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                {#each [['sessionId', 'history.sessions.id'], ['provider', 'history.table.provider'], ['project', 'history.project'], ['firstTs', 'history.sessions.start'], ['lastTs', 'history.sessions.end'], ['durationMs', 'history.sessions.duration'], ['requests', 'history.requests'], ['totalTokens', 'history.table.total'], ['estimatedCostUsd', 'history.estCost']] as [key, label] (key)}
                  <th
                    class:num={key === 'durationMs' || key === 'requests' || key === 'totalTokens' || key === 'estimatedCostUsd'}
                    aria-sort={sessionSortKey === key ? (sessionSortDir === 1 ? 'ascending' : 'descending') : 'none'}
                  >
                    <button onclick={() => sortSessionsBy(key as typeof sessionSortKey)}>
                      {tDyn(label)}
                      {#if sessionSortKey === key}<span class="caret">{sessionSortDir === 1 ? '▲' : '▼'}</span>{/if}
                    </button>
                  </th>
                {/each}
                <th>{t('history.sessions.models')}</th>
              </tr>
            </thead>
            <tbody>
              {#each sortedSessions as s (`${s.provider}:${s.sessionId}`)}
                <tr>
                  <td class="session-id" title={s.sessionId}>{sessionLabel(s)}</td>
                  <td>{providerName(s.provider)}</td>
                  <td class="project-name" title={s.project || t('history.project.unassigned')}>{projectLabel(s.project)}</td>
                  <td class="mono">{new Date(s.firstTs).toLocaleString()}</td>
                  <td class="mono">{new Date(s.lastTs).toLocaleString()}</td>
                  <td class="num mono">{formatDuration(s.durationMs)}</td>
                  <td class="num mono">{formatInt(s.requests)}</td>
                  <td class="num mono strong">{formatTokens(s.totalTokens)}</td>
                  <td class="num mono">{formatEstimatedCost(s)}</td>
                  <td class="model" title={s.models.join(', ')}>{s.models.join(', ')}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {:else if loading && !result}
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
                <td class="num mono">{formatEstimatedCost(r)}</td>
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

  .head-controls {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  .small {
    font-size: 0.75rem;
  }

  .cost-note {
    margin: 0;
    font-size: 0.75rem;
  }

  .activity {
    /* the heatmap sizes itself from the card, never the other way round */
    min-width: 0;
  }

  .budget-stat {
    font-size: 0.75rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .budget-stat.over {
    color: var(--critical);
    font-weight: 600;
  }

  .session-id {
    max-width: 14rem;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: var(--font-mono);
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
