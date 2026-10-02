<!--
  One row per provider CLI: what is missing (CLI folder not found / not signed
  in / ok) and, when something is, the login command with copy buttons. Used by
  the Overview getting-started card and the first-run wizard. The data comes
  from `get_provider_setup` (existence checks only) combined with the snapshot
  status; no credential is ever read. [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import ProviderLogo from '$lib/components/ProviderLogo.svelte';
  import { copyText } from '$lib/external';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { providerHealth } from '$lib/onboarding';
  import { accentFor } from '$lib/stores/rings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import type { ProviderSetup } from '$lib/types';

  let { setups }: { setups: ProviderSetup[] } = $props();

  let copied = $state<string | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | null = null;
  onMount(() => () => { if (copyTimer) clearTimeout(copyTimer); });

  const rows = $derived(
    setups.map((setup) => {
      const quota = snapshot.value?.providers.find((p) => p.provider === setup.provider);
      return { setup, name: quota?.displayName ?? setup.provider, health: providerHealth(setup, quota) };
    })
  );

  async function copy(text: string) {
    if (await copyText(text)) {
      copied = text;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copied = null), 1600);
    }
  }
</script>

<ul class="providers">
  {#each rows as row (row.setup.provider)}
    <li class="row" data-health={row.health}>
      <span class="logo" style:color={accentFor(row.setup.provider, 0)}>
        <ProviderLogo provider={row.setup.provider} size={22} />
      </span>
      <div class="what">
        <span class="name">{row.name}</span>
        <span class="state">{tDyn(`gs.state.${row.health}`, { dir: row.setup.configDir })}</span>
        {#if row.health === 'cli-missing' || row.health === 'not-signed-in'}
          <div class="login">
            <span class="muted">{t('gs.signIn')}</span>
            {#each row.setup.loginSteps as step, i (step)}
              {#if i > 0}<span class="muted">{t('gs.then')}</span>{/if}
              <span class="cmd">
                <code>{step}</code>
                <button class="btn copy" onclick={() => void copy(step)} aria-label={`${t('gs.copy')}: ${step}`}>
                  {copied === step ? t('gs.copied') : t('gs.copy')}
                </button>
              </span>
            {/each}
          </div>
        {/if}
      </div>
    </li>
  {/each}
</ul>

<style>
  .providers {
    display: flex;
    flex-direction: column;
    gap: 0.625rem;
    margin: 0.25rem 0 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    padding: 0.625rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: var(--r-control);
    background: var(--surface-2);
  }

  .logo {
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    flex: none;
    border-radius: 999px;
    background: var(--surface);
  }

  .what {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
  }

  .name {
    font-weight: 600;
  }

  .state {
    font-size: 0.8125rem;
    color: var(--muted);
    overflow-wrap: anywhere;
  }

  .row[data-health='ok'] .state {
    color: var(--ok);
  }

  .row[data-health='error'] .state,
  .row[data-health='cli-missing'] .state {
    color: var(--warn);
  }

  .login {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.375rem 0.5rem;
    font-size: 0.8125rem;
  }

  .cmd {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
  }

  code {
    padding: 0.125rem 0.4375rem;
    border-radius: 0.375rem;
    background: var(--bg);
    border: 1px solid var(--border);
    font-family: var(--font-mono);
    font-size: 0.8125rem;
  }

  .copy {
    padding: 0.125rem 0.5rem;
    font-size: 0.75rem;
  }
</style>
