<!--
  The price list editor and the price-list update actions (check / apply / use
  source). A source application must never overwrite edits that were not saved,
  so the unsaved state lives here next to the buttons it disables. [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { getPricing, setPricing, useSourcePricing } from '$lib/api';
  import { formatAgo } from '$lib/format';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { pricingUpdate } from '$lib/stores/pricing-update.svelte';
  import type { PricingEntry, PricingTable } from '$lib/types';

  let pricing = $state<PricingTable | null>(null);
  let pricingSaved = $state(false);
  let pricingLoading = $state(true);
  let pricingSaving = $state(false);
  let pricingError = $state<string | null>(null);
  let pricingDirty = $state(false);
  let savedTimer: ReturnType<typeof setTimeout> | undefined;

  const pu = $derived(pricingUpdate.value);

  onMount(() => {
    const dispose = pricingUpdate.init();
    void loadPricing();
    return () => {
      clearTimeout(savedTimer);
      dispose();
    };
  });

  async function loadPricing() {
    pricingLoading = true;
    pricingError = null;
    try { pricing = await getPricing(); }
    catch (e) { pricingError = String(e); }
    finally { pricingLoading = false; }
  }

  async function savePricing() {
    if (!pricing || pricingSaving) return;
    pricingError = null;
    const patterns = new Set<string>();
    for (const entry of pricing.entries) {
      const pattern = entry.modelPattern.trim().toLowerCase();
      const prices = [entry.inputPerM, entry.outputPerM, entry.cacheWritePerM, entry.cacheReadPerM];
      if (!pattern || patterns.has(pattern) || prices.some((value) => !Number.isFinite(value) || value < 0)) {
        pricingError = t('settings.pricing.invalid');
        return;
      }
      patterns.add(pattern);
    }
    pricingSaving = true;
    try {
      pricing = await setPricing({ ...pricing, entries: pricing.entries.map((entry) => ({ ...entry, modelPattern: entry.modelPattern.trim() })) });
      pricingSaved = true;
      pricingDirty = false;
      clearTimeout(savedTimer);
      savedTimer = setTimeout(() => (pricingSaved = false), 1500);
    } catch (e) { pricingError = String(e); }
    finally { pricingSaving = false; }
  }

  async function checkPricingUpdates() {
    await pricingUpdate.check();
  }

  /** A source application must never overwrite edits that have not been saved. */
  async function applyPricingUpdate() {
    if (pricingDirty) return;
    const table = await pricingUpdate.apply();
    if (!table || pricingDirty) return;
    pricing = table;
    pricingSaved = true;
    clearTimeout(savedTimer);
    savedTimer = setTimeout(() => (pricingSaved = false), 1500);
  }

  /** Deliberately separate from Apply: this discards the active manual table. */
  async function switchToSourcePricing() {
    if (pricingDirty) return;
    if (!window.confirm(t('settings.pricing.useSourceConfirm'))) return;
    pricingError = null;
    try {
      const table = await useSourcePricing();
      if (!pricingDirty) pricing = table;
      await pricingUpdate.check();
    } catch (e) { pricingError = String(e); }
  }

  function addPricingRow() {
    if (!pricing) return;
    const entry: PricingEntry = {
      modelPattern: '',
      inputPerM: 0,
      outputPerM: 0,
      cacheWritePerM: 0,
      cacheReadPerM: 0,
    };
    pricing = { ...pricing, entries: [...pricing.entries, entry] };
    pricingDirty = true;
    pricingSaved = false;
  }

  function removePricingRow(index: number) {
    if (!pricing) return;
    pricing = { ...pricing, entries: pricing.entries.filter((_, i) => i !== index) };
    pricingDirty = true;
    pricingSaved = false;
  }

  function editPricing(index: number, key: keyof PricingEntry, value: string) {
    if (!pricing) return;
    const entries = pricing.entries.map((e, i) =>
      i === index
        ? { ...e, [key]: key === 'modelPattern' ? value : (value === '' ? NaN : Number(value)) }
        : e
    );
    pricing = { ...pricing, entries };
    pricingDirty = true;
    pricingSaved = false;
  }
</script>

<div class="pricing">
  <header class="pricing-head">
    <span class="label">{t('settings.pricing')}</span>
    {#if pricing?.updatedAt}
      <span class="muted small">
        {t('settings.pricing.updated', { when: new Date(pricing.updatedAt).toLocaleDateString() })}
      </span>
    {/if}
  </header>

  {#if pricingError}
    <p class="err" role="alert">{t('common.error', { message: pricingError })}</p>
    {#if !pricing}<button class="btn" onclick={() => void loadPricing()} disabled={pricingLoading}>{t('common.retry')}</button>{/if}
  {/if}
  {#if pricingLoading}
    <p class="muted small">{t('common.loading')}</p>
  {:else if pricing?.entries.length === 0}
    <p class="muted small">{t('settings.pricing.none')}</p>
  {:else if pricing}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>{t('settings.pricing.model')}</th>
            <th class="n">{t('settings.pricing.input')}</th>
            <th class="n">{t('settings.pricing.output')}</th>
            <th class="n">{t('settings.pricing.cacheWrite')}</th>
            <th class="n">{t('settings.pricing.cacheRead')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each pricing.entries as e, i (i)}
            <tr>
              <td>
                <input
                  class="field pattern"
                  value={e.modelPattern}
                  disabled={pricingSaving}
                  oninput={(ev) => editPricing(i, 'modelPattern', ev.currentTarget.value)}
                  aria-label={t('settings.pricing.model')}
                />
              </td>
              {#each [['inputPerM', 'settings.pricing.input'], ['outputPerM', 'settings.pricing.output'], ['cacheWritePerM', 'settings.pricing.cacheWrite'], ['cacheReadPerM', 'settings.pricing.cacheRead']] as [key, label] (key)}
                <td class="n">
                  <input
                    class="field price"
                    type="number"
                    min="0"
                    step="0.01"
                    value={e[key as keyof PricingEntry]}
                    disabled={pricingSaving}
                    oninput={(ev) => editPricing(i, key as keyof PricingEntry, ev.currentTarget.value)}
                    aria-label={tDyn(label)}
                  />
                </td>
              {/each}
              <td class="n">
                <button class="btn icon" onclick={() => removePricingRow(i)} disabled={pricingSaving} aria-label={t('common.remove')}>×</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <div class="pricing-actions">
    {#if pricingDirty}<span class="muted small">{t('settings.pricing.unsaved')}</span>{/if}
    <button class="btn" onclick={() => void checkPricingUpdates()} disabled={pricingSaving || pu?.checking}>
      {pu?.checking ? t('pricingUpdate.checking') : t('pricingUpdate.check')}
    </button>
    {#if pu?.customPricing}
      <span class="muted small">{t('settings.pricing.customSource')}</span>
      <button
        class="btn"
        onclick={() => void switchToSourcePricing()}
        disabled={pricingSaving || pricingDirty}
        title={pricingDirty ? t('settings.pricing.saveFirst') : undefined}
      >{t('settings.pricing.useSource')}</button>
    {:else}
      {#if pu?.available}
        <button
          class="btn btn-primary"
          onclick={() => void applyPricingUpdate()}
          disabled={pricingSaving || pricingDirty || pu.applying}
          title={pricingDirty ? t('settings.pricing.saveFirst') : undefined}
        >{pu.applying ? t('pricingUpdate.applying') : t('pricingUpdate.apply')}</button>
      {/if}
    {/if}
    <button class="btn" onclick={addPricingRow} disabled={!pricing || pricingSaving}>{t('settings.pricing.add')}</button>
    <button
      class="btn btn-primary"
      onclick={() => void savePricing()}
      disabled={!pricing || pricingSaving}
      title={t('settings.pricing.saveCustomHint')}
    >
      {pricingSaving ? t('common.saving') : pricingSaved ? t('common.saved') : t('settings.pricing.save')}
    </button>
  </div>
  <div class="pricing-update-status" role="status">
    {#if pu?.error || pricingUpdate.error}
      <span class="err">{t('common.error', { message: pu?.error ?? pricingUpdate.error ?? '' })}</span>
    {:else if pu?.checking}
      <span class="muted small">{t('pricingUpdate.checking')}</span>
    {:else if pu?.customPricing && pu.available}
      <span class="offer">{t('pricingUpdate.customAvailable', { revision: pu.revision ?? '—' })}</span>
    {:else if pu?.customPricing}
      <span class="muted small">{t('settings.pricing.customSource')}</span>
    {:else if pu?.available}
      <span class="offer">{t('pricingUpdate.available', { revision: pu.revision ?? '—' })}</span>
    {:else if pu?.checkedAt}
      <span class="muted small">{t('pricingUpdate.upToDate', { ago: formatAgo(pu.checkedAt) })}</span>
    {:else}
      <span class="muted small">{t('pricingUpdate.unknown')}</span>
    {/if}
  </div>
</div>

<style>
  .pricing {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding-top: 0.75rem;
    margin-top: 0.25rem;
    border-top: 1px solid var(--border);
  }

  .pricing-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .label {
    font-weight: 500;
  }

  .table-wrap {
    overflow-x: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.8125rem;
  }

  th {
    text-align: left;
    font-weight: 500;
    color: var(--muted);
    padding: 0 0.25rem 0.25rem;
    white-space: nowrap;
  }

  th.n,
  td.n {
    text-align: right;
  }

  td {
    padding: 0.125rem 0.25rem;
  }

  .pattern {
    width: 100%;
    min-width: 10rem;
  }

  .price {
    width: 5.25rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .pricing-actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
    align-items: center;
    flex-wrap: wrap;
  }

  .pricing-update-status {
    min-height: 1.1rem;
    font-size: 0.75rem;
  }

  /* an offer is worth noticing, but this is not an alert */
  .offer {
    color: var(--focus);
    font-weight: 500;
  }
</style>
