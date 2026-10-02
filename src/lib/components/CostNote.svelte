<!--
  The one place that words "this cost is an estimate". Renders inline text for
  a surrounding `<p class="note">`, and adds a subtle remark when the price
  table behind the estimate was last checked more than 60 days ago. [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n/i18n.svelte';
  import type { PricingStaleness } from '$lib/pricing-age';
  import { loadPricingStaleness } from '$lib/stores/pricing-freshness';

  let stale = $state<PricingStaleness | null>(null);

  onMount(() => {
    let alive = true;
    void loadPricingStaleness().then((s) => {
      if (alive) stale = s;
    });
    return () => (alive = false);
  });
</script>

<span class="cost-note">{t('history.costNote')}</span>{#if stale}
  <span class="price-stale">{t('history.priceStale', { date: stale.date })}</span>{/if}

<style>
  .price-stale {
    font-style: italic;
    opacity: 0.85;
  }
</style>
