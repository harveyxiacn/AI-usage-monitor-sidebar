<!--
  Tiny quota trend line for the dashboard overview. [FRONTEND]
  Values are 0..100 used-percent samples in chronological order; the polyline is
  drawn in a fixed 100×28 viewBox with `preserveAspectRatio="none"` so it
  stretches to whatever width the card gives it without recomputing points.
-->
<script lang="ts">
  interface Props {
    values: number[];
    color: string;
    height?: number;
    /** draw a faint area under the line */
    fill?: boolean;
    label?: string;
  }

  let { values, color, height = 28, fill = true, label }: Props = $props();

  const W = 100;
  const H = 28;

  const points = $derived.by(() => {
    if (values.length === 0) return '';
    if (values.length === 1) return `0,${H / 2} ${W},${H / 2}`;
    const step = W / (values.length - 1);
    return values
      .map((v, i) => {
        const y = H - (Math.min(100, Math.max(0, v)) / 100) * (H - 2) - 1;
        return `${(i * step).toFixed(2)},${y.toFixed(2)}`;
      })
      .join(' ');
  });

  const area = $derived(points ? `${points} ${W},${H} 0,${H}` : '');
</script>

{#if values.length === 0}
  <div class="empty" style:height={`${height / 16}rem`}></div>
{:else}
  <svg
    viewBox="0 0 {W} {H}"
    preserveAspectRatio="none"
    style:height={`${height / 16}rem`}
    role={label ? 'img' : 'presentation'}
    aria-label={label}
  >
    {#if fill}
      <polygon points={area} fill={color} opacity="0.14" />
    {/if}
    <polyline
      {points}
      fill="none"
      stroke={color}
      stroke-width="1.5"
      stroke-linejoin="round"
      stroke-linecap="round"
      vector-effect="non-scaling-stroke"
    />
  </svg>
{/if}

<style>
  svg {
    display: block;
    width: 100%;
  }

  .empty {
    width: 100%;
    border-bottom: 1px dashed var(--border);
  }
</style>
