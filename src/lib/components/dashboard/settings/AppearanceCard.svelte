<!-- Settings → Appearance. Every control applies immediately. [FRONTEND] -->
<script lang="ts">
  import Field from '../Field.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import SidebarStyleControls from './SidebarStyleControls.svelte';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import type {
    CyberAccent,
    Language,
    PercentMode,
    PercentPosition,
    RingMode,
    SurfaceStyle,
    Theme,
  } from '$lib/types';

  const s = $derived(settings.value);

  const THEMES: Theme[] = ['auto', 'dark', 'light'];
  const LANGUAGES: Language[] = ['auto', 'en', 'zh-CN'];
  const RING_MODES: RingMode[] = ['concentric', 'primary', 'all'];
  const SURFACE_STYLES: SurfaceStyle[] = ['glass', 'solid', 'cyber'];
  const CYBER_ACCENTS: CyberAccent[] = ['neon', 'matrix', 'amber', 'ice', 'synthwave'];
  const PERCENT_MODES: PercentMode[] = ['used', 'remaining'];
  const PERCENT_POSITIONS: PercentPosition[] = ['below', 'center'];

  const num = (e: Event) => Number((e.currentTarget as HTMLInputElement).value);
</script>

<SettingsCard id="appearance" title={t('settings.appearance')} reset="appearance">
  <Field label={t('settings.theme')}>
    <select aria-label={t('settings.theme')} class="field" value={s.theme} onchange={(e) => void settings.patch({ theme: e.currentTarget.value as Theme })}>
      {#each THEMES as v (v)}<option value={v}>{tDyn(`settings.theme.${v}`)}</option>{/each}
    </select>
  </Field>

  <Field label={t('settings.language')}>
    <select aria-label={t('settings.language')} class="field" value={s.language} onchange={(e) => void settings.patch({ language: e.currentTarget.value as Language })}>
      {#each LANGUAGES as v (v)}<option value={v}>{tDyn(`settings.language.${v}`)}</option>{/each}
    </select>
  </Field>

  <Field label={t('settings.scale')} hint={`${Math.round(s.scale * 100)}%`}>
    <input
      type="range"
      min="0.75"
      max="1.5"
      step="0.05"
      value={s.scale}
      oninput={(e) => void settings.patch({ scale: num(e) })}
      aria-label={t('settings.scale')}
    />
  </Field>

  <Field label={t('settings.opacity')} hint={`${Math.round(s.opacity * 100)}%`}>
    <input
      type="range"
      min="0.3"
      max="1"
      step="0.05"
      value={s.opacity}
      oninput={(e) => void settings.patch({ opacity: num(e) })}
      aria-label={t('settings.opacity')}
    />
  </Field>

  <Field label={t('settings.percentMode')}>
    <select aria-label={t('settings.percentMode')} class="field" value={s.percentMode} onchange={(e) => void settings.patch({ percentMode: e.currentTarget.value as PercentMode })}>
      {#each PERCENT_MODES as v (v)}<option value={v}>{tDyn(`settings.percentMode.${v}`)}</option>{/each}
    </select>
  </Field>

  <Field label={t('settings.percentPosition')} hint={t('settings.percentPosition.hint')}>
    <select
      aria-label={t('settings.percentPosition')}
      class="field"
      value={s.percentPosition}
      disabled={!s.sidebarItems.percentLabel}
      onchange={(e) => void settings.patch({ percentPosition: e.currentTarget.value as PercentPosition })}
    >
      {#each PERCENT_POSITIONS as v (v)}<option value={v}>{tDyn(`settings.percentPosition.${v}`)}</option>{/each}
    </select>
  </Field>

  <Field label={t('settings.ringMode')}>
    <select aria-label={t('settings.ringMode')} class="field" value={s.ringMode} onchange={(e) => void settings.patch({ ringMode: e.currentTarget.value as RingMode })}>
      {#each RING_MODES as v (v)}<option value={v}>{tDyn(`settings.ringMode.${v}`)}</option>{/each}
    </select>
  </Field>

  <SidebarStyleControls />

  <Field label={t('settings.surfaceStyle')}>
    <select aria-label={t('settings.surfaceStyle')} class="field" value={s.surfaceStyle} onchange={(e) => void settings.patch({ surfaceStyle: e.currentTarget.value as SurfaceStyle })}>
      {#each SURFACE_STYLES as v (v)}<option value={v}>{tDyn(`settings.surfaceStyle.${v}`)}</option>{/each}
    </select>
  </Field>

  <!-- the accent pair only paints the cyber HUD, so it only exists there -->
  {#if s.surfaceStyle === 'cyber'}
    <Field label={t('settings.cyberAccent')}>
      <select aria-label={t('settings.cyberAccent')} class="field" value={s.cyberAccent} onchange={(e) => void settings.patch({ cyberAccent: e.currentTarget.value as CyberAccent })}>
        {#each CYBER_ACCENTS as v (v)}<option value={v}>{tDyn(`settings.cyberAccent.${v}`)}</option>{/each}
      </select>
    </Field>
  {/if}
</SettingsCard>
