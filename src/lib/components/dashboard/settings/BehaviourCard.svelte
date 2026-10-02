<!-- Settings → Behaviour: auto-hide, popover, refresh and start-up. [FRONTEND] -->
<script lang="ts">
  import Field from '../Field.svelte';
  import Toggle from '../Toggle.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { isAdvanced } from '$lib/settings-tiers';
  import { settings } from '$lib/stores/settings.svelte';
  import type { TrayDisplay } from '$lib/types';

  const TRAY_DISPLAYS: TrayDisplay[] = ['icon', 'percent'];

  const s = $derived(settings.value);
  const num = (e: Event) => Number((e.currentTarget as HTMLInputElement).value);
</script>

<SettingsCard id="behaviour" title={t('settings.behaviour')} reset="behaviour">
  <Field label={t('settings.autoHide')}>
    <Toggle
      checked={s.autoHide}
      label={t('settings.autoHide')}
      onchange={(v) => void settings.patch({ autoHide: v })}
    />
  </Field>

  <Field advanced={isAdvanced('autoHideDelayMs')} label={t('settings.autoHideDelayMs')}>
    <input
      class="field num"
      type="number"
      min="0"
      max="10000"
      step="100"
      value={s.autoHideDelayMs}
      disabled={!s.autoHide}
      onchange={(e) => void settings.patch({ autoHideDelayMs: Math.round(num(e)) })}
      aria-label={t('settings.autoHideDelayMs')}
    />
  </Field>

  <Field advanced={isAdvanced('popoverTimeoutSec')} label={t('settings.popoverTimeoutSec')} hint={t('settings.popoverTimeoutSec.hint')}>
    <input
      class="field num"
      type="number"
      min="0"
      max="600"
      step="1"
      value={s.popoverTimeoutSec}
      onchange={(e) => void settings.patch({ popoverTimeoutSec: Math.min(600, Math.max(0, Math.round(num(e)))) })}
      aria-label={t('settings.popoverTimeoutSec')}
    />
  </Field>

  <Field advanced={isAdvanced('collapsedWidth')} label={t('settings.collapsedWidth')}>
    <input
      class="field num"
      type="number"
      min="2"
      max="40"
      step="1"
      value={s.collapsedWidth}
      disabled={!s.autoHide}
      onchange={(e) => void settings.patch({ collapsedWidth: Math.round(num(e)) })}
      aria-label={t('settings.collapsedWidth')}
    />
  </Field>

  <Field label={t('settings.refreshIntervalSec')} hint={t('settings.refreshIntervalSec.hint')}>
    <input
      class="field num"
      type="number"
      min="15"
      max="3600"
      step="5"
      value={s.refreshIntervalSec}
      onchange={(e) => void settings.patch({ refreshIntervalSec: Math.round(num(e)) })}
      aria-label={t('settings.refreshIntervalSec')}
    />
  </Field>

  <Field advanced={isAdvanced('adaptiveRefresh')} label={t('settings.adaptiveRefresh')} hint={t('settings.adaptiveRefresh.hint')}>
    <Toggle
      checked={s.adaptiveRefresh}
      label={t('settings.adaptiveRefresh')}
      onchange={(v) => void settings.patch({ adaptiveRefresh: v })}
    />
  </Field>

  <Field advanced={isAdvanced('trayDisplay')} label={t('settings.trayDisplay')} hint={t('settings.trayDisplay.hint')}>
    <select
      class="field"
      aria-label={t('settings.trayDisplay')}
      value={s.trayDisplay}
      onchange={(e) => void settings.patch({ trayDisplay: e.currentTarget.value as TrayDisplay })}
    >
      {#each TRAY_DISPLAYS as v (v)}<option value={v}>{tDyn(`settings.trayDisplay.${v}`)}</option>{/each}
    </select>
  </Field>

  <Field label={t('settings.autostart')}>
    <Toggle
      checked={s.autostart}
      label={t('settings.autostart')}
      onchange={(v) => void settings.patch({ autostart: v })}
    />
  </Field>
</SettingsCard>
