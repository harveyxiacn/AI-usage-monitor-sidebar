<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { getQuotaHistory, exportUsageCsv } from '$lib/api';
  import QuotaHistoryChart from '$lib/components/QuotaHistoryChart.svelte';
  import { kindLabel } from '$lib/format';
  import { csvCell, type HistoryRange } from '$lib/history';
  import { intlLocale, t, tDyn } from '$lib/i18n/i18n.svelte';
  import { providerDisplayName } from '$lib/providers';
  import { buildQuotaHistory, type QuotaHistorySeries } from '$lib/quota-history';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import type { ProviderId, QuotaSample } from '$lib/types';

  interface Props { range: HistoryRange | null; provider: ProviderId | null; live: boolean; themeKey: string }
  let { range, provider, live, themeKey }: Props = $props();
  let samples = $state<QuotaSample[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let exportError = $state<string | null>(null);
  let exporting = $state(false);
  let saved = $state<string | null>(null);
  let selectedKey = $state('');
  let changesOnly = $state(true);
  let visibleCount = $state(100);
  let filterVersion = $state(0);
  let requestId = 0;
  let filterKey = '';
  let disposed = false;
  const generatedAt = $derived(snapshot.value?.generatedAt);
  const series = $derived(buildQuotaHistory(samples));
  const selected = $derived(series.find((s) => s.key === selectedKey) ?? series[0] ?? null);
  const selectedSeriesKey = $derived(selected?.key);
  const remaining = $derived(settings.value.percentMode === 'remaining');
  const entries = $derived(selected?.entries.filter((entry) => !changesOnly || entry.event !== 'unchanged' || entry.previous?.resetsAt !== entry.sample.resetsAt).reverse() ?? []);
  const number = (v: number) => new Intl.NumberFormat(intlLocale(), { maximumFractionDigits: 2 }).format(v);
  const percent = (v: number) => `${number(v)}%`;
  const timestamp = (value: string | null) => value && Number.isFinite(Date.parse(value)) ? new Date(value).toLocaleString(intlLocale()) : '—';
  const label = (s: QuotaHistorySeries) => [providerDisplayName(s.provider), kindLabel(s.kind, 'dashboard', s.scope !== null) || t('history.quota.other'), s.scope === '' ? '""' : s.scope].filter((v) => v !== null).join(' · ');

  async function load() {
    const id = ++requestId;
    const key = JSON.stringify([range?.from, live ? null : range?.to, provider, live]);
    if (key !== filterKey) {
      filterKey = key;
      filterVersion++;
      samples = [];
      saved = null;
    }
    error = null;
    if (!range) { samples = []; loading = false; return; }
    const query = { from: new Date(range.from).toISOString(), to: new Date(live ? Math.max(range.to, Date.now() + 1) : range.to).toISOString(), provider };
    loading = true;
    try {
      const result = await getQuotaHistory(query);
      if (!disposed && id === requestId) samples = result;
    } catch (e) {
      if (!disposed && id === requestId) error = String(e);
    } finally {
      if (!disposed && id === requestId) loading = false;
    }
  }

  $effect(() => {
    void [range, provider, live, generatedAt];
    untrack(() => { void load(); });
  });
  $effect(() => { void [selectedSeriesKey, changesOnly, filterVersion]; visibleCount = 100; });
  onDestroy(() => { disposed = true; requestId++; });

  async function exportCsv() {
    if (!selected || exporting) return;
    exporting = true; exportError = null; saved = null;
    const header = ['timestamp', 'provider', 'window', 'scope', 'used_percent', 'remaining_percent', 'change_percentage_points', 'event', 'resets_at', 'plan'];
    const rows = selected.entries.map(({ sample: s, change, event }) => [s.ts, s.provider, s.kind, s.scope ?? '', s.usedPercent, 100 - s.usedPercent, change ?? '', event, s.resetsAt ?? '', s.plan ?? '']);
    const csv = [header, ...rows].map((row) => row.map(csvCell).join(',')).join('\r\n') + '\r\n';
    try {
      const path = await exportUsageCsv(csv, `quota-${selected.provider}-${selected.kind}.csv`);
      if (!disposed && path) saved = path;
    } catch (e) { if (!disposed) exportError = String(e); }
    finally { if (!disposed) exporting = false; }
  }
</script>

<section class="card quota-panel" aria-label={t('history.quota.title')} aria-busy={loading}>
  <header>
    <div><h3>{t('history.quota.title')}</h3><p class="muted">{t('history.quota.scopeNote')}</p></div>
    {#if selected}
      <label class="window-picker">{t('history.quota.window')}
        <select class="field" value={selected.key} onchange={(event) => selectedKey = event.currentTarget.value}>
          {#each series as item (item.key)}<option value={item.key}>{label(item)}</option>{/each}
        </select>
      </label>
    {/if}
  </header>
  {#if error}<p class="error" role="alert">{error} <button class="btn" onclick={() => void load()}>{t('history.quota.retry')}</button></p>{/if}
  {#if selected}
    <dl class="quota-stats">
      <div><dt>{t(remaining ? 'history.quota.latestRemaining' : 'history.quota.latestUsed')}</dt><dd>{percent(remaining ? 100 - selected.lastUsedPercent : selected.lastUsedPercent)}</dd></div>
      <div><dt>{t('history.quota.increase')}</dt><dd>{t('history.quota.points', { n: number(selected.observedIncrease) })}</dd></div>
      <div><dt>{t('history.quota.resets')}</dt><dd>{number(selected.resetCount)}</dd></div>
      <div><dt>{t('history.quota.samples')}</dt><dd>{number(selected.samples.length)}</dd></div>
    </dl>
    <p class="muted range-note">{t(remaining ? 'history.quota.remaining' : 'history.quota.used')} · {timestamp(selected.samples[0].ts)} → {timestamp(selected.samples.at(-1)!.ts)}</p>
    <QuotaHistoryChart series={selected} {remaining} {themeKey} />
    <p class="muted">{t('history.quota.observedNote')}</p>
    <div class="detail-head">
      <h4>{t('history.quota.details')}</h4>
      <label class="changes-toggle"><input type="checkbox" bind:checked={changesOnly} />{t('history.quota.changesOnly')}</label>
      <button class="btn" onclick={() => void exportCsv()} disabled={exporting}>{t('history.quota.export')}</button>
    </div>
    <div class="quota-table-wrap">
      <table>
        <thead><tr><th>{t('history.quota.time')}</th><th>{t('history.quota.used')}</th><th>{t('history.quota.remaining')}</th><th>{t('history.quota.change')}</th><th>{t('history.quota.event')}</th><th>{t('history.quota.resetAt')}</th><th>{t('history.quota.plan')}</th></tr></thead>
        <tbody>
          {#each entries.slice(0, visibleCount) as entry (entry.sample.ts)}
            <tr>
              <td>{timestamp(entry.sample.ts)}</td><td class="num">{percent(entry.sample.usedPercent)}</td><td class="num">{percent(100 - entry.sample.usedPercent)}</td>
              <td class="num">{entry.change === null ? '—' : t('history.quota.points', { n: `${entry.change > 0 ? '+' : ''}${number(entry.change)}` })}</td>
              <td><span class:transition={entry.event === 'reset' || entry.event === 'plan_change'}>{entry.event === 'unchanged' && entry.previous?.resetsAt !== entry.sample.resetsAt ? t('history.quota.deadlineUpdated') : tDyn(`history.quota.event.${entry.event}`)}</span></td>
              <td>{timestamp(entry.sample.resetsAt)}</td><td>{entry.sample.plan ?? '—'}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <div class="detail-foot"><span class="muted">{t('history.quota.showing', { n: Math.min(visibleCount, entries.length), total: entries.length })}</span>
      {#if entries.length > visibleCount}<button class="btn" onclick={() => visibleCount += 100}>{t('history.quota.more')}</button>{/if}
    </div>
  {:else}
    <p class="empty muted">{loading ? t('history.quota.loading') : t('history.quota.empty')}</p>
  {/if}
  {#if saved}<p class="muted" role="status">{t('history.quota.saved', { path: saved })}</p>{/if}
  {#if exportError}<p class="error" role="alert">{exportError}</p>{/if}
</section>

<style>
  .quota-panel { padding: 1rem; display: flex; flex-direction: column; gap: 0.875rem; min-width: 0; }
  header, .detail-head, .detail-foot { display: flex; align-items: center; gap: 0.75rem; flex-wrap: wrap; }
  header { justify-content: space-between; }
  h3, h4, p, dl, dd { margin: 0; }
  h3 { font-size: 0.9375rem; } h4 { font-size: 0.8125rem; }
  .muted, .window-picker, .changes-toggle { color: var(--muted); font-size: 0.75rem; line-height: 1.6; }
  header p { margin-top: 0.25rem; }
  .window-picker { display: flex; align-items: center; gap: 0.5rem; min-width: 0; }
  .window-picker select { max-width: min(23rem, 65vw); }
  .quota-stats { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 1rem; }
  dt { color: var(--muted); font-size: 0.75rem; } dd { font-size: 1.375rem; font-variant-numeric: tabular-nums; margin-top: 0.3rem; }
  .range-note { margin-bottom: -0.375rem; }
  .detail-head { border-top: 1px solid var(--border); padding-top: 0.875rem; }
  .changes-toggle { display: flex; gap: 0.375rem; align-items: center; margin-left: auto; }
  input { accent-color: var(--focus); }
  .quota-table-wrap { overflow-x: auto; max-height: 27rem; }
  table { border-collapse: collapse; width: 100%; font-size: 0.75rem; white-space: nowrap; }
  th, td { padding: 0.5rem 0.625rem; text-align: left; border-bottom: 1px solid var(--border); }
  th { font-weight: 500; color: var(--muted); }
  .num { font-variant-numeric: tabular-nums; }
  .transition { color: var(--focus); }
  .detail-foot { justify-content: space-between; }
  .error { color: var(--danger, #ff6b6b); font-size: 0.8125rem; }
  .empty { padding: 1.5rem 0; text-align: center; }
  @media (max-width: 650px) { .quota-stats { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
