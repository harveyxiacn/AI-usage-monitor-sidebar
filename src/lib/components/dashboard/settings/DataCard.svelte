<!-- Settings → Data: log ingestion, budget, retention, subscription prices. [FRONTEND] -->
<script lang="ts">
  import Field from '../Field.svelte';
  import Toggle from '../Toggle.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import { reingestLogs } from '$lib/api';
  import { t } from '$lib/i18n/i18n.svelte';
  import { providerDisplayName } from '$lib/providers';
  import { planPriceHint } from '$lib/subscription';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';

  const s = $derived(settings.value);

  let rescanResult = $state<string | null>(null);
  let rescanError = $state<string | null>(null);
  let rescanning = $state(false);

  async function rescan() {
    rescanning = true;
    rescanError = null;
    rescanResult = null;
    try {
      const stats = await reingestLogs();
      rescanResult = t('history.ingestStats', { files: stats.filesScanned, updated: stats.filesUpdated, events: stats.eventsAdded, ms: stats.durationMs });
      if (stats.errors.length > 0) rescanError = stats.errors.join('\n');
    } catch (e) {
      rescanError = String(e);
    } finally {
      rescanning = false;
    }
  }
</script>

<SettingsCard id="data" title={t('settings.data')} reset="data">
  <Field label={t('settings.ingestEnabled')}>
    <Toggle
      checked={s.ingestEnabled}
      label={t('settings.ingestEnabled')}
      onchange={(v) => void settings.patch({ ingestEnabled: v })}
    />
  </Field>

  <Field label={t('settings.monthlyBudgetUsd')} hint={t('settings.monthlyBudgetHint')}>
    <input
      class="field num"
      type="number"
      min="0"
      max="1000000"
      step="1"
      value={s.monthlyBudgetUsd}
      aria-label={t('settings.monthlyBudgetUsd')}
      onchange={(e) => void settings.patch({ monthlyBudgetUsd: Math.min(1_000_000, Math.max(0, e.currentTarget.valueAsNumber || 0)) })}
    />
  </Field>

  <Field label={t('settings.quotaRetentionDays')} hint={t('settings.quotaRetentionHint')}>
    <input
      class="field num"
      type="number"
      min="0"
      max="3650"
      step="1"
      value={s.quotaRetentionDays}
      aria-label={t('settings.quotaRetentionDays')}
      onchange={(e) => void settings.patch({ quotaRetentionDays: Math.min(3650, Math.max(0, Math.round(e.currentTarget.valueAsNumber || 0))) })}
    />
  </Field>

  {#each ['claude', 'codex'] as id (id)}
    {@const quota = snapshot.value?.providers.find((p) => p.provider === id)}
    {@const suggestion = planPriceHint(id, quota?.planLabel ?? quota?.plan)}
    <Field
      label={t('settings.subscriptionUsd', { provider: providerDisplayName(id) })}
      hint={t('settings.subscriptionHint') + (suggestion ? ' ' + t('settings.subscriptionSuggest', { price: '$' + suggestion }) : '')}
    >
      <input
        class="field num"
        type="number"
        min="0"
        max="10000"
        step="1"
        value={s.subscriptionUsd[id] ?? 0}
        aria-label={t('settings.subscriptionUsd', { provider: providerDisplayName(id) })}
        onchange={(e) => void settings.patch({ subscriptionUsd: { [id]: Math.min(10_000, Math.max(0, e.currentTarget.valueAsNumber || 0)) } })}
      />
    </Field>
  {/each}

  <Field label={t('history.rescan')}>
    <button class="btn" disabled={rescanning} onclick={() => void rescan()}>
      {rescanning ? t('history.ingestRunning') : t('history.rescan')}
    </button>
  </Field>

  {#if rescanResult}<p class="muted small" role="status">{rescanResult}</p>{/if}
  {#if rescanError}<p class="err" role="alert">{t('common.error', { message: rescanError })}</p>{/if}
</SettingsCard>
