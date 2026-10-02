<!-- Settings → Providers: track / show on the bar / order. [FRONTEND] -->
<script lang="ts">
  import { onMount } from 'svelte';
  import Toggle from '../Toggle.svelte';
  import Field from '../Field.svelte';
  import SettingsBlock from './SettingsBlock.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import ProviderLogo from '$lib/components/ProviderLogo.svelte';
  import { getProviders } from '$lib/api';
  import { t } from '$lib/i18n/i18n.svelte';
  import { providerDisplayName } from '$lib/providers';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import type { ProviderId, ProviderInfo } from '$lib/types';

  const s = $derived(settings.value);

  let providerInfos = $state<ProviderInfo[]>([]);
  let error = $state<string | null>(null);

  onMount(() => {
    void getProviders().then((p) => (providerInfos = p)).catch((e) => (error = String(e)));
  });

  /** Provider rows: every provider the backend reports, ordered by settings. */
  const providerRows = $derived.by(() => {
    const ids = new Set<ProviderId>((snapshot.value?.providers ?? []).map((p) => p.provider));
    for (const info of providerInfos) ids.add(info.id);
    for (const id of Object.keys(s.providers)) ids.add(id as ProviderId);
    return [...ids].sort((a, b) => (s.providers[a]?.order ?? 0) - (s.providers[b]?.order ?? 0));
  });

  /** `get_providers` is the authority on which providers exist and their state. */
  const providerInfoOf = (id: ProviderId) => providerInfos.find((p) => p.id === id) ?? null;

  const providerName = (id: ProviderId) =>
    snapshot.value?.providers.find((p) => p.provider === id)?.displayName ??
    providerInfos.find((p) => p.id === id)?.displayName ??
    providerDisplayName(id);

  /** Swap the `order` of two adjacent providers. */
  async function move(id: ProviderId, delta: -1 | 1) {
    const list = providerRows;
    const i = list.indexOf(id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= list.length) return;
    const order = [...list];
    [order[i], order[j]] = [order[j], order[i]];
    await settings.patch({ providers: Object.fromEntries(order.map((provider, index) => [provider, { order: index }])) });
  }

  const searchText = $derived(
    [...providerRows.map(providerName), t('settings.providers.hint'), t('settings.providerEnabled'), t('settings.sidebarItems.provider'), t('settings.openrouterKeyEnv'), t('settings.openrouterKeyEnv.hint')].join(' ')
  );
</script>

<SettingsCard id="providers" title={t('settings.providers')} reset="providers">
  {#if error}<p class="err" role="alert">{t('common.error', { message: error })}</p>{/if}
  <SettingsBlock text={searchText}>
    {#each providerRows as id, i (id)}
      {@const info = providerInfoOf(id)}
      <div class="prow">
        <span class="plogo"><ProviderLogo provider={id} size={20} /></span>
        <span class="pname">{providerName(id)}</span>
        {#if info?.experimental}
          <span class="badge" title={t('settings.provider.experimental.hint')}>
            {t('settings.provider.experimental')}
          </span>
        {/if}
        <button class="btn icon" disabled={i === 0} onclick={() => void move(id, -1)} aria-label={t('common.up')}>↑</button>
        <button class="btn icon" disabled={i === providerRows.length - 1} onclick={() => void move(id, 1)} aria-label={t('common.down')}>↓</button>
        <span class="pcol" title={t('settings.providerEnabled')}>
          <span class="pcap">{t('settings.providerEnabled.short')}</span>
          <Toggle
            checked={s.providers[id]?.enabled ?? true}
            label={`${providerName(id)} — ${t('settings.providerEnabled')}`}
            onchange={(v) => void settings.patchProvider(id, { enabled: v })}
          />
        </span>
        <span class="pcol" title={t('settings.sidebarItems.provider')}>
          <span class="pcap">{t('settings.sidebarItems.provider.short')}</span>
          <Toggle
            checked={s.providers[id]?.showInSidebar ?? true}
            label={`${providerName(id)} — ${t('settings.sidebarItems.provider')}`}
            disabled={!(s.providers[id]?.enabled ?? true)}
            onchange={(v) => void settings.patchProvider(id, { showInSidebar: v })}
          />
        </span>
      </div>
    {/each}
    <p class="note">{t('settings.providers.hint')}</p>
    {#if s.providers.openrouter}
      <Field label={t('settings.openrouterKeyEnv')} hint={t('settings.openrouterKeyEnv.hint')}>
        <input
          class="field"
          type="text"
          spellcheck="false"
          autocomplete="off"
          maxlength="100"
          value={s.openrouterKeyEnv}
          aria-label={t('settings.openrouterKeyEnv')}
          onchange={(e) => void settings.patch({ openrouterKeyEnv: e.currentTarget.value.trim() })}
        />
      </Field>
    {/if}
  </SettingsBlock>
</SettingsCard>

<style>
  .prow {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4375rem 0;
    border-bottom: 1px solid var(--border);
  }

  .prow:last-of-type {
    border-bottom: none;
  }

  .plogo {
    display: grid;
    place-items: center;
    width: 1.75rem;
    height: 1.75rem;
    border-radius: 999px;
    background: var(--surface-2);
    flex: none;
  }

  .pname {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* "experimental": the provider's quota source could not be verified against
     a live account, so the row says so rather than the README alone */
  .badge {
    flex: none;
    padding: 0.0625rem 0.375rem;
    border: 1px solid var(--border-strong);
    border-radius: 999px;
    font-size: 0.625rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--warn);
    white-space: nowrap;
  }

  /* the two provider switches need a caption each to be told apart */
  .pcol {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.125rem;
    flex: none;
  }

  .pcap {
    font-size: 0.625rem;
    line-height: 1;
    color: var(--muted);
    white-space: nowrap;
  }
</style>
