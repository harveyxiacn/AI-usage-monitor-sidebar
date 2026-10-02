<!--
  Dashboard → History. Three sub-views behind one shared, sticky filter bar
  (range / provider / project):

    Usage            token analytics from the local session logs: KPI tiles,
                     usage over time, token composition, model & effort mix,
                     project ranking, activity heatmap, provider cards, table
    Quota            the quota-window history (QuotaHistoryPanel)
    Cost & budget    the monthly budget burn-up

  This file owns the filter state and the queries every sub-view shares; the
  panels live in ./history/. `?tab=history&view=quota|cost` deep-links a
  sub-view. [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { historyViewState, initialHistoryView, rememberHistoryView, type HistoryView } from '$lib/history-view-state';
  import BudgetChart from '$lib/components/BudgetChart.svelte';
  import UsageChart from '$lib/components/UsageChart.svelte';
  import UsageHeatmap from '$lib/components/UsageHeatmap.svelte';
  import { bucketDateRange } from '$lib/analytics';
  import QuotaHistoryPanel from './QuotaHistoryPanel.svelte';
  import SubscriptionRoi from './history/SubscriptionRoi.svelte';
  import { subscriptionTotal } from '$lib/subscription';
  import HistoryEmpty, { type EmptyKind } from './history/HistoryEmpty.svelte';
  import HistoryFilterBar from './history/HistoryFilterBar.svelte';
  import IngestPanel from './history/IngestPanel.svelte';
  import ModelMixPanel from './history/ModelMixPanel.svelte';
  import ProjectRanking from './history/ProjectRanking.svelte';
  import ProviderCompare from './history/ProviderCompare.svelte';
  import Segmented from './history/Segmented.svelte';
  import TokenCompositionChart from './history/TokenCompositionChart.svelte';
  import UsageKpiRow from './history/UsageKpiRow.svelte';
  import UsageTable from './history/UsageTable.svelte';
  import { getUsageCalendar, getUsageHistory, onIngestProgress, reingestLogs, type Unlisten } from '$lib/api';
  import { budgetProgress, historyRange, localDateInput, projectLabels, projectName, type HeatMetric, type HistoryPreset } from '$lib/history';
  import { formatCost, formatInt } from '$lib/format';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { providerDisplayName } from '$lib/providers';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import type { Bucket, CalendarResult, HistoryResult, IngestStats, ProviderId } from '$lib/types';

  interface Props {
    /** flips whenever the palette changes so the canvas re-renders */
    themeKey: string;
  }

  let { themeKey }: Props = $props();

  const DAY = 86_400_000;
  let view = $state<HistoryView>(initialHistoryView());
  let preset = $state<HistoryPreset>(historyViewState.preset);
  let customFrom = $state(historyViewState.customFrom || localDateInput(Date.now() - 6 * DAY));
  let customTo = $state(historyViewState.customTo || localDateInput(Date.now()));
  let queryTime = $state(Date.now());
  let bucket = $state<Bucket>(historyViewState.bucket);
  let provider = $state<ProviderId | ''>(historyViewState.provider);
  let groupByModel = $state(historyViewState.groupByModel);
  let groupByProject = $state(historyViewState.groupByProject);
  let project = $state<string | null>(historyViewState.project);
  let projects = $state<string[]>([]);
  let metric = $state<'tokens' | 'cost'>(historyViewState.metric);
  let chartLayout = $state<'grouped' | 'stacked'>(historyViewState.chartLayout);

  let result = $state<HistoryResult | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  /** Activity heatmap: a fixed 26-week window, independent of the range above. */
  const HEATMAP_WEEKS = 26;
  let heatView = $state<'calendar' | 'punchcard'>(historyViewState.heatView);
  let heatMetric = $state<HeatMetric>(historyViewState.heatMetric);
  let calendar = $state<CalendarResult | null>(null);
  let calendarError = $state<string | null>(null);
  let calendarId = 0;

  let tableView = $state<'buckets' | 'sessions'>(historyViewState.tableView);
  $effect(() => {
    Object.assign(historyViewState, { preset, customFrom, customTo, bucket, provider, groupByModel, groupByProject, project, metric, chartLayout, tableView, heatView, heatMetric });
  });

  let ingest = $state<IngestStats | null>(null);
  let rescanning = $state(false);
  let actionError = $state<string | null>(null);
  let requestId = 0;
  let disposed = false;
  /** Bumped when a log scan finished, so the slower panels reload once. */
  let dataVersion = $state(0);

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

  function pickPreset(next: HistoryPreset) {
    preset = next;
    queryTime = Date.now();
    // a sensible default granularity per preset; the user can still override
    if (next === 'today') bucket = 'hour';
    else if (next === '90d') bucket = 'week';
    else bucket = 'day';
  }

  let historyFilterKey = '';

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
    // Invalidate immediately so an older response cannot flash during debounce.
    requestId++;
    const timer = setTimeout(() => void load(), 80);
    return () => clearTimeout(timer);
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
    calendarId++;
    const timer = setTimeout(() => void loadCalendar(), 80);
    return () => clearTimeout(timer);
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
    // Ingestion events carry fresh data. The fallback reconciles after hiding,
    // and midnight moves the local-day boundaries without idle minute queries.
    const resume = () => { if (!document.hidden && !rescanning) queryTime = Date.now(); };
    const timer = setInterval(() => { if (!document.hidden && !rescanning && localDateInput(queryTime) !== localDateInput(Date.now())) queryTime = Date.now(); }, 60_000);
    document.addEventListener('visibilitychange', resume);
    return () => {
      disposed = true;
      requestId++;
      calendarId++;
      un?.();
      clearInterval(timer);
      document.removeEventListener('visibilitychange', resume);
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

  /** Clicking a heatmap day narrows the range to that single local day. */
  function pickDay(date: string) {
    preset = 'custom';
    customFrom = date;
    customTo = date;
    bucket = 'hour';
  }

  /** Clicking a chart bar narrows the range to that bucket (an hour is already the finest). */
  function pickBucket(bucketStart: string) {
    const picked = bucketDateRange(bucketStart, bucket);
    if (!picked) return;
    if (picked.from === picked.to) return pickDay(picked.from);
    preset = 'custom';
    customFrom = picked.from;
    customTo = picked.to;
    bucket = 'day';
  }

  const pickedDay = $derived(preset === 'custom' && customFrom === customTo ? customFrom : null);

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

  // ---- monthly budget (estimates only; see history.costNote) ----
  const monthlyBudgetUsd = $derived(settings.value.monthlyBudgetUsd);
  const budget = $derived(
    monthlyBudgetUsd > 0 && calendar ? budgetProgress(calendar.days, monthlyBudgetUsd, queryTime) : null
  );
  /** Requests this month whose model has no price: the budget total excludes them. */
  const budgetUnpriced = $derived.by(() => {
    if (!budget || !calendar) return 0;
    const start = localDateInput(budget.monthStart);
    return calendar.days.reduce((sum, day) => (day.date >= start ? sum + (day.unpricedRequests ?? 0) : sum), 0);
  });

  // ---- empty states ----
  const emptyKind = $derived.by((): EmptyKind | null => {
    if (!result || result.rows.length > 0) return null;
    if (!settings.value.ingestEnabled) return 'ingestDisabled';
    if (ingest && !ingest.running && ingest.filesScanned === 0) return 'noLogs';
    if (provider !== '' || project !== null) return 'filtered';
    return 'range';
  });

  function setView(next: HistoryView) {
    view = next;
    rememberHistoryView(next);
  }

  function clearFilters() {
    provider = '';
    project = null;
  }

  const metricOptions = $derived([['tokens', t('history.metric.tokens')], ['cost', t('history.metric.cost')]] as const);
  const layoutOptions = $derived([['grouped', t('history.chartLayout.grouped')], ['stacked', t('history.chartLayout.stacked')]] as const);
  const heatViewOptions = $derived([['calendar', t('history.activity.calendar')], ['punchcard', t('history.activity.punchcard')]] as const);
  const heatMetricOptions = $derived(
    (['tokens', 'cost', 'requests', 'cache'] as const).map((id) => [id, tDyn(`history.activity.metric.${id}`)] as const)
  );
</script>

<section class="history">
  <HistoryFilterBar
    {view}
    onview={setView}
    {preset}
    bind:customFrom
    bind:customTo
    bind:provider
    bind:project
    rangeValid={range !== null}
    {providerOptions}
    {projectOptions}
    {providerName}
    {projectLabel}
    onpreset={pickPreset}
  />

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

  {#if view === 'usage'}
    <div class="card usage-controls">
      <div class="group">
        <label class="ctl-label" for="bucket">{t('history.bucket')}</label>
        <select id="bucket" class="field" bind:value={bucket}>
          {#each buckets as b (b)}
            <option value={b}>{tDyn(`history.bucket.${b}`)}</option>
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

        <Segmented options={metricOptions} value={metric} label={t('history.metric')} onchange={(v) => (metric = v)} />
      </div>
    </div>

    {#if emptyKind}
      <HistoryEmpty kind={emptyKind} {rescanning} onrescan={() => void rescan()} onclear={clearFilters} />
    {:else}
      <UsageKpiRow {range} {provider} {project} {dataVersion} {themeKey} />

      <p class="muted small">{t('history.modelDetailsNote')}</p>

      <div class="card panel" aria-busy={loading}>
        <header class="panel-head">
          <h3>{t('history.chartTitle')}</h3>
          <div class="head-controls">
            <Segmented options={layoutOptions} value={chartLayout} label={t('history.chartLayout')} onchange={(v) => (chartLayout = v)} />
          </div>
        </header>
        {#if loading && !result}
          <p class="muted" role="status">{t('common.loading')}</p>
        {:else if result}
          <UsageChart {rows} {bucket} {groupByModel} {groupByProject} {projectNames} {metric} layout={chartLayout} {themeKey} onpickbucket={pickBucket} />
        {/if}
      </div>

      <div class="card panel" aria-busy={loading}>
        <header class="panel-head">
          <h3>{t('history.composition.title')}</h3>
          <span class="muted small">{t('history.composition.note')}</span>
        </header>
        {#if loading && !result}
          <p class="muted" role="status">{t('common.loading')}</p>
        {:else if result}
          <TokenCompositionChart {rows} {bucket} {themeKey} />
        {/if}
      </div>

      <div class="card panel">
        <header class="panel-head">
          <h3>{t('history.mix.title')}</h3>
          <span class="muted small">{t('history.mix.note')}</span>
        </header>
        <ModelMixPanel
          {range}
          {bucket}
          {provider}
          {project}
          mainRows={groupByModel && !groupByProject ? rows : null}
          {dataVersion}
          {themeKey}
        />
      </div>

      <div class="card panel">
        <header class="panel-head">
          <h3>{t('history.projects.title')}</h3>
          <span class="muted small">{t('history.projects.hint')}</span>
        </header>
        <ProjectRanking
          {range}
          {provider}
          selected={project}
          {dataVersion}
          {themeKey}
          {projectLabel}
          onpick={(path) => (project = project === path ? null : path)}
        />
      </div>

      <div class="card panel activity">
        <header class="panel-head">
          <h3>{t('history.activity')}</h3>
          <div class="head-controls">
            <span class="muted">{t('history.activity.range', { weeks: HEATMAP_WEEKS })}</span>
            <Segmented options={heatMetricOptions} value={heatMetric} label={t('history.activity.metric')} onchange={(v) => (heatMetric = v)} />
            <Segmented options={heatViewOptions} value={heatView} label={t('history.activity.view')} onchange={(v) => (heatView = v)} />
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
            metric={heatMetric}
            view={heatView}
            selected={pickedDay}
            onpick={pickDay}
          />
        {:else}
          <p class="muted" role="status">{t('common.loading')}</p>
        {/if}
        <p class="note">{t('history.costNote')}</p>
      </div>

      <ProviderCompare totals={providerTotals} {metric} costApproximate={result?.costApproximate ?? false} {providerName} />

      <UsageTable
        {range}
        {provider}
        {project}
        {rows}
        {loading}
        hasResult={result !== null}
        {bucket}
        {groupByModel}
        {showProject}
        bind:tableView
        {providerName}
        {projectLabel}
        onclearproject={() => (project = null)}
      />
    {/if}

    {#if !emptyKind}
      <IngestPanel {ingest} {rescanning} onrescan={() => void rescan()} />
    {/if}
  {:else if view === 'quota'}
    <QuotaHistoryPanel {range} provider={provider || null} live={preset !== 'custom'} {themeKey} />
  {:else}
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
      {#if monthlyBudgetUsd <= 0}
        <p class="muted">{t('history.budget.notSet')}</p>
      {:else if budget}
        {#if budgetUnpriced > 0}
          <p class="muted cost-note" role="status">{t('history.budget.unpriced', { count: formatInt(budgetUnpriced) })}</p>
        {:else if budget.incomplete}
          <p class="muted cost-note" role="status">{t('history.budget.incomplete')}</p>
        {/if}
        <BudgetChart series={budget.series} budgetUsd={monthlyBudgetUsd} subscriptionUsd={subscriptionTotal(settings.value.subscriptionUsd, provider || null)} {themeKey} />
        <p class="note">{t('history.costNote')}</p>
      {:else if calendarError}
        <p class="err" role="alert">{t('common.error', { message: calendarError })}</p>
      {:else}
        <p class="muted" role="status">{t('common.loading')}</p>
      {/if}
    </div>
    <SubscriptionRoi provider={provider || null} {themeKey} refreshKey={dataVersion} />
  {/if}
</section>

<style>
  .history {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .usage-controls {
    padding: 0.625rem 1rem;
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

  .project-group {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
    cursor: pointer;
  }

  .project-group input { accent-color: var(--focus); }

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

  p.small {
    margin: 0;
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

  .note {
    margin: 0;
    font-size: 0.6875rem;
    color: var(--faint);
  }

  .feedback {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  .err {
    margin: 0;
    color: var(--critical);
    font-size: 0.8125rem;
  }
</style>
