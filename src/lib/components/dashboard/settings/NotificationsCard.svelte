<!--
  Settings → Notifications: everything that decides when the app speaks up —
  the notification channels and thresholds (NotificationsPanel) and focus mode.
  Kept together on purpose so it can be replaced as one unit. [FRONTEND]
-->
<script lang="ts">
  import Field from '../Field.svelte';
  import Toggle from '../Toggle.svelte';
  import NotificationsPanel from './NotificationsPanel.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import { t } from '$lib/i18n/i18n.svelte';
  import { isAdvanced } from '$lib/settings-tiers';
  import { settings } from '$lib/stores/settings.svelte';

  const s = $derived(settings.value);

  // Focus mode (do not disturb). The backend expires a timed focus by itself;
  // the dashboard only reads the deadline and writes a new one.
  let focusNow = $state(Date.now());
  const focusActive = $derived(s.focusUntil === -1 || s.focusUntil > focusNow);
  $effect(() => {
    focusNow = Date.now();
    const timer = setInterval(() => (focusNow = Date.now()), 30_000);
    return () => clearInterval(timer);
  });

  function focusTime(ms: number): string {
    return new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }

  function setFocus(choice: string): Promise<void> {
    const now = Date.now();
    let until = 0;
    if (choice === 'hour') until = now + 3_600_000;
    else if (choice === 'forever') until = -1;
    else if (choice === 'tomorrow') {
      const d = new Date(now);
      d.setDate(d.getDate() + 1);
      d.setHours(8, 0, 0, 0);
      until = d.getTime();
    } else if (choice !== 'off') return Promise.resolve();
    return settings.patch({ focusUntil: until });
  }
</script>

<SettingsCard id="notifications" title={t('settings.card.notifications')} reset="notifications">
  <NotificationsPanel />

  <Field
    label={t('settings.focus')}
    hint={focusActive
      ? s.focusUntil === -1
        ? t('settings.focus.active')
        : t('settings.focus.activeUntil', { time: focusTime(s.focusUntil) })
      : t('settings.focus.hint')}
  >
    <select
      aria-label={t('settings.focus')}
      class="field"
      value={focusActive ? (s.focusUntil === -1 ? 'forever' : 'timed') : 'off'}
      onchange={(e) => void setFocus(e.currentTarget.value)}
    >
      <option value="off">{t('settings.focus.off')}</option>
      <option value="hour">{t('settings.focus.hour')}</option>
      <option value="tomorrow">{t('settings.focus.tomorrow')}</option>
      <option value="forever">{t('settings.focus.forever')}</option>
      {#if focusActive && s.focusUntil !== -1}
        <option value="timed" disabled>{t('settings.focus.timed', { time: focusTime(s.focusUntil) })}</option>
      {/if}
    </select>
  </Field>

  <Field advanced={isAdvanced('focusHidesSidebar')} label={t('settings.focusHidesSidebar')}>
    <Toggle
      checked={s.focusHidesSidebar}
      label={t('settings.focusHidesSidebar')}
      onchange={(v) => void settings.patch({ focusHidesSidebar: v })}
    />
  </Field>

</SettingsCard>
