<!--
  Provider status badge, shared by the ring and the mini-bar. [FRONTEND]
  Differs by *shape*, not only by colour: "!" (not signed in), a padlock (login
  expired), a clock (rate-limited) or a solid red dot (error). Renders nothing
  for a healthy provider. Decorative: the group's aria-label already says the
  status. `inline` flows with the text instead of sitting on the ring's
  1-o'clock edge.
-->
<script lang="ts">
  import type { ProviderStatus } from '$lib/types';

  interface Props {
    status: ProviderStatus;
    inline?: boolean;
  }

  let { status, inline = false }: Props = $props();

  const badge = $derived(
    status === 'not_logged_in'
      ? 'warn'
      : status === 'token_expired'
        ? 'lock'
        : status === 'rate_limited'
          ? 'clock'
          : status === 'error'
            ? 'dot'
            : null
  );
</script>

{#if badge === 'warn'}
  <span class="badge badge-warn" class:inline aria-hidden="true">!</span>
{:else if badge === 'lock'}
  <span class="badge badge-warn" class:inline aria-hidden="true">
    <svg viewBox="0 0 10 10" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.2">
      <rect x="2" y="4.6" width="6" height="4.2" rx="0.8" fill="currentColor" stroke="none" />
      <path d="M3.2 4.6V3.4a1.8 1.8 0 0 1 3.6 0v1.2" stroke-linecap="round" />
    </svg>
  </span>
{:else if badge === 'clock'}
  <span class="badge badge-warn" class:inline aria-hidden="true">
    <svg viewBox="0 0 10 10" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round">
      <circle cx="5" cy="5" r="3.9" />
      <path d="M5 2.8V5l1.5 1" />
    </svg>
  </span>
{:else if badge === 'dot'}
  <span class="badge badge-dot" class:inline aria-hidden="true"></span>
{/if}

<style>
  .badge {
    position: absolute;
    /* sit on the ring's 1-o'clock edge, half outside the circle */
    top: -0.0625rem;
    right: -0.0625rem;
    display: grid;
    place-items: center;
    flex: none;
    border-radius: 999px;
    box-shadow: 0 0 0 0.125rem rgb(var(--bar-bg-rgb) / 0.9);
  }

  .badge.inline {
    position: static;
    box-shadow: none;
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

  .badge-warn svg {
    display: block;
    width: 0.625rem;
    height: 0.625rem;
  }

  /* error: a solid red dot — the one badge with no glyph */
  .badge-dot {
    width: 0.625rem;
    height: 0.625rem;
    background: var(--critical);
  }
</style>
