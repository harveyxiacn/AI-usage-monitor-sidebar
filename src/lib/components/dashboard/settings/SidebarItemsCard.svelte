<!-- Settings → Sidebar items: what the bar draws (hidden items are still tracked). [FRONTEND] -->
<script lang="ts">
  import Field from '../Field.svelte';
  import Toggle from '../Toggle.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { defaultSidebarItems, settings } from '$lib/stores/settings.svelte';
  import type { SidebarItems } from '$lib/types';

  const s = $derived(settings.value);

  /** "Sidebar items" rows, in the order they are declared in the contract. */
  const SIDEBAR_ITEM_KEYS = Object.keys(defaultSidebarItems) as (keyof SidebarItems)[];
</script>

<SettingsCard id="sidebarItems" title={t('settings.sidebarItems')} reset="sidebarItems" keywords={t('settings.sidebarItems.hint')}>
  <p class="note">{t('settings.sidebarItems.hint')}</p>
  {#each SIDEBAR_ITEM_KEYS as key (key)}
    <Field label={tDyn(`settings.sidebarItems.${key}`)}>
      <Toggle
        checked={s.sidebarItems[key]}
        label={tDyn(`settings.sidebarItems.${key}`)}
        disabled={key === 'scoped' && s.ringMode !== 'concentric'}
        onchange={(v) => void settings.patch({ sidebarItems: { [key]: v } })}
      />
    </Field>
  {/each}
</SettingsCard>
