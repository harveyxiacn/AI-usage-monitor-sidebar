<!--
  Settings → Updates: app updates and price list updates in one place, with the
  same rules for both: a check only reads a small file from GitHub (or your own
  price list URL), and nothing is installed or applied without a click. [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Field from '../Field.svelte';
  import Toggle from '../Toggle.svelte';
  import PricingEditor from './PricingEditor.svelte';
  import SettingsBlock from './SettingsBlock.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import { openExternal } from '$lib/api';
  import { formatAgo } from '$lib/format';
  import { t } from '$lib/i18n/i18n.svelte';
  import { isAdvanced } from '$lib/settings-tiers';
  import { settings } from '$lib/stores/settings.svelte';
  import { update } from '$lib/stores/update.svelte';
  import { pricingUpdate } from '$lib/stores/pricing-update.svelte';

  const s = $derived(settings.value);
  const u = $derived(update.value);

  let actionError = $state<string | null>(null);

  onMount(() => update.init());

  const updateSummary = $derived.by(() => {
    if (!u) return t('update.unknown');
    if (u.checking) return t('update.checking');
    if (u.available) return t('update.available', { version: u.available });
    if (!u.checkedAt) return t('update.unknown');
    return t('update.upToDate');
  });

  async function openRelease(url: string) {
    actionError = null;
    try { await openExternal(url); } catch (e) { actionError = String(e); }
  }

  async function patchPricingUrl(value: string) {
    await settings.patch({ pricingUrl: value.trim() });
    // A different source cannot reuse the previous source's offer. The check
    // replaces the status immediately after the setting has been persisted.
    await pricingUpdate.check();
  }
</script>

<SettingsCard id="updates" title={t('settings.card.updates')} reset="updates" wide>
  <SettingsBlock text={`${t('settings.updates.app')} ${t('settings.updates.hint')}`}>
    <h4>{t('settings.updates.app')}</h4>
    <p class="note">{t('settings.updates.hint')}</p>
  </SettingsBlock>

  <Field label={t('settings.autoUpdateCheck')}>
    <Toggle
      checked={s.autoUpdateCheck}
      label={t('settings.autoUpdateCheck')}
      onchange={(v) => void settings.patch({ autoUpdateCheck: v })}
    />
  </Field>

  <!-- Updates are never installed without this click; a build owned by a
       package manager only gets the release link. -->
  <Field
    label={t('settings.about.programUpdates')}
    hint={u?.checkedAt ? t('update.checkedAt', { ago: formatAgo(u.checkedAt) }) : undefined}
  >
    <span class="upd-state" class:offer={!!u?.available}>{updateSummary}</span>
    <button class="btn" disabled={u?.checking || u?.installing} onclick={() => void update.check()}>
      {t('update.check')}
    </button>
  </Field>

  {#if u?.available}
    <SettingsBlock text={`${t('update.available', { version: u.available })} ${t('update.install')} ${t('update.notes')}`}>
      <p class="muted small">
        {#if u.canInstall}
          <button class="btn btn-primary" disabled={u.installing} onclick={() => void update.install()}>
            {u.installing ? t('update.installing') : t('update.install')}
          </button>
        {:else}
          {t('update.managed')}
        {/if}
        <button class="btn link" onclick={() => void openRelease(u.releaseUrl)}>
          {u.canInstall ? t('update.notes') : t('update.openRelease')}
        </button>
      </p>
    </SettingsBlock>
  {/if}
  {#if update.error}<p class="err" role="alert">{t('common.error', { message: update.error })}</p>{/if}
  {#if actionError}<p class="err" role="alert">{t('common.error', { message: actionError })}</p>{/if}

  <SettingsBlock advanced text={t('settings.updates.prices')}>
    <h4 class="second">{t('settings.updates.prices')}</h4>
  </SettingsBlock>

  <Field advanced={isAdvanced('autoPricingCheck')} label={t('settings.autoPricingCheck')} hint={t('settings.autoPricingCheck.hint')}>
    <Toggle
      checked={s.autoPricingCheck}
      label={t('settings.autoPricingCheck')}
      onchange={(v) => void settings.patch({ autoPricingCheck: v })}
    />
  </Field>

  <Field advanced={isAdvanced('pricingUrl')} label={t('settings.pricingUrl')} hint={t('settings.pricingUrl.hint')} wide>
    <input
      class="field url"
      type="url"
      inputmode="url"
      placeholder={t('settings.pricingUrl.placeholder')}
      value={s.pricingUrl}
      onchange={(e) => void patchPricingUrl((e.currentTarget as HTMLInputElement).value)}
      aria-label={t('settings.pricingUrl')}
    />
  </Field>

  <SettingsBlock advanced text={`${t('settings.pricing')} ${t('settings.pricing.model')} ${t('settings.pricing.input')} ${t('settings.pricing.output')} ${t('settings.pricing.cacheWrite')} ${t('settings.pricing.cacheRead')} ${t('pricingUpdate.check')}`}>
    <PricingEditor />
  </SettingsBlock>
</SettingsCard>

<style>
  h4 {
    margin: 0.25rem 0 0;
    font-size: 0.8125rem;
    font-weight: 600;
  }

  h4.second {
    margin-top: 0.875rem;
    padding-top: 0.75rem;
    border-top: 1px solid var(--border);
  }

  /* an offer is worth noticing, but this is not an alert */
  .upd-state.offer {
    color: var(--focus);
    font-weight: 500;
  }

  .url {
    width: 100%;
    min-width: 0;
  }
</style>
