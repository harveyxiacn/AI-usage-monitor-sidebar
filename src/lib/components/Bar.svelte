<!--
  Horizontal quota bar. [FRONTEND]
  `value` is the 0..100 fraction to paint (the popover paints "remaining" when
  Settings.percentMode is 'remaining', like the reference design where
  "剩余 100%" shows a completely full bar), `color` is already threshold-resolved
  by the caller so the bar and its ring always agree.
-->
<script lang="ts">
  interface Props {
    /** 0..100 */
    value: number;
    color: string;
    /** track height in px at scale 1 */
    height?: number;
    dimmed?: boolean;
  }

  let { value, color, height = 6, dimmed = false }: Props = $props();

  const pct = $derived(Math.min(100, Math.max(0, Number.isFinite(value) ? value : 0)));
</script>

<div class="track" class:dimmed style:--h={`${height / 16}rem`}>
  <div class="fill" style:width={`${pct}%`} style:background={color}></div>
</div>

<style>
  .track {
    position: relative;
    width: 100%;
    height: var(--h);
    border-radius: 999px;
    background: var(--surface-track);
    overflow: hidden;
  }

  .track.dimmed {
    opacity: 0.5;
  }

  .fill {
    height: 100%;
    border-radius: 999px;
    /* same timing as the ring arc so a refresh animates as one gesture */
    transition:
      width var(--dur-ring) var(--ease-out),
      background var(--dur-ring) var(--ease-out);
    /* a hairline minimum so 0 % still reads as "a bar", like the reference */
    min-width: var(--h);
  }
</style>
