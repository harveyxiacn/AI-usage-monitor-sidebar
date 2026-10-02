<!--
  Window "popover" (route "/popover"). [FRONTEND]

  Transparent + frameless like the sidebar. The window is hidden by default; the
  platform layer emits `popover-target` right before showing it, which tells us
  which provider (and optionally which window) to render.

  * `observeSize` reports the bubble's size (tail included, see Popover.svelte)
    to `popover_relayout`, which resizes the window and re-anchors it next to
    the ring — so the bubble is never clipped and never leaves dead transparent
    margin that would eat clicks.
  * hover is reported to Rust as source 'popover' so moving the pointer from the
    bar into the bubble does not close it.
  * Outside Tauri there is no platform layer to emit `popover-target`, so the
    first provider of the snapshot is rendered as a preview.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Popover from '$lib/components/Popover.svelte';
  import { HEARTBEAT_MS, observeSize, throttle } from '$lib/actions';
  import {
    hoverReport,
    isTauri,
    onPopoverTarget,
    onSidebarState,
    openDashboard,
    popoverHide,
    popoverRelayout,
    getQuotaHistory,
    type Unlisten,
  } from '$lib/api';
  import { quotaKey, splitQuotaKey } from '$lib/providers';
  import { SPARK_SPAN_MS, SparkCache } from '$lib/quota-spark';
  import { nextTickDelay } from '$lib/countdown';
  import { t } from '$lib/i18n/i18n.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import { applyTheme, markWindow } from '$lib/stores/theme.svelte';
  import type { PopoverRequest, QuotaSample } from '$lib/types';

  // stamped before the first applyTheme() effect so the theme store knows
  // which window it is (the custom text colour is widget-only)
  markWindow('popover');

  const s = $derived(settings.value);

  let target = $state<PopoverRequest | null>(null);
  /** re-evaluated on a self-scheduled tick so "Resets in 51 min" / "Updated 12 s ago" count down */
  let now = $state(Date.now());
  /** mirrors the sidebar's pin; a pinned popover shows a close button */
  let pinned = $state(false);

  const quota = $derived.by(() => {
    const providers = snapshot.value?.providers ?? [];
    if (providers.length === 0) return null;
    if (target) return providers.find((p) => quotaKey(p) === target!.provider) ?? null;
    // browser preview / first paint before the platform picked a target
    return isTauri() ? null : providers[0];
  });

  /**
   * ~24 h of quota samples for the row sparklines. Fetched when the popover is
   * shown for a provider (every `popover-target` event), not on snapshot
   * updates, and cached for a minute so hovering back and forth costs nothing.
   */
  const sparkCache = new SparkCache();
  let history = $state<QuotaSample[]>([]);
  /** `claude` or `claude@work`: the cache key and, split, the history query */
  const provider = $derived(quota ? quotaKey(quota) : null);
  $effect(() => {
    void target; // a fresh show request re-checks the cache
    const id = provider;
    if (!id) {
      history = [];
      return;
    }
    let cancelled = false;
    const at = Date.now();
    sparkCache
      .get(id, () =>
        // `account: ''` = this provider's primary account only, never a mix with the extra ones
        getQuotaHistory({ from: new Date(at - SPARK_SPAN_MS).toISOString(), to: new Date(at + 60_000).toISOString(), provider: splitQuotaKey(id).provider, account: splitQuotaKey(id).account })
      )
      .then((samples) => {
        if (!cancelled) history = samples;
      })
      .catch(() => {
        if (!cancelled) history = [];
      });
    return () => {
      cancelled = true;
    };
  });

  onMount(() => {
    const disposers: Array<() => void> = [settings.init(), snapshot.init()];
    let un: Unlisten | null = null;
    let unState: Unlisten | null = null;
    let disposed = false;
    void onPopoverTarget((req) => {
      target = req;
      now = Date.now();
    }).then((u) => (disposed ? u() : (un = u)));
    void onSidebarState((state) => (pinned = state.pinned)).then((u) => (disposed ? u() : (unState = u)));
    return () => {
      disposed = true;
      un?.();
      unState?.();
      disposers.forEach((d) => d());
    };
  });

  /**
   * One timer, re-armed after every tick: once a second only while the "updated
   * … ago" label counts seconds, otherwise at the next minute boundary of
   * whatever is on screen (nothing here shows seconds after that, and a
   * webview that repaints every second is wasted work in a tray app).
   */
  $effect(() => {
    const q = quota;
    const anchors = q
      ? [...q.windows.map((w) => Date.parse(w.resetsAt ?? '')), Date.parse(q.nextAttemptAt ?? '')]
      : [];
    const fetched = q ? Date.parse(q.fetchedAt ?? '') : null;
    let timer: ReturnType<typeof setTimeout>;
    const arm = () => {
      timer = setTimeout(() => {
        now = Date.now();
        arm();
      }, nextTickDelay(Date.now(), anchors, fetched));
    };
    arm();
    return () => clearTimeout(timer);
  });

  /**
   * Esc closes a pinned popover. The popover window never takes focus (it must
   * not steal it from the editor), so this only fires once the user has focused
   * it themselves; the keyboard route from the bar is Enter on the focused ring,
   * which toggles the pin there.
   */
  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') void popoverHide();
  }

  $effect(() => applyTheme(settings.value));

  /** "still here" while the pointer moves over the popover, see HEARTBEAT_MS */
  const heartbeat = throttle(() => void hoverReport('popover', true), HEARTBEAT_MS);
</script>

<svelte:head><title>{t('app.name')}</title></svelte:head>

<svelte:window onkeydown={onKeydown} />

<div
  class="stage"
  use:observeSize={(size) => void popoverRelayout(size.width, size.height)}
  oncontextmenu={(e) => e.preventDefault()}
  onmouseenter={() => void hoverReport('popover', true)}
  onmouseleave={() => void hoverReport('popover', false)}
  onmousemove={heartbeat}
  onwheel={heartbeat}
  role="presentation"
>
  {#if quota}
    <Popover
      {quota}
      edge={s.edge}
      thresholds={s.thresholds}
      percentMode={s.percentMode}
      highlightKind={target?.windowKind ?? null}
      {now}
      {history}
      {pinned}
      paused={s.pollingPaused}
      onClose={() => void popoverHide()}
      onDetails={() => void openDashboard('history')}
    />
  {:else}
    <div class="placeholder surface">{snapshot.loading ? t('popover.loading') : t('popover.empty')}</div>
  {/if}
</div>

<style>
  .stage {
    width: max-content;
    height: max-content;
    max-width: 45rem; /* the bubble limits itself: 360px + tail, 44rem when two columns */
  }

  .placeholder {
    padding: 0.75rem 1rem;
    border-radius: var(--r-bubble);
    color: var(--muted);
    white-space: nowrap;
  }
</style>
