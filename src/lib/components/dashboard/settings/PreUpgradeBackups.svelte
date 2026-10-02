<!--
  Settings → Integrations: the automatic backups taken before a database
  upgrade, with "Reveal in folder". Restoring one is a CLI step
  (`--restore-pre-upgrade`), see the hint. [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { listPreUpgradeBackups, revealPreUpgradeBackup } from '$lib/api';
  import { t } from '$lib/i18n/i18n.svelte';
  import type { PreUpgradeBackup } from '$lib/types';

  let backups = $state<PreUpgradeBackup[]>([]);
  let error = $state<string | null>(null);

  onMount(() => {
    void listPreUpgradeBackups().then((b) => (backups = b)).catch((e) => (error = String(e)));
  });

  /** 20261002-101500 -> a local date and time */
  function when(stamp: string): string {
    const m = /^(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})$/.exec(stamp);
    if (!m) return stamp;
    const [y, mo, d, h, mi, s] = m.slice(1).map(Number);
    return new Date(y, mo - 1, d, h, mi, s).toLocaleString();
  }

  async function reveal(name: string) {
    error = null;
    try { await revealPreUpgradeBackup(name); } catch (e) { error = String(e); }
  }
</script>

<h3 class="sub">{t('preUpgrade.title')}</h3>
<p class="hint">{t('preUpgrade.hint')}</p>
{#if backups.length === 0}
  <p class="hint">{t('preUpgrade.none')}</p>
{:else}
  <ul class="list">
    {#each backups as b (b.name)}
      <li>
        <span>{t('preUpgrade.item', { from: b.fromVersion, to: b.toVersion, time: when(b.stamp) })}</span>
        <button class="btn" type="button" onclick={() => void reveal(b.name)}>{t('preUpgrade.reveal')}</button>
      </li>
    {/each}
  </ul>
{/if}
{#if error}
  <p class="err" role="alert">{t('common.error', { message: error })}</p>
{/if}

<style>
  h3.sub {
    margin: 0.875rem 0 0.25rem;
    font-size: 0.875rem;
    font-weight: 600;
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .list li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.375rem 0;
    border-bottom: 1px solid var(--border);
  }

  .list li:last-child {
    border-bottom: none;
  }

  .hint {
    margin: 0.25rem 0;
    font-size: 0.75rem;
    color: var(--muted);
  }

  .err {
    margin: 0.5rem 0 0;
    font-size: 0.8125rem;
    overflow-wrap: anywhere;
    color: var(--critical, var(--warn));
  }
</style>
