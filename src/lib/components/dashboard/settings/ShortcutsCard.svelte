<!--
  Settings → Shortcuts. The overlays are dock windows and never take focus, so a
  global shortcut is the only keyboard path to them. Empty = not registered. [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Field from '../Field.svelte';
  import ShortcutRecorder from './ShortcutRecorder.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import { getShortcutRegistrations } from '$lib/api';
  import { t } from '$lib/i18n/i18n.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import type { ShortcutRegistrations } from '$lib/types';

  const s = $derived(settings.value);

  const OFF = { state: 'off', message: null } as const;
  let registrations = $state<ShortcutRegistrations>({ toggleSidebar: OFF, openDashboard: OFF });
  let error = $state<string | null>(null);

  /** The backend re-registers on `settings-updated`; ask it what happened. */
  async function refresh() {
    try { registrations = await getShortcutRegistrations(); }
    catch (e) { error = String(e); }
  }

  onMount(() => void refresh());

  async function patchShortcut(key: 'shortcutToggleSidebar' | 'shortcutOpenDashboard', value: string) {
    await settings.patch({ [key]: value.trim() });
    await refresh();
  }
</script>

<SettingsCard id="shortcuts" title={t('settings.card.shortcuts')} reset="shortcuts" keywords={t('settings.shortcutHint')}>
  {#if error}<p class="err" role="alert">{t('common.error', { message: error })}</p>{/if}

  <Field label={t('settings.shortcutToggleSidebar')} hint={t('settings.shortcutHint')}>
    <ShortcutRecorder
      value={s.shortcutToggleSidebar}
      label={t('settings.shortcutToggleSidebar')}
      registration={registrations.toggleSidebar}
      onchange={(v) => void patchShortcut('shortcutToggleSidebar', v)}
    />
  </Field>

  <Field label={t('settings.shortcutOpenDashboard')} hint={t('settings.shortcutWayland')}>
    <ShortcutRecorder
      value={s.shortcutOpenDashboard}
      label={t('settings.shortcutOpenDashboard')}
      registration={registrations.openDashboard}
      onchange={(v) => void patchShortcut('shortcutOpenDashboard', v)}
    />
  </Field>

  {#each [registrations.toggleSidebar, registrations.openDashboard] as r, i (i)}
    {#if r.state === 'failed' && r.message}
      <p class="note">{t('settings.shortcutFailed', { message: r.message })}</p>
    {/if}
  {/each}
</SettingsCard>
