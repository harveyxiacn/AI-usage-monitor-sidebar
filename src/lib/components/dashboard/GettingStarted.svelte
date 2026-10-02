<!--
  Overview → "Get started". Shown while no provider delivers quota data: per
  provider what is missing (CLI folder not found / not signed in / ok), the
  login command with a copy button, a "Check again" button and a one-line
  privacy note. Nothing here reads a credential: the backend only reports
  whether the CLI's folders exist. [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import ProviderSetupList from './ProviderSetupList.svelte';
  import { getProviderSetup } from '$lib/api';
  import { t } from '$lib/i18n/i18n.svelte';
  import { needsGettingStarted } from '$lib/onboarding';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import type { ProviderSetup } from '$lib/types';

  let setups = $state<ProviderSetup[]>([]);
  let checking = $state(false);

  async function load() {
    try {
      setups = await getProviderSetup();
    } catch {
      setups = [];
    }
  }

  onMount(() => void load());

  const visible = $derived(needsGettingStarted(snapshot.value?.providers, snapshot.loading));

  async function checkAgain() {
    checking = true;
    try {
      await Promise.all([snapshot.refresh(), load()]);
    } finally {
      checking = false;
    }
  }
</script>

{#if visible}
  <article class="card start" aria-labelledby="gs-title">
    <header>
      <h3 id="gs-title">{t('gs.title')}</h3>
      <button class="btn btn-primary" disabled={checking || snapshot.refreshing !== null} onclick={() => void checkAgain()}>
        {checking ? t('gs.checking') : t('gs.check')}
      </button>
    </header>
    <p class="muted intro">{t('gs.intro')}</p>
    <ProviderSetupList {setups} />
    <p class="privacy muted">{t('gs.privacy')}</p>
  </article>
{/if}

<style>
  .start {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 1rem;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }

  h3 {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
  }

  .intro {
    margin: 0;
    font-size: 0.8125rem;
  }

  .privacy {
    margin: 0.25rem 0 0;
    font-size: 0.75rem;
  }
</style>
