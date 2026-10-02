<!--
  History → Cost & budget → Plan advisor: per provider, how often the limits
  were reached over the last N weeks, how heavy a typical cycle is and what the
  usage would cost at API prices versus the subscription price the user typed in
  → "upgrade / downgrade / keep / not enough data", each with its reasons.
  The rules live in `$lib/plan-advisor`; prices are hints, never billing facts. [FRONTEND]
-->
<script lang="ts">
  import { onDestroy } from 'svelte';
  import { getQuotaHistory, getUsageHistory } from '$lib/api';
  import { formatCost } from '$lib/format';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { advisePlan, type PlanAdvice, type PlanReason } from '$lib/plan-advisor';
  import { buildCycles } from '$lib/quota-cycles';
  import { buildQuotaHistory } from '$lib/quota-history';
  import { providerDisplayName } from '$lib/providers';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import type { ProviderId } from '$lib/types';
  import Segmented from './Segmented.svelte';

  interface Props {
    /** history provider filter; null = every provider */
    provider: ProviderId | null;
    /** bumps when new usage was ingested */
    refreshKey: number;
  }
  let { provider, refreshKey }: Props = $props();

  const WEEK_OPTIONS = [4, 8, 12] as const;
  let weeks = $state<(typeof WEEK_OPTIONS)[number]>(8);
  let advices = $state<PlanAdvice[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let requestId = 0;
  let disposed = false;

  /** primary accounts only: extra accounts have no token log to price */
  const targets = $derived(
    (snapshot.value?.providers ?? [])
      .filter((q) => !q.accountId && (!provider || q.provider === provider))
      .map((q) => ({ id: q.provider, name: q.displayName, plan: q.planLabel ?? q.plan ?? null }))
  );
  const targetKey = $derived(JSON.stringify(targets));
  const prices = $derived(settings.value.subscriptionUsd);
  const weekOptions = $derived(WEEK_OPTIONS.map((n) => [String(n), t('history.plan.weeksN', { n })] as const));

  async function load() {
    const id = ++requestId;
    const list = targets;
    const to = Date.now();
    const from = to - weeks * 7 * 86_400_000;
    try {
      const out: PlanAdvice[] = [];
      for (const target of list) {
        const [samples, usage] = await Promise.all([
          getQuotaHistory({ from: new Date(from).toISOString(), to: new Date(to).toISOString(), provider: target.id, account: '' }),
          getUsageHistory({ from: new Date(from).toISOString(), to: new Date(to).toISOString(), bucket: 'month', groupByModel: false, provider: target.id }),
        ]);
        const series = buildQuotaHistory(samples).filter((s) => s.scope === null && !s.account);
        const cyclesOf = (kind: 'five_hour' | 'seven_day') => {
          const found = series.find((s) => s.kind === kind);
          return found ? buildCycles(found, to) : [];
        };
        out.push(advisePlan({
          provider: target.id,
          planLabel: target.plan,
          subscriptionUsd: prices[target.id] ?? 0,
          weeklyCycles: cyclesOf('seven_day'),
          fiveHourCycles: cyclesOf('five_hour'),
          apiCostUsd: usage.totals.estimatedCostUsd,
          periodDays: weeks * 7,
        }));
      }
      if (disposed || id !== requestId) return;
      advices = out;
      error = null;
      loaded = true;
    } catch (e) {
      if (!disposed && id === requestId) error = String(e);
    }
  }

  $effect(() => {
    void [targetKey, weeks, refreshKey, JSON.stringify(prices)];
    const timer = setTimeout(() => void load(), 150);
    return () => clearTimeout(timer);
  });
  onDestroy(() => { disposed = true; requestId++; });

  function reasonText(r: PlanReason): string {
    const params: Record<string, string | number> = { ...r.params };
    if (typeof params.window === 'string') params.window = tDyn(`history.plan.window.${params.window}`);
    if (typeof params.cost === 'number') params.cost = formatCost(params.cost);
    if (typeof params.price === 'number') params.price = formatCost(params.price);
    return tDyn(`history.plan.reason.${r.code}`, params);
  }

  function tierHint(a: PlanAdvice): string | null {
    if (a.verdict === 'upgrade') {
      return a.suggestedTier
        ? t('history.plan.hint.next', { tier: a.suggestedTier.name, price: formatCost(a.suggestedTier.priceUsd) })
        : a.currentTier ? t('history.plan.hint.top') : null;
    }
    if (a.verdict === 'downgrade' && a.suggestedTier) {
      return t('history.plan.hint.prev', { tier: a.suggestedTier.name, price: formatCost(a.suggestedTier.priceUsd) });
    }
    return null;
  }
</script>

{#if targets.length > 0}
  <div class="card plan" aria-label={t('history.plan.title')}>
    <header>
      <h3>{t('history.plan.title')}</h3>
      <Segmented
        options={weekOptions}
        value={String(weeks)}
        label={t('history.plan.weeks')}
        onchange={(v) => (weeks = Number(v) as (typeof WEEK_OPTIONS)[number])}
      />
    </header>

    {#if !loaded && !error}
      <p class="muted" role="status">{t('common.loading')}</p>
    {/if}
    {#each advices as a (a.provider)}
      {@const target = targets.find((x) => x.id === a.provider)}
      {@const hint = tierHint(a)}
      <section class="row" data-verdict={a.verdict} aria-label={providerDisplayName(a.provider)}>
        <div class="head">
          <strong>{target?.name ?? providerDisplayName(a.provider)}</strong>
          {#if target?.plan}<span class="muted">{t('history.plan.current', { plan: target.plan })}</span>{/if}
          <span class="verdict" data-verdict={a.verdict}>{tDyn(`history.plan.verdict.${a.verdict}`)}</span>
        </div>
        <ul>
          {#each a.reasons as r, i (r.code + i)}
            <li>{reasonText(r)}</li>
          {/each}
        </ul>
        {#if hint}<p class="muted hint">{hint}</p>{/if}
      </section>
    {:else}
      {#if loaded}<p class="muted">{t('history.plan.empty')}</p>{/if}
    {/each}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <p class="muted note">{t('history.plan.note')}</p>
  </div>
{/if}

<style>
  .plan { display: flex; flex-direction: column; gap: 0.75rem; padding: 1rem; min-width: 0; }
  header { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 0.75rem; }
  h3, p, ul { margin: 0; }
  h3 { font-size: 0.9375rem; }
  .row { display: flex; flex-direction: column; gap: 0.375rem; padding-top: 0.75rem; border-top: 1px solid var(--border); }
  .head { display: flex; align-items: baseline; flex-wrap: wrap; gap: 0.625rem; font-size: 0.8125rem; }
  .verdict { margin-left: auto; font-size: 0.75rem; font-weight: 600; padding: 0.0625rem 0.5rem; border-radius: 999px; border: 1px solid var(--border-strong); }
  .verdict[data-verdict='upgrade'] { color: var(--warn); }
  .verdict[data-verdict='downgrade'] { color: var(--focus); }
  .verdict[data-verdict='insufficient'] { color: var(--muted); }
  ul { padding-left: 1.125rem; font-size: 0.8125rem; line-height: 1.65; }
  .muted { color: var(--muted); font-size: 0.75rem; }
  .hint, .note { line-height: 1.6; }
  .error { color: var(--danger, #ff6b6b); font-size: 0.8125rem; }
</style>
