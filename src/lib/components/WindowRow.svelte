<!--
  One rate-limit window: label / bar / "73% Used" + "Resets in 51 min", plus the
  burn-rate line ("Runs out in ~40 min") when the backend sent a forecast.
  Used by the popover bubble and by the dashboard overview cards. [FRONTEND]
-->
<script lang="ts">
  import Bar from './Bar.svelte';
  import { formatForecast, formatPercent, formatReset, severityColor, severityOf, clampPercent, windowLabel } from '$lib/format';
  import type { PercentMode, QuotaWindow, Thresholds } from '$lib/types';

  interface Props {
    window: QuotaWindow;
    accent: string;
    thresholds: Thresholds;
    percentMode?: PercentMode;
    /** 'popover' renames the primary 5-hour window to "Current session" */
    context?: 'popover' | 'dashboard';
    /** override the label (dashboard shows the raw backend label for scoped rows) */
    label?: string;
    highlight?: boolean;
    /** re-render tick so "Resets in …" counts down */
    now?: number;
    compact?: boolean;
  }

  let {
    window: w,
    accent,
    thresholds,
    percentMode = 'used',
    context = 'popover',
    label,
    highlight = false,
    now = Date.now(),
    compact = false,
  }: Props = $props();

  const used = $derived(clampPercent(w.usedPercent));
  const severity = $derived(severityOf(w.usedPercent, thresholds));
  const color = $derived(severityColor(accent, severity));
  // the bar paints whatever the percent label says, see Bar.svelte
  const shown = $derived(percentMode === 'remaining' ? 100 - used : used);
  const forecast = $derived(formatForecast(w, percentMode, now));
</script>

<div class="row" class:highlight class:compact>
  <div class="label">{label ?? windowLabel(w, context)}</div>
  <Bar value={shown} {color} height={compact ? 5 : 6} />
  <div class="stats">
    <span class="pct">{formatPercent(w.usedPercent, percentMode)}</span>
    <span class="reset">{formatReset(w.resetsAt, now)}</span>
  </div>
  {#if forecast}
    <div class="forecast" class:warn={forecast.tone === 'warn'}>{forecast.text}</div>
  {/if}
</div>

<style>
  .row {
    display: flex;
    flex-direction: column;
    gap: 0.4375rem;
  }

  .row.compact {
    gap: 0.3125rem;
  }

  .label {
    font-size: 0.875rem;
    font-weight: 500;
    color: var(--text);
    letter-spacing: 0.005em;
  }

  .compact .label {
    font-size: 0.8125rem;
  }

  .highlight .label::after {
    content: '';
    display: inline-block;
    width: 0.3125rem;
    height: 0.3125rem;
    margin-left: 0.375rem;
    border-radius: 999px;
    background: currentColor;
    vertical-align: 0.15em;
  }

  .stats {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
    font-size: 0.8125rem;
  }

  .pct {
    font-weight: 500;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  .reset {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  /* The burn-rate line is a footnote to the row: one notch smaller than the
     stats above it, and only coloured when the window will actually run out.
     `-0.125rem` pulls it back into the 0.4375rem row gap so the row does not
     grow a full line taller when a forecast appears. */
  .forecast {
    margin-top: -0.125rem;
    font-size: 0.75rem;
    line-height: 1.3;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }

  .forecast.warn {
    color: var(--warn);
  }
</style>
