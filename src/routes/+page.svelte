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
    the window and snaps it to the nearer edge of the drop monitor on release.
  * Hovering a ring asks for the popover with the ring's centre y in CSS px
    relative to *this* window — which is the viewport, so
    `rect.top + rect.height / 2` is already the right number.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Ring from '$lib/components/Ring.svelte';
  import ProviderLogo from '$lib/components/ProviderLogo.svelte';
  import { dragHandle, observeSize, type DragPhase, type SizeReport } from '$lib/actions';
  import {
    hoverReport,
    onSidebarState,
    openDashboard,
    popoverSetPinned,
    popoverShow,
    sidebarDrag,
    sidebarRelayout,
    type Unlisten,
  } from '$lib/api';
  import { severityColor, worstSeverity } from '$lib/format';
  import { t } from '$lib/i18n/i18n.svelte';
  import { rings, type RingItem } from '$lib/stores/rings.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import { applyTheme, markWindow } from '$lib/stores/theme.svelte';

  // stamped before the first applyTheme() effect so the theme store knows
  // which window it is (the custom text colour is widget-only)
  markWindow('sidebar');

  const s = $derived(settings.value);
  const items = $derived(rings.items);
  const loading = $derived(snapshot.value === null);

  /** platform-owned expand/collapse state; only meaningful when autoHide is on */
  let expanded = $state(true);
  let expandedSize = $state<SizeReport>({ width: 76, height: 160 });
  /** ring key of the pinned popover, null when nothing is pinned */
  let pinnedKey = $state<string | null>(null);

  const collapsed = $derived(s.autoHide && !expanded);

  function reportSize(size: SizeReport) {
    if (!collapsed) expandedSize = size;
    // A collapsed handle must not overwrite the size to restore on hover.
    void sidebarRelayout(expandedSize.width, expandedSize.height);
  }

  /**
   * Colour of the collapsed handle: the worst threshold reached by any ring.
   * When nothing crossed a threshold the handle takes the accent of the ring
   * that is closest to its limit, so the sliver still says *who* is busy.
   */
  const handleColor = $derived.by(() => {
    if (items.length === 0) return 'var(--surface-track)';
    const worst = worstSeverity(items.map((i) => i.severity));
    const pct = (i: RingItem) => i.labelWindow?.usedPercent ?? -1;
    const leader = items.reduce((a, b) => (pct(b) > pct(a) ? b : a));
    return severityColor(leader.accent, worst);
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

  function anchorOf(el: HTMLElement): number {
    const r = el.getBoundingClientRect();
    return Math.round(r.top + r.height / 2);
  }

  function requestPopover(item: RingItem, el: HTMLElement) {
    void popoverShow({
      provider: item.provider,
      ringIndex: item.index,
      anchorY: anchorOf(el),
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
    void popoverSetPinned(nextPinned);
    // re-target so a click on a *different* ring moves the pinned popover
    if (nextPinned) requestPopover(item, el);
  }

  /** Screen-reader label: every arc of the group, outer → inner. */
  function ringLabel(item: RingItem): string {
    if (item.arcs.length === 0) return item.quota.displayName;
    const parts = item.arcs.map((a) => `${a.window.label} ${Math.round(a.window.usedPercent)}%`);
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
  role="presentation"
>
  {#if collapsed}
    <!-- auto-hidden: only a thin coloured sliver is left on the screen edge -->
    <div
      class="handle"
      style:width={`${Math.max(2, s.collapsedWidth)}px`}
      style:height={`${expandedSize.height}px`}
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
            <Ring arcs={[]} thresholds={s.thresholds} loading showPercentLabel={s.showPercentLabel} />
          </div>
        {/each}
      {:else if items.length === 0}
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
              arcs={item.arcs.map((a) => ({ percent: a.window.usedPercent, accent: a.accent }))}
              labelPercent={item.labelWindow?.usedPercent ?? null}
              thresholds={s.thresholds}
              showPercentLabel={s.showPercentLabel}
              percentMode={s.percentMode}
              status={item.quota.status}
              interactive
            >
              {#snippet logo(logoSize)}
                <ProviderLogo provider={item.provider} size={logoSize} />
              {/snippet}
            </Ring>
          </div>
        {/each}
        <button class="dots" onclick={() => void openDashboard('overview')} aria-label={t('sidebar.more')}>⋯</button>
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

  .slot {
    display: block;
    line-height: 0;
  }

  .slot.empty {
    display: grid;
    place-items: center;
    min-height: 3.5rem;
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

  .handle {
    height: 8.75rem;
    border-radius: 999px;
    opacity: 0.85;
    transition: background var(--dur-ring) var(--ease-out);
  }
</style>
