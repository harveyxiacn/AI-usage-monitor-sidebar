<!--
  Quota cycles of the selected window: peak used % per cycle (5-hour or
  weekly), how often the limit was reached, and what a percentage point of
  quota "costs" in tokens (from the local usage log). Estimates only. [FRONTEND]
-->
<script lang="ts">
  import { onDestroy } from 'svelte';
  import { getWindowUsage } from '$lib/api';
  import QuotaCycleChart from '$lib/components/QuotaCycleChart.svelte';
  import { formatCost, formatInt, formatTokens } from '$lib/format';
  import { intlLocale, t } from '$lib/i18n/i18n.svelte';
  import { buildCycles, limitReachedCount, tokensPerPercent, type CycleUsage } from '$lib/quota-cycles';
  import type { QuotaHistorySeries } from '$lib/quota-history';
  import { settings } from '$lib/stores/settings.svelte';

  interface Props { allSeries: QuotaHistorySeries[]; selected: QuotaHistorySeries; themeKey: string; generatedAt?: string }
  let { allSeries, selected, themeKey, generatedAt }: Props = $props();

  /** cap the table/chart so a year of 5-hour cycles stays readable and the backend query small */
  const MAX_CYCLES = 60;
  let kindChoice = $state<'five_hour' | 'seven_day' | null>(null);
  let totals = $state<Awaited<ReturnType<typeof getWindowUsage>>>([]);
  let totalsKey = '';
  let error = $state<string | null>(null);
  let requestId = 0;
  let disposed = false;

  const sameScope = $derived(allSeries.filter((s) => s.provider === selected.provider && s.account === selected.account && s.scope === selected.scope &&
    (s.kind === 'five_hour' || s.kind === 'seven_day')));
  const kinds = $derived(sameScope.map((s) => s.kind as 'five_hour' | 'seven_day'));
  const series = $derived(sameScope.find((s) => s.kind === kindChoice) ?? sameScope.find((s) => s.kind === selected.kind) ?? sameScope[0] ?? null);
  const cycles = $derived.by(() => {
    void generatedAt;
    return series ? buildCycles(series, Date.now()).slice(-MAX_CYCLES) : [];
  });
  const usages = $derived<CycleUsage[]>(cycles.map((cycle, i) => ({ cycle, totals: totals[i] ?? null })));
  const ratio = $derived(tokensPerPercent(usages));
  const limitCount = $derived(limitReachedCount(cycles));
  const thresholds = $derived(settings.value.thresholds);
  const withTime = $derived(series?.kind === 'five_hour');
  const number = (v: number) => new Intl.NumberFormat(intlLocale(), { maximumFractionDigits: 1 }).format(v);
  const stamp = (ms: number) => new Date(ms).toLocaleString(intlLocale(), withTime
    ? { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' } : { month: 'numeric', day: 'numeric' });
  const perPercent = (u: CycleUsage) => u.totals && u.cycle.peak >= 1 && u.totals.totalTokens > 0
    ? formatTokens(u.totals.totalTokens / u.cycle.peak) : '—';

  async function load(provider: string, account: string, list: typeof cycles) {
    const key = JSON.stringify([provider, account, list.map((c) => [c.start, c.end])]);
    if (key === totalsKey) return;
    totalsKey = key;
    const id = ++requestId;
    if (list.length === 0) { totals = []; return; }
    try {
      // the tokens of this very account ('' = the primary one)
      const result = await getWindowUsage({
        provider: provider as 'claude' | 'codex',
        windows: list.map((c) => ({ from: new Date(c.start).toISOString(), to: new Date(c.end).toISOString() })),
        account,
      });
      if (!disposed && id === requestId) { totals = result; error = null; }
    } catch (e) {
      if (!disposed && id === requestId) { totals = []; error = String(e); totalsKey = ''; }
    }
  }

  $effect(() => {
    const provider = series?.provider;
    const account = series?.account ?? '';
    const list = cycles;
    if (!provider) return;
    const timer = setTimeout(() => void load(provider, account, list), 120);
    return () => clearTimeout(timer);
  });
  onDestroy(() => { disposed = true; requestId++; });
</script>

{#if series && cycles.length > 0}
  <section class="cycles" aria-label={t('history.quota.cycles.title')}>
    <div class="head">
      <h4>{t('history.quota.cycles.title')}</h4>
      {#if kinds.length > 1}
        <div class="segmented" role="group" aria-label={t('history.quota.window')}>
          {#each kinds as kind (kind)}
            <button class:active={series.kind === kind} aria-pressed={series.kind === kind} onclick={() => (kindChoice = kind)}>
              {t(kind === 'five_hour' ? 'history.quota.cycles.fiveHour' : 'history.quota.cycles.weekly')}
            </button>
          {/each}
        </div>
      {/if}
    </div>
    <dl class="facts">
      <div><dt>{t('history.quota.cycles.count')}</dt><dd>{formatInt(cycles.length)}</dd></div>
      <div><dt>{t('history.quota.cycles.limitReached')}</dt><dd class:hit={limitCount > 0}>{t('history.quota.cycles.times', { n: formatInt(limitCount) })}</dd></div>
      <div>
        <dt>{t('history.quota.cycles.perPercent')}</dt>
        <dd>{#if ratio}≈ {formatTokens(ratio.tokens)}{#if ratio.costUsd != null} <span class="sub">(≈ {formatCost(ratio.costUsd)})</span>{/if}{:else}—{/if}</dd>
      </div>
    </dl>
    {#if ratio}<p class="muted">{t('history.quota.cycles.perPercentNote', { n: ratio.cycles })}</p>{/if}
    <QuotaCycleChart provider={series.provider} {usages} {thresholds} {withTime} {themeKey} />
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="table-wrap">
      <table>
        <thead><tr>
          <th>{t('history.quota.cycles.ended')}</th><th>{t('history.quota.peak')}</th><th>{t('history.quota.tokens')}</th>
          <th>{t('history.quota.cost')}</th><th>{t('history.quota.cycles.tokensPerPercent')}</th>
        </tr></thead>
        <tbody>
          {#each [...usages].reverse() as u (u.cycle.end)}
            <tr class:running={!u.cycle.completed}>
              <td>{stamp(u.cycle.end)}{#if !u.cycle.completed} <span class="tag">{t('history.quota.cycles.running')}</span>{:else if !u.cycle.reliable} <span class="tag" title={t('history.quota.cycles.partialHint')}>{t('history.quota.cycles.partial')}</span>{/if}</td>
              <td class="num" class:hit={u.cycle.peak >= thresholds.critical}>{number(u.cycle.peak)}%</td>
              <td class="num">{u.totals ? formatTokens(u.totals.totalTokens) : '—'}</td>
              <td class="num">{u.totals?.estimatedCostUsd != null ? `≈ ${formatCost(u.totals.estimatedCostUsd)}` : '—'}</td>
              <td class="num">{perPercent(u)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="muted">{t('history.quota.cycles.note')}</p>
  </section>
{/if}

<style>
  .cycles { display: flex; flex-direction: column; gap: 0.75rem; border-top: 1px solid var(--border); padding-top: 0.875rem; min-width: 0; }
  .head { display: flex; align-items: center; gap: 0.75rem; flex-wrap: wrap; justify-content: space-between; }
  h4, p, dl, dd { margin: 0; }
  h4 { font-size: 0.8125rem; }
  .muted, dt { color: var(--muted); font-size: 0.75rem; line-height: 1.6; }
  .facts { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 1rem; }
  dd { font-size: 1.25rem; font-variant-numeric: tabular-nums; margin-top: 0.25rem; }
  .sub { font-size: 0.75rem; color: var(--muted); }
  .hit { color: var(--critical); }
  .table-wrap { overflow-x: auto; max-height: 16rem; }
  table { border-collapse: collapse; width: 100%; font-size: 0.75rem; white-space: nowrap; }
  th, td { padding: 0.4rem 0.625rem; text-align: left; border-bottom: 1px solid var(--border); }
  th { font-weight: 500; color: var(--muted); }
  .num { font-variant-numeric: tabular-nums; }
  .running { color: var(--muted); }
  .tag { font-size: 0.6875rem; color: var(--muted); border: 1px solid var(--border); border-radius: 999px; padding: 0 0.375rem; }
  .segmented { display: inline-flex; padding: 0.125rem; gap: 0.125rem; border-radius: var(--r-control); background: var(--surface-2); border: 1px solid var(--border); }
  .segmented button { padding: 0.1875rem 0.625rem; border-radius: calc(var(--r-control) - 0.125rem); font-size: 0.8125rem; color: var(--muted); white-space: nowrap; }
  .segmented button:hover { color: var(--text); }
  .segmented button.active { background: var(--surface); color: var(--text); box-shadow: var(--shadow-card); }
  .error { color: var(--danger, #ff6b6b); font-size: 0.8125rem; }
  @media (max-width: 650px) { .facts { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
