<!--
  "Will run out" indicator: a small red hourglass shown while an arc / segment
  is projected to hit 100 % before its reset. Shape-coded (not just red) and
  decorative: the group's aria-label already carries the forecast text.
  `inline` flows with the text instead of sitting on the ring's 11 o'clock edge.
  [FRONTEND]
-->
<script lang="ts">
  import { t } from '$lib/i18n/i18n.svelte';

  let { inline = false }: { inline?: boolean } = $props();
</script>

<span class="runout" class:inline title={t('ring.runsOut')} aria-hidden="true">
  <svg viewBox="0 0 10 10" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round" stroke-linecap="round">
    <path d="M2.5 1.2h5M2.5 8.8h5M3 1.2c0 2.2 2 2.6 2 3.8S3 6.6 3 8.8M7 1.2c0 2.2-2 2.6-2 3.8s2 1.6 2 3.8" />
  </svg>
</span>

<style>
  .runout {
    position: absolute;
    top: -0.0625rem;
    left: -0.0625rem;
    display: grid;
    place-items: center;
    flex: none;
    width: 0.875rem;
    height: 0.875rem;
    border-radius: 999px;
    background: var(--critical);
    color: #fff;
    box-shadow: 0 0 0 0.125rem rgb(var(--bar-bg-rgb) / 0.9);
  }

  .runout.inline {
    position: static;
    box-shadow: none;
  }

  .runout svg {
    display: block;
    width: 0.625rem;
    height: 0.625rem;
  }
</style>
