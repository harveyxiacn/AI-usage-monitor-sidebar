<!--
  The dark "speech bubble" of the popover window. [FRONTEND]

  Layout maths worth knowing:
  * The bubble is wrapped in `.root`, which carries the tail-side padding
    (TAIL_W). The popover route measures `.root` with a ResizeObserver and sends
    that size to `popover_relayout`, so the Rust window is exactly big enough for
    the bubble *plus* its tail — no clipping, no dead transparent margin.
  * `--tail-y` is the y of the tail tip inside the bubble (default 50%). The
    platform layer can align it with the ring by resizing/moving the window; the
    route also sets it from PopoverRequest.anchorY when it knows its own height.
-->
<script lang="ts">
  import ProviderLogo from './ProviderLogo.svelte';
  import WindowRow from './WindowRow.svelte';
  import { accentFor } from '$lib/stores/rings.svelte';
  import { formatAgo } from '$lib/format';
  import { t, tDyn, hasKey } from '$lib/i18n/i18n.svelte';
  import type { Edge, PercentMode, ProviderQuota, Thresholds, WindowKind } from '$lib/types';

  interface Props {
    quota: ProviderQuota;
    edge: Edge;
    thresholds: Thresholds;
    percentMode?: PercentMode;
    /** window to mark with a dot, from PopoverRequest.windowKind */
    highlightKind?: WindowKind | null;
    /** 0..100, vertical position of the tail tip */
    tailPercent?: number;
    now?: number;
    onDetails?: () => void;
  }

  let {
    quota,
    edge,
    thresholds,
    percentMode = 'used',
    highlightKind = null,
    tailPercent = 50,
    now = Date.now(),
    onDetails,
  }: Props = $props();

  /** primary first, then the remaining account-wide windows in backend order */
  const mainWindows = $derived(
    quota.windows
      .filter((w) => w.scope == null)
      .sort((a, b) => Number(b.isPrimary) - Number(a.isPrimary))
  );
  const scopedWindows = $derived(quota.windows.filter((w) => w.scope != null));

  let showMore = $state(false);

  const statusHint = $derived.by(() => {
    if (quota.status === 'ok') return null;
    if (quota.error) return quota.error;
    const specific = `status.hint.${quota.provider}.${quota.status}`;
    const generic = `status.hint.${quota.status}`;
    if (hasKey(specific)) return tDyn(specific);
    if (hasKey(generic)) return tDyn(generic);
    return tDyn(`status.${quota.status}`);
  });

  const creditsLine = $derived.by(() => {
    const c = quota.credits;
    if (!c) return null;
    if (c.unlimited) return t('popover.creditsUnlimited');
    if (c.hasCredits || (c.balance && c.balance !== '0'))
      return t('popover.creditsBalance', { balance: c.balance ?? '—' });
    return t('popover.noCredits');
  });
</script>

<div class="root" data-edge={edge} style:--tail-y={`${Math.min(92, Math.max(8, tailPercent))}%`}>
  <div class="bubble surface">
    <header>
      <span class="logo" style:color={accentFor(quota.provider, 0)}>
        <ProviderLogo provider={quota.provider} size={24} />
      </span>
      <h1>{t('popover.title', { provider: quota.displayName })}</h1>
      {#if quota.planLabel}
        <span class="plan">{quota.planLabel}</span>
      {/if}
    </header>

    {#if mainWindows.length === 0}
      <p class="empty">{t('status.noWindows')}</p>
    {:else}
      <div class="rows">
        {#each mainWindows as w, i (w.kind + ':' + i)}
          <WindowRow
            window={w}
            accent={accentFor(quota.provider, i)}
            {thresholds}
            {percentMode}
            {now}
            context="popover"
            highlight={highlightKind != null && w.kind === highlightKind}
          />
        {/each}
      </div>
    {/if}

    {#if scopedWindows.length > 0}
      <button
        class="more"
        aria-expanded={showMore}
        onclick={() => (showMore = !showMore)}
      >
        <span class="chev" class:open={showMore} aria-hidden="true">›</span>
        {t('popover.moreLimits')}
        <span class="count">{scopedWindows.length}</span>
      </button>
      {#if showMore}
        <div class="rows scoped">
          {#each scopedWindows as w, i (w.label + ':' + i)}
            <WindowRow
              window={w}
              accent={accentFor(quota.provider, 1)}
              {thresholds}
              {percentMode}
              {now}
              context="popover"
              compact
            />
          {/each}
        </div>
      {/if}
    {/if}

    {#if creditsLine}
      <div class="line">
        <span class="k">{t('popover.credits')}</span>
        <span class="v">{creditsLine}</span>
      </div>
    {/if}

    {#if statusHint}
      <p class="status" class:bad={quota.status === 'error'}>{statusHint}</p>
    {/if}

    <footer>
      <span class="updated"
        >{t('popover.updated', {
          ago: formatAgo(quota.fetchedAt, now),
          source: tDyn(`source.${quota.source}`),
        })}</span
      >
      <button class="details" onclick={() => onDetails?.()}>{t('popover.details')}</button>
    </footer>
  </div>
  <span class="tail" aria-hidden="true"></span>
</div>

<style>
  /* TAIL_W = 0.5rem — kept in sync with the padding below and the route's
     measurement of `.root`. */
  .root {
    position: relative;
    width: max-content;
    max-width: 22.5rem; /* 360px @ scale 1 */
    min-width: 19rem; /* ≈ the reference bubble; keeps short labels from producing a cramped card */
  }

  .root[data-edge='right'] {
    padding-right: 0.5rem;
  }

  .root[data-edge='left'] {
    padding-left: 0.5rem;
  }

  /* background / border / shadow / glass layers come from the global
     `.surface` material in base.css — identical to the sidebar pill */
  .bubble {
    border-radius: var(--r-bubble);
    padding: 0.875rem 1rem 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .tail {
    position: absolute;
    top: var(--tail-y, 50%);
    transform: translateY(-50%);
    width: 0;
    height: 0;
    border-top: 0.5rem solid transparent;
    border-bottom: 0.5rem solid transparent;
    /* a filter shadow keeps the beak visually attached to the bubble */
    filter: drop-shadow(0 0.125rem 0.25rem rgb(0 0 0 / 0.3));
  }

  .root[data-edge='right'] .tail {
    right: 0.0625rem;
    border-left: 0.5rem solid var(--surface-fill);
  }

  .root[data-edge='left'] .tail {
    left: 0.0625rem;
    border-right: 0.5rem solid var(--surface-fill);
  }

  header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .logo {
    display: grid;
    place-items: center;
    flex: none;
  }

  h1 {
    margin: 0;
    font-size: 1.0625rem; /* 17px */
    font-weight: 600;
    letter-spacing: -0.01em;
    white-space: nowrap;
  }

  .plan {
    margin-left: auto;
    flex: none;
    font-size: 0.6875rem;
    font-weight: 500;
    color: var(--muted);
    padding: 0.125rem 0.4375rem;
    border-radius: 999px;
    border: 1px solid var(--bar-border);
    background: var(--hover);
    white-space: nowrap;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 0.875rem;
  }

  .rows.scoped {
    gap: 0.625rem;
    padding-left: 0.5rem;
    border-left: 2px solid var(--surface-track);
  }

  .more {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0;
    font-size: 0.8125rem;
    color: var(--muted);
    transition: color var(--dur-ui) var(--ease-out);
  }

  .more:hover {
    color: var(--text);
  }

  .chev {
    display: inline-block;
    transition: transform var(--dur-ui) var(--ease-out);
    font-size: 1rem;
    line-height: 1;
  }

  .chev.open {
    transform: rotate(90deg);
  }

  .count {
    font-size: 0.6875rem;
    padding: 0 0.3125rem;
    border-radius: 999px;
    background: var(--surface-track);
    color: var(--muted);
  }

  .line {
    display: flex;
    justify-content: space-between;
    gap: 0.75rem;
    font-size: 0.8125rem;
  }

  .line .k {
    color: var(--muted);
  }

  .status {
    margin: 0;
    font-size: 0.75rem;
    line-height: 1.4;
    color: var(--warn);
  }

  .status.bad {
    color: var(--critical);
  }

  .empty {
    margin: 0;
    font-size: 0.8125rem;
    color: var(--muted);
  }

  footer {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--bar-border);
    font-size: 0.6875rem;
    color: var(--faint);
  }

  .details {
    padding: 0;
    font-size: 0.6875rem;
    font-weight: 500;
    color: var(--muted);
    white-space: nowrap;
    transition: color var(--dur-ui) var(--ease-out);
  }

  .details:hover {
    color: var(--text);
  }
</style>
