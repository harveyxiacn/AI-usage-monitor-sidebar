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
  import { onMount } from 'svelte';
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
  import { shortPercent } from '$lib/format';
  import { forecastTickPercent } from '$lib/forecast';
  import { t } from '$lib/i18n/i18n.svelte';
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

  /** platform-owned expand/collapse state; only meaningful when autoHide is on */
  let expanded = $state(true);
  let expandedSize = $state<SizeReport>({ width: 76, height: 160 });
  /** ring key of the pinned popover, null when nothing is pinned */
  let pinnedKey = $state<string | null>(null);

  const collapsed = $derived(s.autoHide && !expanded);
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
    const r = el.getBoundingClientRect();
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

  function onRingClick(item: RingItem, ev: MouseEvent) {
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

  /** Screen-reader label: every arc of the group, outer → inner. */
  function ringLabel(item: RingItem): string {
    if (item.arcs.length === 0) return item.quota.displayName;
    const parts = item.arcs.map((a) => `${a.window.label} ${shortPercent(a.window.usedPercent, s.percentMode)}`);
    return `${item.quota.displayName}: ${parts.join(', ')}`;
  }
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
  {#if collapsed}
    <!-- auto-hidden: only a thin coloured sliver is left on the screen edge -->
    <!-- `collapsedWidth` is the sliver's thickness: its width on a left/right
         edge, its height on a top/bottom one -->
    <div
      class="handle"
      style:width={horizontal ? `${expandedSize.width}px` : `${Math.max(2, s.collapsedWidth)}px`}
      style:height={horizontal ? `${Math.max(2, s.collapsedWidth)}px` : `${expandedSize.height}px`}
      style:background={handleColor}
      onmouseenter={() => void hoverReport('bar', true)}
      role="presentation"
      title={t('app.name')}
    ></div>
  {:else}
    <div class="pill surface" use:dragHandle={onDrag} ondblclick={() => void openDashboard('overview')} role="presentation">
      {#if loading}
        {#each [0, 1] as i (i)}
          <div class="slot">
            <Ring arcs={[]} thresholds={s.thresholds} loading showPercentLabel={s.sidebarItems.percentLabel} percentPosition={s.percentPosition} />
          </div>
        {/each}
      {:else if items.length === 0}
        <!-- Nothing to draw (no provider enabled, or everything hidden from the
             bar): the grip is rendered whatever `moreButton` says, so the pill
             keeps a non-zero box and stays hoverable, draggable and clickable. -->
        <div class="slot empty" title={t('overview.noProviders')}>
          <button class="dots" onclick={() => void openDashboard('settings')} aria-label={t('sidebar.more')}>⋯</button>
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
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                onRingClick(item, e as unknown as MouseEvent);
              }
            }}
          >
            <Ring
              arcs={item.arcs.map((a) => ({
                percent: a.window.usedPercent,
                accent: a.accent,
                projectedPercent: forecastTickPercent(a.window),
              }))}
              labelPercent={item.labelWindow?.usedPercent ?? null}
              thresholds={s.thresholds}
              showPercentLabel={s.sidebarItems.percentLabel}
              percentMode={s.percentMode}
              percentPosition={s.percentPosition}
              status={item.quota.status}
              interactive
            >
              {#snippet logo(logoSize)}
                {#if s.sidebarItems.logo}<ProviderLogo provider={item.provider} size={logoSize} />{/if}
              {/snippet}
            </Ring>
          </div>
        {/each}
        {#if s.sidebarItems.moreButton}
          <button class="dots" onclick={() => void openDashboard('overview')} aria-label={t('sidebar.more')}>⋯</button>
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
    display: block;
    width: 100%;
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

  /* In a row the dots sit beside the last ring instead of under it; the
     negative margin keeps them tucked against the group either way. */
  .stage[data-edge='top'] .dots,
  .stage[data-edge='bottom'] .dots {
    width: auto;
    align-self: center;
    margin-top: 0;
    margin-left: -0.375rem;
  }

  .handle {
    height: 8.75rem;
    border-radius: 999px;
    opacity: 0.85;
    transition: background var(--dur-ring) var(--ease-out);
  }

  /* The inline width/height above win; this only keeps the fallback sane. */
  .stage[data-edge='top'] .handle,
  .stage[data-edge='bottom'] .handle {
    width: 8.75rem;
    height: auto;
  }
</style>
