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
    offset = C * (1 - used/100)       → how much of the dash to hide
  Each arc is rotated -90° about the centre so 0 % starts at 12 o'clock.
  The element is sized in rem (size / 16) so Settings.scale scales it.

  The logo shrinks as arcs are added (3 → 16px, 2 → 18px, 1 → 22px at the
  default 56px ring, scaled by ringSize/56 and capped to the inner disc) so it
  always clears the innermost stroke; the size is handed to the `logo` snippet.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { severityColor, severityOf, clampPercent, shortPercent } from '$lib/format';
  import { clampSize, settings } from '$lib/stores/settings.svelte';
  import type { PercentMode, ProviderStatus, Thresholds } from '$lib/types';

  export interface RingArcView {
    /** used percent 0..100, or null when unknown */
    percent: number | null;
    /** css colour expression before the threshold override */
    accent: string;
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
    status = 'ok',
    loading = false,
    interactive = false,
    ariaLabel,
    logo,
  }: Props = $props();

  // geometry follows Settings.sizes unless the caller pinned it (the settings
  // preview does, so it can show a size before it is applied)
  const dim = $derived(clampSize('ringSize', size ?? settings.value.sizes.ringSize));
  const sw = $derived(clampSize('ringStroke', stroke ?? settings.value.sizes.ringStroke));
  const c = $derived(dim / 2);

  /** Geometry + resolved colour for every arc, outer → inner. */
  const drawn = $derived.by(() =>
    arcs.map((a, i) => {
      const r = (dim - sw) / 2 - i * (sw + gap);
      const circumference = 2 * Math.PI * r;
      const used = a.percent == null ? 0 : clampPercent(a.percent);
      return {
        r,
        circumference,
        // a ring whose own usage crossed a threshold turns amber / red even
        // when the rest of the group is still on the provider hue
        color: severityColor(a.accent, severityOf(a.percent, thresholds)),
        dashOffset: circumference * (1 - used / 100),
        known: a.percent != null,
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
        {/each}
      {/if}
    </svg>
    <span class="center">
      {@render logo?.(logoSize)}
    </span>
    {#if badge === 'warn'}
      <span class="badge badge-warn" aria-hidden="true">!</span>
    {:else if badge === 'dot'}
      <span class="badge badge-dot" aria-hidden="true"></span>
    {/if}
  </div>
  {#if showPercentLabel}
    <span class="pct" class:dimmed>{loading ? '' : shortPercent(labelPercent, percentMode)}</span>
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

  .center {
    position: relative;
    display: grid;
    place-items: center;
    color: var(--logo);
    pointer-events: none;
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
    font-size: var(--label-size, 0.8125rem);
    font-weight: 600;
    letter-spacing: 0.01em;
    color: var(--text);
    text-shadow: var(--label-shadow);
    font-variant-numeric: tabular-nums;
    min-height: 1.1em;
  }

  .pct.dimmed {
    color: var(--muted);
  }
</style>
