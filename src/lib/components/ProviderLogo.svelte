<!--
  Original, simplified monochrome provider marks. [FRONTEND]
  These are deliberately *not* copies of the trademarked logos: the Claude mark
  is a generic 8-point asterisk/burst, the OpenAI mark a generic six-fold
  hexagonal rosette. Both are drawn from scratch in a 24×24 box and inherit
  `currentColor`. Any other provider gets a neutral monogram — a ringed initial
  — so the sidebar never renders an empty disc for a provider this file has not
  been taught about (and so no future provider's trademark is imitated either).
-->
<script lang="ts">
  import { providerDisplayName } from '$lib/providers';
  import type { ProviderId } from '$lib/types';

  interface Props {
    provider: ProviderId;
    /** rendered size in px at scale 1 (expressed in rem so it follows --ui-scale) */
    size?: number;
    title?: string;
  }

  let { provider, size = 22, title }: Props = $props();

  const rem = $derived(`${size / 16}rem`);
  const monogram = $derived(providerDisplayName(provider).charAt(0).toUpperCase() || '?');

  // --- Claude: 8 spokes, alternating long/short, from r=3.2 outwards. ---
  const spokes = Array.from({ length: 8 }, (_, i) => {
    const a = (i * Math.PI) / 4;
    const outer = i % 2 === 0 ? 10.4 : 8.4;
    return {
      x1: 12 + Math.cos(a) * 3.2,
      y1: 12 + Math.sin(a) * 3.2,
      x2: 12 + Math.cos(a) * outer,
      y2: 12 + Math.sin(a) * outer,
      w: i % 2 === 0 ? 2.1 : 1.6,
    };
  });

  // --- OpenAI: one petal outline repeated at 60° steps (hexagonal rosette). ---
  const PETAL = 'M0 -2.9 C2.3 -4.9 2.3 -8.4 0 -10.4 C-2.3 -8.4 -2.3 -4.9 0 -2.9 Z';
  const petalRotations = [0, 60, 120, 180, 240, 300];
</script>

<svg
  class="logo"
  viewBox="0 0 24 24"
  width={rem}
  height={rem}
  style:width={rem}
  style:height={rem}
  role={title ? 'img' : 'presentation'}
  aria-label={title}
  aria-hidden={title ? undefined : 'true'}
>
  {#if title}<title>{title}</title>{/if}
  {#if provider === 'claude'}
    <g stroke="currentColor" stroke-linecap="round">
      {#each spokes as s (s.x1 + ':' + s.y1)}
        <line x1={s.x1} y1={s.y1} x2={s.x2} y2={s.y2} stroke-width={s.w} />
      {/each}
    </g>
  {:else if provider === 'codex'}
    <g
      fill="none"
      stroke="currentColor"
      stroke-width="1.35"
      stroke-linejoin="round"
      transform="translate(12 12)"
    >
      {#each petalRotations as deg (deg)}
        <path d={PETAL} transform={`rotate(${deg})`} />
      {/each}
      <circle r="2.1" />
    </g>
  {:else}
    <g fill="none" stroke="currentColor" stroke-width="1.35">
      <circle cx="12" cy="12" r="9.2" />
    </g>
    <text
      class="monogram"
      x="12"
      y="12"
      text-anchor="middle"
      dominant-baseline="central"
      fill="currentColor">{monogram}</text
    >
  {/if}
</svg>

<style>
  .logo {
    display: block;
    flex: none;
    color: inherit;
    overflow: visible;
  }

  /* px inside a 24-unit viewBox, so the letter scales with the mark itself */
  .monogram {
    font-family: var(--font-sans);
    font-size: 11px;
    font-weight: 600;
  }
</style>
