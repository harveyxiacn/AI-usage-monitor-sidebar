<!--
  Ring group with the provider logo in the middle. [FRONTEND]

  Renders 0..3 concentric arcs, outer → inner. `ringMode="concentric"` uses all
  of them for one provider; the other two modes pass a single arc, which makes
  this exactly the old single ring.

  Geometry follows Settings.sizes.ringSize / .ringStroke (props may override);
  the inter-ring gap stays a constant 2.5 user units.

  Ring geometry (all in the SVG's own user units, viewBox = size × size):
    cx = cy = size / 2
    r(i)   = (size - stroke) / 2 - i * (stroke + gap)
             → the stroke is centred on r, so the outermost arc touches the
               viewBox edge exactly and each inner ring steps in by one stroke
               plus one gap
    C      = 2πr(i)                   → dasharray, that ring's circumference
    offset = C * (1 - shown/100)       → how much of the dash to hide
  Shown is used percent, or 100 - used in remaining mode. Forecast ticks
  follow the same scale; threshold colours always follow actual usage.
  Each arc is rotated -90° about the centre so 0 % starts at 12 o'clock.
  The element is sized in rem (size / 16) so Settings.scale scales it.

  The logo shrinks as arcs are added (3 → 16px, 2 → 18px, 1 → 22px at the
  default 56px ring, scaled by ringSize/56 and capped to the inner disc) so it
  always clears the innermost stroke; the size is handed to the `logo` snippet.

  An arc may also carry `projectedPercent`: a hairline tick across the stroke at
  the "at this pace, here at the reset" angle (see forecast.ts). It is opt-in
  per arc so a low-confidence guess never marks up a 56px ring.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { severityColor, severityOf, clampPercent, shortPercent } from '$lib/format';
  import { clampSize, settings } from '$lib/stores/settings.svelte';
  import type { PercentMode, PercentPosition, ProviderStatus, Thresholds } from '$lib/types';

  export interface RingArcView {
    /** used percent 0..100, or null when unknown */
    percent: number | null;
    /** css colour expression before the threshold override */
    accent: string;
    /**
     * Projected used percent at the reset, 0..100 — draws a tick across the
     * arc at that angle. Null (the default) draws nothing; the caller decides
     * when a forecast is trustworthy enough, see `forecastTickPercent`.
     */
    projectedPercent?: number | null;
  }

  interface Props {
    /** outer → inner; empty renders a bare track (not signed in / no data) */
    arcs: RingArcView[];
    thresholds: Thresholds;
    /** percent written under the group — the primary window's */
    labelPercent?: number | null;
    /** override Settings.sizes.ringSize (px at scale 1) */
    size?: number;
    /** override Settings.sizes.ringStroke */
    stroke?: number;
    /** space between two concentric arcs, in user units */
    gap?: number;
    showPercentLabel?: boolean;
    percentMode?: PercentMode;
    percentPosition?: PercentPosition;
    status?: ProviderStatus;
    loading?: boolean;
    interactive?: boolean;
    ariaLabel?: string;
    /** receives the logo size in px for the current arc count */
    logo?: Snippet<[number]>;
  }

  let {
    arcs,
    thresholds,
    labelPercent = null,
    size,
    stroke,
    gap = 2.5,
    showPercentLabel = false,
    percentMode = 'used',
    percentPosition = 'below',
    status = 'ok',
    loading = false,
    interactive = false,
    ariaLabel,
    logo,
  }: Props = $props();

  // geometry follows Settings.sizes unless the caller pinned it (the settings
  // preview does, so it can show a size before it is applied)
  const dim = $derived(clampSize('ringSize', size ?? settings.value.sizes.ringSize));
  const configuredSw = $derived(clampSize('ringStroke', stroke ?? settings.value.sizes.ringStroke));
  const c = $derived(dim / 2);
  const centerPercent = $derived(showPercentLabel && percentPosition === 'center');
  /**
   * Three regular arcs can leave no room at the minimum 40px ring size. In
   * centre-percent mode reserve a readable inner disc and tighten only the
   * painted arc geometry. The outer size never changes, so bar layout stays
   * stable when the user switches positions.
   */
  const effectiveGap = $derived(centerPercent && arcs.length > 1 ? Math.min(gap, 1) : gap);
  const sw = $derived.by(() => {
    if (!centerPercent || arcs.length === 0) return configuredSw;
    const targetInnerRadius = Math.max(10, dim * 0.25);
    const maxStroke = (dim / 2 - targetInnerRadius - (arcs.length - 1) * effectiveGap - 1) / arcs.length;
    return Math.min(configuredSw, Math.max(2.3, maxStroke));
  });

  function shownPercent(percent: number) {
    const used = clampPercent(percent);
    return percentMode === 'remaining' ? 100 - used : used;
  }

  /**
   * Endpoints of the forecast tick on the arc of radius `r`: a short radial
   * segment crossing the stroke at the projected angle. 0 % is 12 o'clock like
   * the arcs, and the outer end stops exactly at the stroke edge so the
   * outermost ring cannot spill out of the viewBox.
   */
  function tick(r: number, percent: number) {
    const angle = ((shownPercent(percent) / 100) * 360 - 90) * (Math.PI / 180);
    const [cosA, sinA] = [Math.cos(angle), Math.sin(angle)];
    return {
      x1: c + (r - sw / 2 - 1.5) * cosA,
      y1: c + (r - sw / 2 - 1.5) * sinA,
      x2: c + (r + sw / 2) * cosA,
      y2: c + (r + sw / 2) * sinA,
    };
  }

  /** Geometry + resolved colour for every arc, outer → inner. */
  const drawn = $derived.by(() =>
    arcs.map((a, i) => {
      const r = (dim - sw) / 2 - i * (sw + effectiveGap);
      const circumference = 2 * Math.PI * r;
      const shown = a.percent == null ? 0 : shownPercent(a.percent);
      return {
        r,
        circumference,
        // a ring whose own usage crossed a threshold turns amber / red even
        // when the rest of the group is still on the provider hue
        color: severityColor(a.accent, severityOf(a.percent, thresholds)),
        dashOffset: circumference * (1 - shown / 100),
        known: a.percent != null,
        projected: a.projectedPercent == null ? null : tick(r, a.projectedPercent),
      };
    })
  );

  /** Radius of the innermost drawn ring, or the outermost track when empty. */
  const lastR = $derived(drawn.length > 0 ? drawn[drawn.length - 1].r : (dim - sw) / 2);
  /** disc behind the logo: just inside the innermost stroke */
  const innerR = $derived(lastR - sw / 2 - 1);
  /** the logo keeps the same share of the disc at every ring size */
  const logoSize = $derived(
    Math.max(8, Math.min(innerR * 2 - 1, (arcs.length >= 3 ? 16 : arcs.length === 2 ? 18 : 22) * (dim / 56)))
  );

  const dimmed = $derived(loading || arcs.length === 0 || status === 'not_logged_in');
  const badge = $derived(
    status === 'not_logged_in' ? 'warn' : status === 'token_expired' || status === 'error' ? 'dot' : null
  );
  const label = $derived(loading ? '' : shortPercent(labelPercent, percentMode));
  /** 100% is the widest label. Keep it inside even for a 40px, three-arc ring. */
  const centerFontSize = $derived(Math.max(0.5, Math.min(0.8, (Math.max(innerR, 0) * 2) / 40)));

  const px = (v: number) => `${v / 16}rem`;
</script>

<div class="ring-wrap" class:interactive style:--ring-size={px(dim)}>
  <div class="ring" class:dimmed class:loading aria-label={ariaLabel} role={ariaLabel ? 'img' : undefined}>
    <svg viewBox="0 0 {dim} {dim}" width={px(dim)} height={px(dim)} aria-hidden="true">
      <!-- logo backdrop -->
      <circle cx={c} cy={c} r={Math.max(innerR, 0)} fill="var(--ring-inner)" />
      {#if drawn.length === 0}
        <circle cx={c} cy={c} r={(dim - sw) / 2} fill="none" stroke="var(--surface-track)" stroke-width={sw} />
      {:else}
        {#each drawn as d, i (i)}
          <!-- track -->
          <circle class="ring-track" cx={c} cy={c} r={d.r} fill="none" stroke="var(--surface-track)" stroke-width={sw} />
          <!-- value arc -->
          {#if !loading && d.known}
            <circle
              class="arc"
              cx={c}
              cy={c}
              r={d.r}
              fill="none"
              stroke={d.color}
              style:color={d.color}
              stroke-width={sw}
              stroke-linecap="round"
              stroke-dasharray={d.circumference}
              stroke-dashoffset={d.dashOffset}
              transform="rotate(-90 {c} {c})"
            />
          {/if}
          <!-- forecast: where this arc lands at the reset -->
          {#if !loading && d.known && d.projected}
            <line class="tick-halo" {...d.projected} />
            <line class="tick" {...d.projected} />
          {/if}
        {/each}
      {/if}
    </svg>
    <span class="center" class:percent-center={centerPercent} style:--center-font-size={`${centerFontSize}rem`}>
      {#if centerPercent}
        <span class="center-pct" class:dimmed>{label}</span>
      {:else}
        {@render logo?.(logoSize)}
      {/if}
    </span>
    {#if badge === 'warn'}
      <span class="badge badge-warn" aria-hidden="true">!</span>
    {:else if badge === 'dot'}
      <span class="badge badge-dot" aria-hidden="true"></span>
    {/if}
  </div>
  {#if showPercentLabel && !centerPercent}
    <span class="pct" class:dimmed>{label}</span>
  {/if}
</div>

<style>
  .ring-wrap {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.25rem;
  }

  .ring {
    position: relative;
    width: var(--ring-size);
    height: var(--ring-size);
    display: grid;
    place-items: center;
    transition: transform var(--dur-ui) var(--ease-out), opacity var(--dur-ui) var(--ease-out);
  }

  .ring svg {
    display: block;
    position: absolute;
    inset: 0;
  }

  .interactive:hover .ring {
    transform: scale(1.06);
  }

  .ring.dimmed {
    opacity: 0.45;
  }

  .ring.loading {
    animation: ring-pulse 1.4s ease-in-out infinite;
  }

  @keyframes ring-pulse {
    0%,
    100% {
      opacity: 0.32;
    }
    50% {
      opacity: 0.62;
    }
  }

  /* the ring arcs are the only thing that animates on a value change */
  .arc {
    transition:
      stroke-dashoffset var(--dur-ring) var(--ease-out),
      stroke var(--dur-ring) var(--ease-out);
  }

  /* Forecast tick: a hairline in the text colour on a band of the pill's own
     background, so it stays legible over the track, over the value arc and in
     all three surface styles without inventing a new palette entry. Butt caps
     keep the halo from growing past the stroke it crosses. */
  .tick,
  .tick-halo {
    stroke-linecap: butt;
    transition: all var(--dur-ring) var(--ease-out);
  }

  .tick-halo {
    stroke: rgb(var(--bar-bg-rgb) / 0.85);
    stroke-width: 2.6;
  }

  .tick {
    stroke: var(--text);
    stroke-width: 1.1;
    opacity: 0.85;
  }

  .center {
    position: relative;
    display: grid;
    place-items: center;
    color: var(--logo);
    pointer-events: none;
  }

  .center-pct {
    display: block;
    max-width: calc(var(--ring-size) * 0.54);
    overflow: hidden;
    color: var(--text);
    font-size: var(--center-font-size);
    font-weight: 700;
    line-height: 1;
    letter-spacing: -0.04em;
    text-align: center;
    text-overflow: clip;
    text-shadow: var(--label-shadow);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .center-pct.dimmed {
    color: var(--muted);
  }

  .badge {
    position: absolute;
    /* sit on the ring's 1-o'clock edge, half outside the circle */
    top: -0.0625rem;
    right: -0.0625rem;
    display: grid;
    place-items: center;
    border-radius: 999px;
    box-shadow: 0 0 0 0.125rem rgb(var(--bar-bg-rgb) / 0.9);
  }

  .badge-warn {
    width: 0.875rem;
    height: 0.875rem;
    background: var(--warn);
    color: #141414;
    font-size: 0.625rem;
    font-weight: 800;
    line-height: 1;
  }

  .badge-dot {
    width: 0.5rem;
    height: 0.5rem;
    background: var(--warn);
  }

  .pct {
    display: block;
    font-size: var(--label-size, 0.8125rem);
    font-weight: 600;
    letter-spacing: 0.01em;
    color: var(--text);
    text-shadow: var(--label-shadow);
    font-variant-numeric: tabular-nums;
    min-height: 1.1em;
    line-height: 1.1;
  }

  .pct.dimmed {
    color: var(--muted);
  }
</style>
