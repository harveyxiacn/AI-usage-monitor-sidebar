<!--
  Dashboard → History. Token usage recorded from the providers' local session
  logs: range/bucket/provider/group controls, a stacked bar chart, per-provider
  comparison cards, a sortable table with CSV export and the ingestion line.
  [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import UsageChart from '$lib/components/UsageChart.svelte';
  import { getUsageHistory, onIngestProgress, reingestLogs, type Unlisten } from '$lib/api';
  import { formatBucket, formatCost, formatInt, formatTokens } from '$lib/format';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import type {
    Bucket,
    HistoryResult,
    HistoryRow,
    IngestStats,
    ProviderId,
    TokenTotals,
  } from '$lib/types';

  interface Props {
    /** flips whenever the palette changes so the canvas re-renders */
    themeKey: string;
  }

  let { themeKey }: Props = $props();

  type Preset = 'today' | '7d' | '30d' | '90d' | 'custom';

  const DAY = 86_400_000;
  const toLocalDateInput = (ms: number) => {
    const d = new Date(ms);
    // <input type="date"> wants a *local* yyyy-mm-dd, toISOString would shift it
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  };
  const startOfToday = () => {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    return d.getTime();
  };

  let preset = $state<Preset>('7d');
  let customFrom = $state(toLocalDateInput(Date.now() - 6 * DAY));
  let customTo = $state(toLocalDateInput(Date.now()));
  let bucket = $state<Bucket>('day');
  let provider = $state<ProviderId | ''>('');
  let groupByModel = $state(false);
  let metric = $state<'tokens' | 'cost'>('tokens');

  let result = $state<HistoryResult | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  let ingest = $state<IngestStats | null>(null);
  let rescanning = $state(false);
  let copied = $state(false);

  let sortKey = $state<'bucketStart' | 'provider' | 'model' | keyof TokenTotals>('bucketStart');
  let sortDir = $state<1 | -1>(-1);

  /** [fromMs, toMs) for the active preset; custom uses whole local days. */
  const range = $derived.by<{ from: number; to: number }>(() => {
    const now = Date.now();
    switch (preset) {
      case 'today':
        return { from: startOfToday(), to: now };
      case '30d':
        return { from: now - 30 * DAY, to: now };
      case '90d':
        return { from: now - 90 * DAY, to: now };
      case 'custom': {
        const from = new Date(`${customFrom}T00:00:00`).getTime();
        const to = new Date(`${customTo}T23:59:59.999`).getTime();
        return Number.isFinite(from) && Number.isFinite(to) && to > from
          ? { from, to }
          : { from: now - 7 * DAY, to: now };
      }
      default:
        return { from: now - 7 * DAY, to: now };
    }
  });

  /** Hour buckets only make sense for short ranges (contract: ≤ 2 days). */
  const hourAllowed = $derived(range.to - range.from <= 2 * DAY + 1000);
  const buckets = $derived<Bucket[]>(
    hourAllowed ? ['hour', 'day', 'week', 'month'] : ['day', 'week', 'month']
  );

  // keep the bucket legal when the range shrinks/grows underneath it
  $effect(() => {
    if (!hourAllowed && bucket === 'hour') bucket = 'day';
  });

  function pickPreset(next: Preset) {
    preset = next;
    // a sensible default granularity per preset; the user can still override
    if (next === 'today') bucket = 'hour';
    else if (next === '90d') bucket = 'week';
    else bucket = 'day';
  }

  async function load() {
    loading = true;
    try {
      result = await getUsageHistory({
        from: new Date(range.from).toISOString(),
        to: new Date(range.to).toISOString(),
        bucket,
        groupByModel,
        provider: provider === '' ? null : provider,
      });
      error = null;
    } catch (e) {
      error = String(e);
      result = null;
    } finally {
      loading = false;
    }
  }

  // refetch whenever a query input changes
  $effect(() => {
    void [range.from, range.to, bucket, groupByModel, provider];
    void load();
  });

  onMount(() => {
    let un: Unlisten | null = null;
    let disposed = false;
    void onIngestProgress((stats) => {
      ingest = stats;
      // a finished scan may have added events — refresh the view
      if (!stats.running) void load();
    }).then((u) => (disposed ? u() : (un = u)));
    return () => {
      disposed = true;
      un?.();
    };
  });

  async function rescan() {
    rescanning = true;
    try {
      ingest = await reingestLogs();
    } catch (e) {
      error = String(e);
    } finally {
      rescanning = false;
      await load();
    }
  }

  const rows = $derived(result?.rows ?? []);
  const totals = $derived(result?.totals ?? null);

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
      else if (key === 'model') cmp = (a.model ?? '').localeCompare(b.model ?? '');
      else cmp = (a[key] ?? 0) - (b[key] ?? 0);
      return cmp * dir || Date.parse(a.bucketStart) - Date.parse(b.bucketStart);
    });
    return list;
  });

  function sortBy(key: typeof sortKey) {
    if (sortKey === key) sortDir = sortDir === 1 ? -1 : 1;
    else {
      sortKey = key;
      sortDir = key === 'bucketStart' ? -1 : -1;
    }
  }

  const CSV_COLUMNS: Array<[string, (r: HistoryRow) => string | number]> = [
    ['bucket_start', (r) => r.bucketStart],
    ['provider', (r) => r.provider],
    ['model', (r) => r.model ?? ''],
    ['input_tokens', (r) => r.inputTokens],
    ['cache_write_tokens', (r) => r.cacheWriteTokens],
    ['cache_read_tokens', (r) => r.cacheReadTokens],
    ['output_tokens', (r) => r.outputTokens],
    ['reasoning_tokens', (r) => r.reasoningTokens],
    ['total_tokens', (r) => r.totalTokens],
    ['requests', (r) => r.requests],
    ['estimated_cost_usd', (r) => (r.estimatedCostUsd == null ? '' : r.estimatedCostUsd.toFixed(4))],
  ];

  function csvCell(v: string | number): string {
    const s = String(v);
    return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  }

  async function copyCsv() {
    const header = CSV_COLUMNS.map(([name]) => name).join(',');
    const body = sortedRows.map((r) => CSV_COLUMNS.map(([, get]) => csvCell(get(r))).join(',')).join('\n');
    const csv = `${header}\n${body}\n`;
    try {
      await navigator.clipboard.writeText(csv);
    } catch {
      // clipboard API is unavailable in some WebKitGTK builds — fall back to
      // a hidden textarea + execCommand, which still works there.
      const ta = document.createElement('textarea');
      ta.value = csv;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      try {
        document.execCommand('copy');
      } catch {
        /* nothing else we can do */
      }
      ta.remove();
    }
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  const metricValue = (tt: TokenTotals) =>
    metric === 'cost' ? formatCost(tt.estimatedCostUsd) : formatTokens(tt.totalTokens);

  const providerName = (id: ProviderId) => (id === 'claude' ? 'Claude' : 'Codex');

  const PRESETS: Array<[Preset, string]> = [
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
          <button class:active={preset === id} onclick={() => pickPreset(id)}>{tDyn(key)}</button>
        {/each}
      </div>
    </div>

    {#if preset === 'custom'}
      <div class="group">
        <label class="ctl-label" for="from">{t('history.from')}</label>
        <input id="from" class="field" type="date" bind:value={customFrom} max={customTo} />
        <label class="ctl-label" for="to">{t('history.to')}</label>
        <input id="to" class="field" type="date" bind:value={customTo} min={customFrom} />
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
        <option value="claude">Claude</option>
        <option value="codex">Codex</option>
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

      <div class="segmented" role="group" aria-label={t('history.metric')}>
        <button class:active={metric === 'tokens'} onclick={() => (metric = 'tokens')}>
          {t('history.metric.tokens')}
        </button>
        <button class:active={metric === 'cost'} onclick={() => (metric = 'cost')}>
          {t('history.metric.cost')}
        </button>
      </div>
    </div>
  </div>

  {#if error}
    <p class="err">{t('common.error', { message: error })}</p>
  {/if}

  <div class="card panel">
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
    {#if loading && !result}
      <p class="muted">{t('common.loading')}</p>
    {:else}
      <UsageChart {rows} {bucket} {groupByModel} {metric} {themeKey} />
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
      <button class="btn" onclick={() => void copyCsv()} disabled={sortedRows.length === 0}>
        {copied ? t('common.copied') : t('common.copy')}
      </button>
    </header>

    {#if sortedRows.length === 0}
      <p class="muted">{t('history.noRows')}</p>
    {:else}
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              {#each [['bucketStart', 'history.table.bucket'], ['provider', 'history.table.provider'], ...(groupByModel ? [['model', 'history.table.model']] : []), ['inputTokens', 'history.input'], ['cacheReadTokens', 'history.cacheRead'], ['cacheWriteTokens', 'history.cacheWrite'], ['outputTokens', 'history.output'], ['requests', 'history.requests'], ['totalTokens', 'history.table.total'], ['estimatedCostUsd', 'history.estCost']] as [key, label] (key)}
                <th
                  class:num={key !== 'bucketStart' && key !== 'provider' && key !== 'model'}
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
            {#each sortedRows as r, i (r.bucketStart + r.provider + (r.model ?? '') + i)}
              <tr>
                <td>{formatBucket(r.bucketStart, bucket)}</td>
                <td>{providerName(r.provider)}</td>
                {#if groupByModel}<td class="model" title={r.model ?? ''}>{r.model ?? '—'}</td>{/if}
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
      <button class="btn" onclick={() => void rescan()} disabled={rescanning}>
        {rescanning ? t('history.ingestRunning') : t('history.rescan')}
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

  .segmented {
    display: inline-flex;
    padding: 0.125rem;
    gap: 0.125rem;
    border-radius: var(--r-control);
    background: var(--surface-2);
    border: 1px solid var(--border);
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
    grid-template-columns: repeat(auto-fit, minmax(17rem, 1fr));
    gap: 1rem;
  }

  .cmp {
    display: flex;
    flex-direction: column;
    gap: 0.625rem;
    padding: 1rem;
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
    grid-template-columns: 1fr 1fr;
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
