<!--
  Tiny trend line for a KPI tile. [FRONTEND]
  Decorative: the tile's own text carries the value and the change, so the
  line is hidden from assistive technology. Fixed 100x24 viewBox, stretched.
-->
<script lang="ts">
  interface Props {
    values: number[];
    color: string;
    height?: number;
  }

  let { values, color, height = 24 }: Props = $props();

  const W = 100;
  const H = 24;

  const points = $derived.by(() => {
    if (values.length === 0) return '';
    const max = Math.max(...values, 0);
    const min = Math.min(...values, 0);
    const span = max - min || 1;
    const step = values.length > 1 ? W / (values.length - 1) : 0;
    const y = (v: number) => H - 2 - ((v - min) / span) * (H - 4);
    if (values.length === 1) return `0,${y(values[0]).toFixed(2)} ${W},${y(values[0]).toFixed(2)}`;
    return values.map((v, i) => `${(i * step).toFixed(2)},${y(v).toFixed(2)}`).join(' ');
  });
</script>

<svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" style:height={`${height / 16}rem`} aria-hidden="true" focusable="false">
  {#if points}
    <polygon points={`${points} ${W},${H} 0,${H}`} fill={color} opacity="0.14" />
    <polyline {points} fill="none" stroke={color} stroke-width="1.5" stroke-linejoin="round" stroke-linecap="round" vector-effect="non-scaling-stroke" />
  {:else}
    <line x1="0" y1={H - 1} x2={W} y2={H - 1} stroke="var(--border)" stroke-dasharray="3 3" vector-effect="non-scaling-stroke" />
  {/if}
</svg>

<style>
  svg {
    display: block;
    width: 100%;
  }
</style>
