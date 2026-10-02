<!--
  Window "sidebar" (route "/"). [FRONTEND]

  A transparent, frameless window that contains exactly one painted element:
  the near-black pill hugging the screen edge (or, when auto-hide collapsed the
  bar, a thin coloured handle).

  Window protocol
  ---------------
  * `observeSize` measures the painted element and reports its CSS px size to
    `sidebar_relayout` (debounced 50 ms) — the Rust side resizes the window to
    exactly that and re-anchors it to the configured edge. Everything is
    `width: max-content` / auto height so the measured size never depends on the
    window size (no resize feedback loop).
  * Hovering the bar reports `hover_report('bar', true|false)`; Rust owns the
    expand/collapse + popover-hide timers.
  * Dragging the pill (past a 5px threshold) streams `sidebar_drag`; Rust moves
    the window and snaps it to the nearest edge of the drop monitor on release.
  * Hovering a ring asks for the popover with the ring's centre in CSS px
    relative to *this* window — which is the viewport, so
    `rect.top + rect.height / 2` (and `rect.left + rect.width / 2`) is already
    the right number. Both are sent; Rust picks the one along the bar's axis.

  Orientation
  -----------
  `settings.edge` decides it: `left`/`right` stack the rings in a column (the
  original pill), `top`/`bottom` lay them out in a row. Only the stage's
  `data-edge` attribute drives the CSS — the flush side loses its border and
  its two corners there, and the "⋯" button moves next to the rings.
-->
<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import MiniBar from '$lib/components/MiniBar.svelte';
  import Ring from '$lib/components/Ring.svelte';
  import ProviderLogo from '$lib/components/ProviderLogo.svelte';
  import { dragHandle, HEARTBEAT_MS, observeSize, throttle, type DragPhase, type SizeReport } from '$lib/actions';
  import {
    hoverReport,
    onSidebarState,
    openDashboard,
    popoverHide,
    popoverSetPinned,
    popoverShow,
    sidebarDrag,
    sidebarRelayout,
    type Unlisten,
  } from '$lib/api';
  import { formatForecast, formatPercent, formatReset, severityOf, shortPercent, windowLabel } from '$lib/format';
  import { compactReset, nextTickDelay } from '$lib/countdown';
  import { forecastTickPercent } from '$lib/forecast';
  import { detectRingEvents, levelsOf, windowKey, type RingEvent, type RingEventKind, type WindowLevel } from '$lib/ring-events';
  import { handlePeak, handleSegments, labelLayout, type LabelLayout } from '$lib/sidebar-visuals';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { handleColorOf, rings, type RingItem } from '$lib/stores/rings.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import { applyTheme, markWindow } from '$lib/stores/theme.svelte';

  // stamped before the first applyTheme() effect so the theme store knows
  // which window it is (the custom text colour is widget-only)
  markWindow('sidebar');

  const s = $derived(settings.value);
  const items = $derived(rings.items);
  const loading = $derived(snapshot.value === null || !settings.loaded);
  /**
   * Placeholder rings while the first snapshot is on its way: as many as the
   * settings say will be drawn, so the bar does not jump in size when the real
   * rings replace them.
   */
  const placeholders = $derived(
    Array.from(
      { length: Math.max(1, Object.values(s.providers).filter((p) => p.enabled && p.showInSidebar).length) },
      (_, i) => i
    )
  );

  /** platform-owned expand/collapse state; only meaningful when autoHide is on */
  let expanded = $state(true);
  let expandedSize = $state<SizeReport>({ width: 76, height: 160 });
  /** ring key of the pinned popover, null when nothing is pinned */
  let pinnedKey = $state<string | null>(null);

  const collapsed = $derived(s.autoHide && !expanded);
  /** compact mode: slim progress bars instead of rings */
  const compact = $derived(s.ringStyle === 'bar');
  /** top/bottom edges: the bar is a horizontal strip, rings laid out in a row */
  const horizontal = $derived(s.edge === 'top' || s.edge === 'bottom');

  function reportSize(size: SizeReport) {
    if (!collapsed) expandedSize = size;
    // A collapsed handle must not overwrite the size to restore on hover.
    void sidebarRelayout(expandedSize.width, expandedSize.height);
  }

  /**
   * Colour of the collapsed handle: the worst threshold reached by any window,
   * in the accent of the provider closest to its limit, so the sliver still
   * says *who* is busy. Computed from the snapshot, not from `items`: a window
   * the user removed from the bar must still be able to raise the alarm.
   */
  const handleColor = $derived(handleColorOf(snapshot.value, s));
  /** one segment per polled provider, each in its own severity colour */
  const segments = $derived(handleSegments(snapshot.value, s));
  /** the handle's tooltip / accessible name: the app and the busiest provider */
  const handleTitle = $derived.by(() => {
    const peak = handlePeak(snapshot.value, s);
    if (!peak) return t('app.name');
    const name = snapshot.value?.providers.find((p) => p.provider === peak.provider)?.displayName ?? peak.provider;
    return t('sidebar.handleTitle', { app: t('app.name'), provider: name, percent: formatPercent(peak.used, s.percentMode) });
  });

  /**
   * Wall clock for the countdown labels and the aria text. Re-armed after every
   * tick for the next minute boundary of any reset moment on screen (the same
   * rule the popover uses), so nothing repaints more often than it changes.
   */
  let now = $state(Date.now());
  $effect(() => {
    const anchors = (snapshot.value?.providers ?? []).flatMap((p) => p.windows.map((w) => Date.parse(w.resetsAt ?? '')));
    let timer: ReturnType<typeof setTimeout>;
    const arm = () => {
      timer = setTimeout(() => {
        now = Date.now();
        arm();
      }, nextTickDelay(Date.now(), anchors));
    };
    now = Date.now();
    arm();
    return () => clearTimeout(timer);
  });

  /**
   * One-shot micro-animations. `fx` maps a window to the event that just hit it;
   * `fxSeq` is bumped per event so a repeat replays the CSS animation. The very
   * first snapshot, and the first one after the thresholds changed (a settings
   * edit is not a quota event), only seed the baseline.
   */
  let fx = $state<Record<string, { kind: RingEventKind; id: number }>>({});
  let fxSeq = 0;
  let fxTimer: ReturnType<typeof setTimeout> | undefined;
  let prevLevels: Map<string, WindowLevel> | null = null;
  let prevThresholds = '';
  $effect(() => {
    const snap = snapshot.value;
    const th = s.thresholds;
    const animate = s.sidebarAnimations;
    if (snap === null) return;
    untrack(() => {
      const thKey = `${th.warn}/${th.critical}`;
      const next = levelsOf(snap, th);
      const events: RingEvent[] = animate && thKey === prevThresholds ? detectRingEvents(prevLevels, next) : [];
      prevLevels = next;
      prevThresholds = thKey;
      if (events.length === 0) return;
      for (const e of events) fx[e.key] = { kind: e.kind, id: ++fxSeq };
      clearTimeout(fxTimer);
      fxTimer = setTimeout(() => (fx = {}), 1000);
    });
  });

  onMount(() => {
    const disposers: Array<() => void> = [settings.init(), snapshot.init()];
    let un: Unlisten | null = null;
    let disposed = false;
    void onSidebarState((state) => {
      expanded = state.expanded;
      if (!state.pinned) pinnedKey = null;
    }).then((u) => (disposed ? u() : (un = u)));
    return () => {
      disposed = true;
      un?.();
      clearTimeout(fxTimer);
      disposers.forEach((d) => d());
    };
  });

  // theme / scale / opacity / language follow the settings rune
  $effect(() => applyTheme(settings.value));

  /** true while the user is carrying the bar to another edge / height */
  let dragging = $state(false);

  function onDrag(phase: DragPhase, dx: number, dy: number) {
    dragging = phase === 'start' || phase === 'move';
    if (phase === 'start') pinnedKey = null; // the platform force-hides the popover
    void sidebarDrag(phase, dx, dy);
  }

  function reportHover(hovered: boolean) {
    // The window trails the pointer during a drag; a stray leave must not
    // start the auto-hide timer under the user's hand.
    if (dragging && !hovered) return;
    void hoverReport('bar', hovered);
  }

  /** Ring centre in CSS px relative to this window (= the viewport). */
  /** "still here" while the pointer moves over the bar, see HEARTBEAT_MS */
  const heartbeat = throttle(() => void hoverReport('bar', true), HEARTBEAT_MS);

  function anchorOf(el: HTMLElement): { x: number; y: number } {
    // aim at the ring itself, not at the text that may sit beside it
    const target = el.querySelector<HTMLElement>('.ring, .mini') ?? el;
    const r = target.getBoundingClientRect();
    return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) };
  }

  function requestPopover(item: RingItem, el: HTMLElement) {
    const anchor = anchorOf(el);
    void popoverShow({
      provider: item.provider,
      ringIndex: item.index,
      // Both axes travel; the platform layer uses the one along the bar.
      anchorY: anchor.y,
      anchorX: anchor.x,
      windowKind: item.labelWindow?.kind ?? null,
    });
  }

  function onRingEnter(item: RingItem, ev: MouseEvent) {
    if (dragging) return;
    requestPopover(item, ev.currentTarget as HTMLElement);
  }

  /**
   * Click or Enter/Space pins the ring's popover. A double-click is two clicks
   * plus a dblclick: only the first click counts, otherwise the second one
   * would un-pin (and hide) the popover that the first had just opened.
   * The popover window never takes focus, so a keyboard user cannot be moved
   * into it; pinning keeps it open, and its Esc / close button need a pointer.
   */
  function onRingClick(item: RingItem, ev: MouseEvent) {
    if (ev.detail > 1) return;
    const el = ev.currentTarget as HTMLElement;
    const nextPinned = pinnedKey !== item.key;
    pinnedKey = nextPinned ? item.key : null;
    // re-target so a click on a *different* ring moves the pinned popover;
    // a second click on the pinned ring dismisses the popover right away
    // (hovering the ring again brings it back)
    if (nextPinned) {
      void popoverSetPinned(true);
      requestPopover(item, el);
    } else {
      void popoverHide();
    }
  }

  /**
   * Screen-reader label: provider, then its status when it is not "ok", then
   * every arc outer → inner with percent, severity and reset time.
   */
  function ringLabel(item: RingItem): string {
    const head = [item.quota.displayName];
    if (item.quota.status !== 'ok') head.push(tDyn(`status.${item.quota.status}`));
    const parts = item.arcs.map((a) => {
      const sev = severityOf(a.window.usedPercent, s.thresholds);
      const level = sev === 'normal' ? '' : ` (${t(`a11y.severity.${sev}`)})`;
      // the forecast is only spoken when the ring draws it (same trust rule)
      const fc = forecastTickPercent(a.window) == null ? null : formatForecast(a.window, s.percentMode, now);
      const outlook = fc ? `, ${t('a11y.forecast', { text: fc.text })}` : '';
      return `${windowLabel(a.window, 'bar')} ${formatPercent(a.window.usedPercent, s.percentMode)}${level}, ${formatReset(a.window.resetsAt, now)}${outlook}`;
    });
    const name = head.join(' · ');
    return parts.length === 0 ? name : `${name}: ${parts.join('; ')}`;
  }

  /** arcs as the ring / mini-bar draw them, with their one-shot animation */
  function arcViews(item: RingItem) {
    return item.arcs.map((a) => {
      const e = fx[windowKey(item.provider, a.window)];
      return {
        percent: a.window.usedPercent,
        accent: a.accent,
        projectedPercent: forecastTickPercent(a.window),
        fx: e?.kind ?? null,
        fxId: e?.id ?? 0,
      };
    });
  }

  /** what the label says and where, see `labelLayout` */
  function layoutOf(item: RingItem): LabelLayout {
    if (!s.sidebarItems.percentLabel) return { center: null, main: null, sub: null };
    const w = item.labelWindow;
    return labelLayout({
      content: s.labelContent,
      position: s.percentPosition,
      horizontal,
      percent: shortPercent(w?.usedPercent ?? null, s.percentMode),
      reset: compactReset(w?.resetsAt, now),
    });
  }

  /** polite live region: only changes when a provider's status does */
  const statusAnnouncement = $derived(
    items
      .filter((i) => i.quota.status !== 'ok')
      .map((i) => t('a11y.statusChanged', { provider: i.quota.displayName, status: tDyn(`status.${i.quota.status}`) }))
      .join('. ')
  );
</script>

<svelte:head><title>{t('app.name')}</title></svelte:head>

<div
  class="stage"
  data-edge={s.edge}
  use:observeSize={reportSize}
  oncontextmenu={(e) => e.preventDefault()}
  onmouseenter={() => reportHover(true)}
  onmouseleave={() => reportHover(false)}
  onmousemove={heartbeat}
  role="presentation"
>
  <div class="sr-only" role="status" aria-live="polite">{statusAnnouncement}</div>
  {#if collapsed}
    <!-- auto-hidden: only a thin coloured sliver is left on the screen edge -->
    <!-- `collapsedWidth` is the sliver's thickness: its width on a left/right
         edge, its height on a top/bottom one -->
    <div
      class="handle"
      class:horizontal
      style:width={horizontal ? `${expandedSize.width}px` : `${Math.max(2, s.collapsedWidth)}px`}
      style:height={horizontal ? `${Math.max(2, s.collapsedWidth)}px` : `${expandedSize.height}px`}
      onmouseenter={() => void hoverReport('bar', true)}
      role="img"
      aria-label={handleTitle}
      title={handleTitle}
    >
      {#each segments as g (g.provider)}
        <span class="seg" style:background={g.accent}></span>
      {:else}
        <span class="seg" style:background={handleColor}></span>
      {/each}
    </div>
  {:else}
    <div class="pill surface" class:compact use:dragHandle={onDrag} ondblclick={() => void openDashboard('overview')} role="presentation">
      {#if loading}
        {#each placeholders as i (i)}
          <div class="slot">
            {#if compact}
              <MiniBar segments={[]} thresholds={s.thresholds} {horizontal} loading />
            {:else}
              <Ring arcs={[]} thresholds={s.thresholds} loading showPercentLabel={s.sidebarItems.percentLabel} percentPosition={s.percentPosition} />
            {/if}
          </div>
        {/each}
      {:else if items.length === 0}
        <!-- Nothing to draw (no provider enabled, or everything hidden from the
             bar): the grip is rendered whatever `moreButton` says, so the pill
             keeps a non-zero box and stays hoverable, draggable and clickable. -->
        <div class="slot empty" title={t('overview.noProviders')}>
          <button class="dots" onclick={() => void openDashboard('settings')} ondblclick={(e) => e.stopPropagation()} aria-label={t('sidebar.more')}>⋯</button>
        </div>
      {:else}
        {#each items as item (item.key)}
          <div
            class="slot"
            class:pinned={pinnedKey === item.key}
            role="button"
            tabindex="0"
            aria-label={ringLabel(item)}
            aria-pressed={pinnedKey === item.key}
            onmouseenter={(e) => onRingEnter(item, e)}
            onclick={(e) => onRingClick(item, e)}
            ondblclick={(e) => e.stopPropagation()}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                onRingClick(item, e as unknown as MouseEvent);
              }
            }}
          >
            {#if compact}
              <MiniBar
                segments={arcViews(item)}
                thresholds={s.thresholds}
                percentMode={s.percentMode}
                layout={layoutOf(item)}
                {horizontal}
                status={item.quota.status}
              >
                {#snippet logo(logoSize)}
                  {#if s.sidebarItems.logo}<ProviderLogo provider={item.provider} size={logoSize} />{/if}
                {/snippet}
              </MiniBar>
            {:else}
              <Ring
                arcs={arcViews(item)}
                labelPercent={item.labelWindow?.usedPercent ?? null}
                thresholds={s.thresholds}
                showPercentLabel={s.sidebarItems.percentLabel}
                percentMode={s.percentMode}
                percentPosition={s.percentPosition}
                layout={layoutOf(item)}
                inline={horizontal}
                status={item.quota.status}
                interactive
              >
                {#snippet logo(logoSize)}
                  {#if s.sidebarItems.logo}<ProviderLogo provider={item.provider} size={logoSize} />{/if}
                {/snippet}
              </Ring>
            {/if}
          </div>
        {/each}
        {#if s.sidebarItems.moreButton}
          <button class="dots" onclick={() => void openDashboard('overview')} ondblclick={(e) => e.stopPropagation()} aria-label={t('sidebar.more')}>⋯</button>
        {/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  /* The stage is exactly as big as the painted element — that measurement is
     what sidebar_relayout sizes the window to. */
  .stage {
    width: max-content;
    height: max-content;
  }

  .pill {
    display: flex;
    flex-direction: column;
    align-items: center;
    /* Geometry comes from Settings.sizes (applied to <html> by applyTheme).
       At the defaults this is the original ~76px pill: 56px ring + 2 × 10px
       side padding, 14px top/bottom, 18px between groups. max-content (not a
       fixed width) so removing the edge-side border below can never squeeze
       the ring out of the content box. */
    width: max-content;
    min-width: calc(var(--ring-size) + 2 * var(--bar-padding));
    /* vertical padding tracks the horizontal one but stays 4px roomier, which
       reproduces the 14/10 of the reference design at the default size */
    padding: calc(var(--bar-padding) + 0.25rem) var(--bar-padding);
    gap: var(--bar-gap);
    /* background / border / shadow / glass layers come from the global
       `.surface` material in base.css so the popover bubble matches exactly */
    border-radius: var(--r-pill);
  }

  .pill:global([data-dragging]) {
    cursor: grabbing;
  }

  /* A top/bottom bar is the same pill turned 90°: the rings run in a row and
     the 4px-roomier padding follows to the horizontal axis. */
  .stage[data-edge='top'] .pill,
  .stage[data-edge='bottom'] .pill {
    flex-direction: row;
    min-width: 0;
    min-height: calc(var(--ring-size) + 2 * var(--bar-padding));
    padding: var(--bar-padding) calc(var(--bar-padding) + 0.25rem);
  }

  /* Compact bars: the footprint is the bars', not a ring's. Declared after the
     edge rules above so it wins at equal specificity. */
  .stage .pill.compact {
    min-width: 0;
    min-height: 0;
    gap: 0.75rem;
    padding: 0.625rem 0.5rem;
  }

  .stage[data-edge='top'] .pill.compact,
  .stage[data-edge='bottom'] .pill.compact {
    gap: 1rem;
    padding: 0.5rem 0.75rem;
  }

  /* The docked side is flush with the screen: no rounding, no border there. */
  .stage[data-edge='right'] .pill {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
    border-right: none;
  }

  .stage[data-edge='left'] .pill {
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
    border-left: none;
  }

  .stage[data-edge='top'] .pill {
    border-top-left-radius: 0;
    border-top-right-radius: 0;
    border-top: none;
  }

  .stage[data-edge='bottom'] .pill {
    border-bottom-left-radius: 0;
    border-bottom-right-radius: 0;
    border-bottom: none;
  }

  .slot {
    display: block;
    line-height: 0;
    border-radius: 0.75rem;
  }

  .slot.empty {
    display: grid;
    place-items: center;
    min-height: 3.5rem;
  }

  .stage[data-edge='top'] .slot.empty,
  .stage[data-edge='bottom'] .slot.empty {
    min-width: 3.5rem;
    min-height: 0;
  }

  .slot.pinned :global(.ring) {
    /* pinned popover: keep the ring visually "held open" */
    transform: scale(1.06);
  }

  .dots {
    position: relative;
    display: block;
    width: 100%;
    border-radius: 0.25rem;
    margin-top: -0.375rem;
    padding: 0;
    font-size: 1rem;
    line-height: 0.6;
    color: var(--faint);
    letter-spacing: 0.1em;
    transition: color var(--dur-ui) var(--ease-out);
  }

  .dots:hover {
    color: var(--text);
  }

  /* The glyph is ~10px tall; an invisible box makes the target at least 24px
     without moving anything. In a column it grows downwards, into the pill's
     bottom padding, so it never covers the ring above. */
  .dots::before {
    content: '';
    position: absolute;
    top: 0;
    left: 0;
    width: max(100%, 24px);
    height: max(1.5rem, 24px);
  }

  /* In a row the dots sit beside the last ring instead of under it; the
     negative margin keeps them tucked against the group either way. */
  .stage[data-edge='top'] .dots,
  .stage[data-edge='bottom'] .dots {
    width: auto;
    align-self: center;
    margin-top: 0;
    margin-left: -0.375rem;
  }

  /* in a row it grows to the right, centred on the glyph's line */
  .stage[data-edge='top'] .dots::before,
  .stage[data-edge='bottom'] .dots::before {
    top: 50%;
    transform: translateY(-50%);
  }

  .handle {
    height: 8.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.0625rem;
    overflow: hidden;
    border-radius: 999px;
    opacity: 0.85;
  }

  .handle.horizontal {
    flex-direction: row;
  }

  /* one equal segment per provider, in that provider's severity colour */
  .handle .seg {
    flex: 1 1 0;
    min-width: 0;
    min-height: 0;
    transition: background var(--dur-ring) var(--ease-out);
  }

  /* The inline width/height above win; this only keeps the fallback sane. */
  .stage[data-edge='top'] .handle,
  .stage[data-edge='bottom'] .handle {
    width: 8.75rem;
    height: auto;
  }
</style>
