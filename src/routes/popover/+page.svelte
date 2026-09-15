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
  import { observeSize } from '$lib/actions';
  import {
    hoverReport,
    isTauri,
    onPopoverTarget,
    openDashboard,
    popoverRelayout,
    type Unlisten,
  } from '$lib/api';
  import { t } from '$lib/i18n/i18n.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import { applyTheme, markWindow } from '$lib/stores/theme.svelte';
  import type { PopoverRequest } from '$lib/types';

  const s = $derived(settings.value);

  let target = $state<PopoverRequest | null>(null);
  /** ticks once a second so "Resets in 51 min" / "Updated 12 s ago" count down */
  let now = $state(Date.now());

  const quota = $derived.by(() => {
    const providers = snapshot.value?.providers ?? [];
    if (providers.length === 0) return null;
    if (target) return providers.find((p) => p.provider === target!.provider) ?? null;
    // browser preview / first paint before the platform picked a target
    return isTauri() ? null : providers[0];
  });

  onMount(() => {
    markWindow('popover');
    const disposers: Array<() => void> = [settings.init(), snapshot.init()];
    const timer = setInterval(() => (now = Date.now()), 1000);
    let un: Unlisten | null = null;
    let disposed = false;
    void onPopoverTarget((req) => {
      target = req;
      now = Date.now();
    }).then((u) => (disposed ? u() : (un = u)));
    return () => {
      disposed = true;
      un?.();
      clearInterval(timer);
      disposers.forEach((d) => d());
    };
  });

  $effect(() => applyTheme(settings.value));
</script>

<svelte:head><title>{t('app.name')}</title></svelte:head>

<div
  class="stage"
  use:observeSize={(size) => void popoverRelayout(size.width, size.height)}
  oncontextmenu={(e) => e.preventDefault()}
  onmouseenter={() => void hoverReport('popover', true)}
  onmouseleave={() => void hoverReport('popover', false)}
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
    max-width: 23rem; /* 360px bubble + the 8px tail gutter */
  }

  .placeholder {
    padding: 0.75rem 1rem;
    border-radius: var(--r-bubble);
    color: var(--muted);
    white-space: nowrap;
  }
</style>
