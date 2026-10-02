<!--
  Settings rows for the sidebar's look beyond rings and colours. [FRONTEND]

  Standalone on purpose: it renders plain `Field` rows (no card of its own) so
  the Appearance / Sidebar card of the settings tab can mount it with
  `<SidebarStyleControls />` and nothing else. It reads and patches the shared
  settings store itself.

    labelContent      percentage | reset countdown | both
    ringStyle         rings | compact bars
    sidebarAnimations pulse on threshold crossings, flash on resets
-->
<script lang="ts">
  import Field from '../Field.svelte';
  import Toggle from '../Toggle.svelte';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import type { LabelContent, RingStyle } from '$lib/types';

  const LABEL_CONTENTS: LabelContent[] = ['percent', 'reset', 'both'];
  const RING_STYLES: RingStyle[] = ['ring', 'bar'];

  const s = $derived(settings.value);
</script>

<Field label={t('settings.ringStyle')} hint={t('settings.ringStyle.hint')}>
  <select
    aria-label={t('settings.ringStyle')}
    class="field"
    value={s.ringStyle}
    onchange={(e) => void settings.patch({ ringStyle: e.currentTarget.value as RingStyle })}
  >
    {#each RING_STYLES as v (v)}<option value={v}>{tDyn(`settings.ringStyle.${v}`)}</option>{/each}
  </select>
</Field>

<Field label={t('settings.labelContent')} hint={t('settings.labelContent.hint')}>
  <select
    aria-label={t('settings.labelContent')}
    class="field"
    value={s.labelContent}
    disabled={!s.sidebarItems.percentLabel}
    onchange={(e) => void settings.patch({ labelContent: e.currentTarget.value as LabelContent })}
  >
    {#each LABEL_CONTENTS as v (v)}<option value={v}>{tDyn(`settings.labelContent.${v}`)}</option>{/each}
  </select>
</Field>

<Field label={t('settings.sidebarAnimations')} hint={t('settings.sidebarAnimations.hint')}>
  <Toggle
    checked={s.sidebarAnimations}
    label={t('settings.sidebarAnimations')}
    onchange={(v) => void settings.patch({ sidebarAnimations: v })}
  />
</Field>
