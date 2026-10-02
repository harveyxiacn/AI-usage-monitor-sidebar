<!--
  Compact sidebar unit (Settings.ringStyle = "bar"). [FRONTEND]

  One provider = a logo dot, one slim progress bar per visible window (the same
  windows a ring group would stack as arcs, outer → inner → top → bottom) and
  the optional label. About a third of a ring's footprint.

  Orientation follows the bar's edge: on a left/right edge the unit is a column
  (logo + percent on top, the bars full width under them, the countdown last);
  on a top/bottom edge it is a row (logo, bars stacked, "73% · 1h12" to the
  right) so it uses the strip's width instead of its height.

  Everything the ring encodes survives here, none of it by colour alone: the
  end of a warn bar carries a dot and of a critical bar a notch, the forecast
  is a hairline tick plus a dashed translucent run, a projection of 100 % adds
  the hourglass, a status shows the shared shape-coded badge, and the one-shot
  pulse / flash (`fx`) is switched off by reduced motion. Geometry is in rem so
  Settings.scale applies.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { clampPercent, severityColor, severityOf, type Severity } from '$lib/format';
  import { forecastArcRange, miniText, willRunOut, type LabelLayout } from '$lib/sidebar-visuals';
  import type { RingEventKind } from '$lib/ring-events';
  import type { PercentMode, ProviderStatus, Thresholds } from '$lib/types';
  import RunOutBadge from './RunOutBadge.svelte';
  import StatusBadge from './StatusBadge.svelte';

  export interface MiniSegment {
    percent: number | null;
    accent: string;
    projectedPercent?: number | null;
    fx?: RingEventKind | null;
    fxId?: number;
  }

  interface Props {
    segments: MiniSegment[];
    thresholds: Thresholds;
    percentMode?: PercentMode;
    layout?: LabelLayout | null;
    /** top/bottom edge: lay the unit out as a row */
    horizontal?: boolean;
    status?: ProviderStatus;
    loading?: boolean;
    /** receives the logo size in px */
    logo?: Snippet<[number]>;
  }

  let {
    segments,
    thresholds,
    percentMode = 'used',
    layout = null,
    horizontal = false,
    status = 'ok',
    loading = false,
    logo,
  }: Props = $props();

  const shown = (p: number) => (percentMode === 'remaining' ? 100 - clampPercent(p) : clampPercent(p));

  const drawn = $derived(
    segments.map((s) => {
      const severity: Severity = severityOf(s.percent, thresholds);
      const range = forecastArcRange(s.percent, s.projectedPercent ?? null, percentMode);
      return {
        severity,
        known: s.percent != null,
        color: severityColor(s.accent, severity),
        width: s.percent == null ? 0 : shown(s.percent),
        tick: s.projectedPercent == null ? null : shown(s.projectedPercent),
        forecast: range,
        fx: s.fx ?? null,
        fxId: s.fxId ?? 0,
      };
    })
  );
  const runsOut = $derived(!loading && segments.some((s) => willRunOut(s.projectedPercent)));
  const text = $derived(layout ? miniText(layout, horizontal) : null);
  const dimmed = $derived(loading || segments.length === 0 || status === 'not_logged_in');
</script>

<div class="mini" class:horizontal class:dimmed class:loading>
  <span class="head">
    <span class="dot">
      {@render logo?.(12)}
      <StatusBadge {status} inline />
      {#if runsOut}<RunOutBadge inline />{/if}
    </span>
    {#if !horizontal && text?.primary}<span class="pct">{text.primary}</span>{/if}
  </span>
  <span class="segs" aria-hidden="true">
    {#each drawn.length > 0 ? drawn : [null] as d, i (i)}
      <span class="seg" style:color={d?.color}>
        {#if d && !loading && d.known}
          {#if d.forecast}
            <span class="forecast" style:left="{d.forecast.from}%" style:width="{d.forecast.to - d.forecast.from}%" style:--c={d.color}></span>
          {/if}
          <span class="fill" style:width="{d.width}%" style:background={d.color}>
            {#if d.severity === 'critical'}<span class="cap cap-bar"></span>{:else if d.severity === 'warn'}<span class="cap cap-dot"></span>{/if}
          </span>
          {#if d.tick != null}<span class="tick" style:left="{d.tick}%"></span>{/if}
          {#if d.fx}
            {#key d.fxId}
              <span class="fx fx-{d.fx === 'reset' ? 'flash' : 'pulse'}"></span>
            {/key}
          {/if}
        {/if}
      </span>
    {/each}
  </span>
  {#if text}
    {#if horizontal}
      {#if text.primary}<span class="pct inline">{text.primary}</span>{/if}
    {:else if text.secondary}
      <span class="pct sub">{text.secondary}</span>
    {/if}
  {/if}
</div>

<style>
  .mini {
    --seg-w: 2.75rem;
    --seg-h: 0.25rem;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 0.25rem;
    width: var(--seg-w);
    line-height: 1;
  }

  .mini.horizontal {
    flex-direction: row;
    align-items: center;
    gap: 0.4375rem;
    width: max-content;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 0.3125rem;
    min-height: 0.875rem;
  }

  .dot {
    display: inline-flex;
    align-items: center;
    gap: 0.1875rem;
    flex: none;
    color: var(--logo);
  }

  .pct {
    font-size: calc(var(--label-size, 0.8125rem) * 0.92);
    font-weight: 600;
    letter-spacing: 0.01em;
    color: var(--text);
    text-shadow: var(--label-shadow);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .pct.sub {
    font-size: calc(var(--label-size, 0.8125rem) * 0.8);
    font-weight: 500;
    color: var(--muted);
    text-align: center;
  }

  .segs {
    display: flex;
    flex-direction: column;
    gap: 0.1875rem;
    width: var(--seg-w);
    flex: none;
  }

  .seg {
    position: relative;
    display: block;
    height: var(--seg-h);
    border-radius: 999px;
    background: var(--surface-track);
  }

  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: inherit;
    transition: width var(--dur-ring) var(--ease-out), background var(--dur-ring) var(--ease-out);
  }

  /* end-of-fill markers: cut out of the bar in the pill's own colour */
  .cap {
    position: absolute;
    top: 50%;
    right: 0;
    background: rgb(var(--bar-bg-rgb) / 0.9);
    transform: translateY(-50%);
  }

  .cap-dot {
    width: 0.1875rem;
    height: 0.1875rem;
    margin-right: 0.0625rem;
    border-radius: 999px;
  }

  .cap-bar {
    width: 0.125rem;
    height: 100%;
  }

  .forecast {
    position: absolute;
    top: 0;
    bottom: 0;
    background: repeating-linear-gradient(90deg, var(--c) 0 0.125rem, transparent 0.125rem 0.25rem);
    opacity: 0.45;
  }

  .tick {
    position: absolute;
    top: -0.0625rem;
    bottom: -0.0625rem;
    width: 0.0625rem;
    margin-left: -0.03125rem;
    background: var(--text);
    box-shadow: 0 0 0 0.03125rem rgb(var(--bar-bg-rgb) / 0.85);
    opacity: 0.85;
  }

  .dimmed .segs,
  .dimmed .pct {
    opacity: 0.45;
  }

  .loading {
    animation: mini-pulse 1.4s ease-in-out infinite;
  }

  @keyframes mini-pulse {
    0%,
    100% {
      opacity: 0.32;
    }
    50% {
      opacity: 0.62;
    }
  }

  .fx {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    pointer-events: none;
  }

  /* an outward ring of the segment's colour, ~600 ms */
  .fx-pulse {
    animation: mini-pulse-out 600ms ease-out 1 both;
  }

  .fx-flash {
    background: var(--text);
    animation: mini-flash 700ms ease-out 1 both;
  }

  @keyframes mini-pulse-out {
    from {
      box-shadow: 0 0 0 0 currentColor;
      opacity: 0.9;
    }
    to {
      box-shadow: 0 0 0 0.375rem currentColor;
      opacity: 0;
    }
  }

  @keyframes mini-flash {
    from {
      opacity: 0.9;
    }
    to {
      opacity: 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .loading {
      animation: none;
      opacity: 0.4;
    }

    .fx {
      display: none;
    }

    .fill {
      transition: none;
    }
  }
</style>
