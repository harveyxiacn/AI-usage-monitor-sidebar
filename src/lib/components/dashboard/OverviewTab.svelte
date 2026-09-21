<!--
  Dashboard → Overview. One card per provider: identity, every rate-limit
  window with its reset time, credits, status and a 7-day trend line per
  non-scoped window (from `get_quota_history`). [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import ProviderLogo from '$lib/components/ProviderLogo.svelte';
  import QuotaExtras from '$lib/components/QuotaExtras.svelte';
  import Sparkline from '$lib/components/Sparkline.svelte';
  import WindowRow from '$lib/components/WindowRow.svelte';
  import { getQuotaHistory } from '$lib/api';
  import { formatAgo, windowLabel } from '$lib/format';
  import { hasKey, t, tDyn } from '$lib/i18n/i18n.svelte';
  import { accentFor } from '$lib/stores/rings.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import type { ProviderId, ProviderQuota, QuotaSample } from '$lib/types';

  const s = $derived(settings.value);
  /** ticks so "Resets in …" / "Updated … ago" stay honest without a refetch */
  let now = $state(Date.now());
  let samples = $state<QuotaSample[]>([]);
  let historyError = $state<string | null>(null);
  let historyRequest = 0;

  const providers = $derived.by(() => {
    const list = snapshot.value?.providers ?? [];
    return [...list].sort(
      (a, b) => (s.providers[a.provider]?.order ?? 0) - (s.providers[b.provider]?.order ?? 0)
    );
  });

  /** provider + kind + scope → chronological used-percent series */
  const series = $derived.by(() => {
    const map = new Map<string, number[]>();
    for (const sample of [...samples].sort((a, b) => Date.parse(a.ts) - Date.parse(b.ts))) {
      const key = `${sample.provider}|${sample.kind}|${sample.scope ?? ''}`;
      const arr = map.get(key);
      if (arr) arr.push(sample.usedPercent);
      else map.set(key, [sample.usedPercent]);
    }
    return map;
  });

  function trend(provider: ProviderId, kind: string, scope: string | null): number[] {
    return series.get(`${provider}|${kind}|${scope ?? ''}`) ?? [];
  }

  async function loadQuotaHistory() {
    const id = ++historyRequest;
    const to = Date.now();
    try {
      const next = await getQuotaHistory({
        from: new Date(to - 7 * 86_400_000).toISOString(),
        to: new Date(to).toISOString(),
        provider: null,
      });
      if (id === historyRequest) { samples = next; historyError = null; }
    } catch (e) {
      if (id === historyRequest) historyError = String(e);
    }
  }

  onMount(() => {
    void loadQuotaHistory();
    const timer = setInterval(() => (now = Date.now()), 15_000);
    return () => { historyRequest++; clearInterval(timer); };
  });

  function statusHint(q: ProviderQuota): string | null {
    if (q.status === 'ok') return null;
    if (q.error) return q.error;
    const specific = `status.hint.${q.provider}.${q.status}`;
    if (hasKey(specific)) return tDyn(specific);
    const generic = `status.hint.${q.status}`;
    return hasKey(generic) ? tDyn(generic) : tDyn(`status.${q.status}`);
  }

  function creditsLine(q: ProviderQuota): string | null {
    const c = q.credits;
    if (!c) return null;
    if (c.unlimited) return t('popover.creditsUnlimited');
    if (c.hasCredits || (c.balance && c.balance !== '0'))
      return t('popover.creditsBalance', { balance: c.balance ?? '—' });
    return t('popover.noCredits');
  }

  async function refresh(provider?: ProviderId) {
    await snapshot.refresh(provider);
    await loadQuotaHistory();
    now = Date.now();
  }
</script>

<section class="overview">
  <header class="bar">
    <h2>{t('tab.overview')}</h2>
    <button
      class="btn"
      disabled={snapshot.refreshing !== null}
      onclick={() => void refresh()}
    >
      {snapshot.refreshing === 'all' ? t('common.refreshing') : t('common.refreshAll')}
    </button>
  </header>

  {#if snapshot.error}<p class="hint bad" role="alert">{t('common.error', { message: snapshot.error })}</p>{/if}
  {#if historyError}<p class="hint bad" role="alert">{t('overview.historyError', { message: historyError })}</p>{/if}

  {#if snapshot.loading}
    <p class="muted">{t('common.loading')}</p>
  {:else if providers.length === 0}
    <p class="muted">{t('overview.noProviders')}</p>
  {:else}
    <div class="grid">
      {#each providers as q (q.provider)}
        {@const accent = accentFor(q.provider, 0)}
        {@const hint = statusHint(q)}
        {@const credits = creditsLine(q)}
        <article class="card provider">
          <header class="head">
            <span class="logo" style:color={accent}>
              <ProviderLogo provider={q.provider} size={26} />
            </span>
            <div class="who">
              <span class="name">{q.displayName}</span>
              <span class="sub">
                {q.planLabel ?? q.plan ?? '—'}{q.account?.email ? ` · ${q.account.email}` : ''}
              </span>
            </div>
            <span class="dot" data-status={q.status} title={tDyn(`status.${q.status}`)}></span>
            <button
              class="btn"
              disabled={snapshot.refreshing !== null}
              onclick={() => void refresh(q.provider)}
            >
              {snapshot.refreshing === q.provider ? t('common.refreshing') : t('common.refresh')}
            </button>
          </header>

          {#if hint}
            <p class="hint" class:bad={q.status === 'error'}>{hint}</p>
          {/if}

          {#if q.windows.length === 0}
            <p class="muted">{t('status.noWindows')}</p>
          {:else}
            <div class="windows">
              {#each q.windows as w, i (w.label + ':' + i)}
                {@const line = w.scope == null ? trend(q.provider, w.kind, null) : []}
                <div class="win">
                  <WindowRow
                    window={w}
                    accent={accentFor(q.provider, w.isPrimary ? 0 : 1)}
                    thresholds={s.thresholds}
                    percentMode={s.percentMode}
                    context="dashboard"
                    label={w.scope ? windowLabel(w, 'dashboard') : undefined}
                    {now}
                  />
                  {#if w.scope == null}
                    <div class="trend">
                      <span class="trend-label">{t('overview.trend7d')}</span>
                      {#if line.length > 1}
                        <Sparkline
                          values={line}
                          color={accentFor(q.provider, w.isPrimary ? 0 : 1)}
                          height={26}
                          label={`${windowLabel(w, 'dashboard')} — ${t('overview.trend7d')}`}
                        />
                      {:else}
                        <span class="muted small">{t('overview.noQuotaHistory')}</span>
                      {/if}
                    </div>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}

          <QuotaExtras extras={q.extras} context="dashboard" />

          <footer class="foot">
            {#if credits}
              <span class="credits">{t('overview.credits')}: {credits}</span>
            {/if}
            <span class="fetched">
              {t('overview.lastFetched', {
                ago: formatAgo(q.fetchedAt, now),
                source: tDyn(`source.${q.source}`),
              })}
            </span>
          </footer>
        </article>
      {/each}
    </div>
  {/if}
</section>

<style>
  .overview {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }

  h2 {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
  }

  .grid {
    display: grid;
    /* two cards side by side on a wide window, one column below ~880px */
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 22rem), 1fr));
    gap: 1rem;
  }

  .provider {
    display: flex;
    flex-direction: column;
    gap: 0.875rem;
    padding: 1rem;
    min-width: 0;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 0.625rem;
  }

  .logo {
    display: grid;
    place-items: center;
    width: 2.25rem;
    height: 2.25rem;
    border-radius: 999px;
    background: var(--surface-2);
    flex: none;
  }

  .who {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1 1 auto;
  }

  .name {
    font-weight: 600;
  }

  .sub {
    font-size: 0.75rem;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 999px;
    background: var(--ok);
    flex: none;
  }

  .dot[data-status='not_logged_in'],
  .dot[data-status='token_expired'] {
    background: var(--warn);
  }

  .dot[data-status='error'] {
    background: var(--critical);
  }

  .dot[data-status='disabled'] {
    background: var(--faint);
  }

  .hint {
    margin: 0;
    font-size: 0.75rem;
    color: var(--warn);
  }

  .hint.bad {
    color: var(--critical);
  }

  .windows {
    display: flex;
    flex-direction: column;
    gap: 0.875rem;
  }

  .win {
    display: flex;
    flex-direction: column;
    gap: 0.375rem;
  }

  .trend {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .trend-label {
    font-size: 0.6875rem;
    color: var(--faint);
    white-space: nowrap;
    flex: none;
  }

  .trend :global(svg) {
    flex: 1 1 auto;
  }

  .foot {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
    flex-wrap: wrap;
    padding-top: 0.625rem;
    border-top: 1px solid var(--border);
    font-size: 0.6875rem;
    color: var(--faint);
  }

  .small {
    font-size: 0.6875rem;
  }
</style>
