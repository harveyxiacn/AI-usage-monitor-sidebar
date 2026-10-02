<!--
  Settings → Advisor: the two opt-in decision-support settings. The advice
  itself (Overview card, popover hint, Plan advisor, Commits view) is always
  computed from data the app already has; these switches only decide whether it
  may also interrupt (notification) and whether a read-only `git log` may run.
  Standalone so the Settings tab can mount it wherever it wants:

    <AdvisorSettings />            card id `advisor`, section id `advisor`
                                   (settings-cards.ts CARD_KEYS.advisor)
  [FRONTEND]
-->
<script lang="ts">
  import Field from '../Field.svelte';
  import Toggle from '../Toggle.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import { t } from '$lib/i18n/i18n.svelte';
  import { settings } from '$lib/stores/settings.svelte';

  const s = $derived(settings.value);
</script>

<SettingsCard id="advisor" title={t('settings.card.advisor')} reset="advisor">
  <Field label={t('settings.advisorNotifications')} hint={t('settings.advisorNotifications.hint')}>
    <Toggle
      checked={s.advisorNotifications}
      disabled={!s.notifications}
      label={t('settings.advisorNotifications')}
      onchange={(v) => void settings.patch({ advisorNotifications: v })}
    />
  </Field>

  <Field label={t('settings.gitAttribution')} hint={t('settings.gitAttribution.hint')}>
    <Toggle
      checked={s.gitAttribution}
      label={t('settings.gitAttribution')}
      onchange={(v) => void settings.patch({ gitAttribution: v })}
    />
  </Field>
</SettingsCard>
