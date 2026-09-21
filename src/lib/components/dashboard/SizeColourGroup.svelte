<!--
  Dashboard → Settings → "Size & colour". [FRONTEND]

  Sliders for Settings.sizes and <input type="color"> pickers (with the hex
  spelled out, and editable as text) for Settings.colors, next to a live Ring
  preview that is pinned to the values currently in the form — so a size can be
  judged before the sidebar itself is re-measured.

  Every control applies immediately through settings.patch(); `update_settings`
  merges `colors` and `sizes` per key, so a patch only ever carries the one key
  that changed.
-->
<script lang="ts">
  import Field from './Field.svelte';
  import ProviderLogo from '$lib/components/ProviderLogo.svelte';
  import Ring from '$lib/components/Ring.svelte';
  import { parseHex } from '$lib/colors';
  import { hasKey, t, tDyn } from '$lib/i18n/i18n.svelte';
  import { providerColorKeys, providerDisplayName } from '$lib/providers';
  import {
    clampSize,
    defaultColors,
    defaultSizes,
    settings,
    SIZE_LIMITS,
  } from '$lib/stores/settings.svelte';
  import type { ColorSettings, SizeSettings } from '$lib/types';

  const s = $derived(settings.value);

  const SIZE_KEYS = Object.keys(defaultSizes) as (keyof SizeSettings)[];
  /**
   * Solid colours: always a real hex. `surface`/`text` may also be "" = theme.
   * The provider accents are whatever `Settings.colors` carries beyond the four
   * fixed keys, so a provider added in the backend gets a picker for free.
   */
  const SOLID_KEYS = $derived([
    ...providerColorKeys(s.colors),
    'warn',
    'critical',
  ] as (keyof ColorSettings)[]);
  const OPTIONAL_KEYS = ['surface', 'text'] as const;

  /** Accent label: the i18n key when there is one, else the provider's name. */
  const colorLabel = (key: keyof ColorSettings) =>
    hasKey(`settings.colors.${key}`)
      ? tDyn(`settings.colors.${key}`)
      : t('settings.colors.provider', { provider: providerDisplayName(key) });

  function setSize(key: keyof SizeSettings, raw: number) {
    const value = clampSize(key, raw);
    if (value === s.sizes[key]) return;
    void settings.patch({ sizes: { [key]: value } });
  }

  function setColor(key: keyof ColorSettings, value: string) {
    void settings.patch({ colors: { [key]: value } });
  }

  /** Text input: only commit once it parses, so half-typed hex doesn't flash. */
  function setColorText(key: keyof ColorSettings, value: string, allowEmpty: boolean) {
    const v = value.trim();
    if (allowEmpty && v === '') return setColor(key, '');
    if (parseHex(v)) setColor(key, v.startsWith('#') ? v.toLowerCase() : `#${v.toLowerCase()}`);
  }

  /** What the <input type="color"> should show for an "use theme default" key. */
  const swatchFallback = (key: (typeof OPTIONAL_KEYS)[number]) =>
    key === 'surface' ? '#0c0c0e' : '#f5f5f7';

  function resetAll() {
    void settings.patch({
      colors: structuredClone(defaultColors),
      sizes: structuredClone(defaultSizes),
    });
  }

  // the preview is pinned to the live form values so it updates even before the
  // sidebar window has re-measured itself
  const previewArcs = $derived([
    { percent: 31, accent: 'var(--accent-claude-1)' },
    { percent: 73, accent: 'var(--accent-claude-2)' },
    { percent: 24, accent: 'var(--accent-claude-3)' },
  ]);
</script>

<article class="card group wide">
  <header class="head">
    <h3>{t('settings.sizeColour')}</h3>
    <button class="btn" onclick={resetAll}>{t('settings.resetDefaults')}</button>
  </header>

  <div class="split">
    <div class="controls">
      <h4>{t('settings.sizes')}</h4>
      {#each SIZE_KEYS as key (key)}
        {@const [min, max, step] = SIZE_LIMITS[key]}
        <Field label={tDyn(`settings.sizes.${key}`)}>
          <input
            type="range"
            {min}
            {max}
            {step}
            value={s.sizes[key]}
            oninput={(e) => setSize(key, Number(e.currentTarget.value))}
            aria-label={tDyn(`settings.sizes.${key}`)}
          />
          <input
            class="field n"
            type="number"
            {min}
            {max}
            {step}
            value={s.sizes[key]}
            onchange={(e) => setSize(key, Number(e.currentTarget.value))}
            aria-label={tDyn(`settings.sizes.${key}`)}
          />
        </Field>
      {/each}

      <h4>{t('settings.colours')}</h4>
      {#each SOLID_KEYS as key (key)}
        <Field label={colorLabel(key)}>
          <input
            class="swatch"
            type="color"
            value={s.colors[key] || '#000000'}
            oninput={(e) => setColor(key, e.currentTarget.value)}
            aria-label={colorLabel(key)}
          />
          <input
            class="field hex mono"
            value={s.colors[key]}
            onchange={(e) => setColorText(key, e.currentTarget.value, false)}
            aria-label={`${colorLabel(key)} (hex)`}
          />
        </Field>
      {/each}

      {#each OPTIONAL_KEYS as key (key)}
        {@const usesDefault = s.colors[key] === ''}
        <Field label={tDyn(`settings.colors.${key}`)}>
          <label class="chk">
            <input
              type="checkbox"
              checked={usesDefault}
              onchange={(e) =>
                setColor(key, e.currentTarget.checked ? '' : swatchFallback(key))}
            />
            {t('settings.useThemeDefault')}
          </label>
          <input
            class="swatch"
            type="color"
            disabled={usesDefault}
            value={s.colors[key] || swatchFallback(key)}
            oninput={(e) => setColor(key, e.currentTarget.value)}
            aria-label={tDyn(`settings.colors.${key}`)}
          />
          <input
            class="field hex mono"
            disabled={usesDefault}
            value={s.colors[key]}
            onchange={(e) => setColorText(key, e.currentTarget.value, true)}
            aria-label={`${tDyn(`settings.colors.${key}`)} (hex)`}
          />
        </Field>
      {/each}

      <p class="note">{t('settings.sizeHint')}</p>
    </div>

    <aside class="preview">
      <span class="ptitle">{t('settings.preview')}</span>
      <!-- the backdrop is a separate element: `.surface` owns the (possibly
           translucent) fill, so anything opaque must sit *behind* it -->
      <div class="backdrop">
        <div class="stage surface">
          <Ring
            arcs={previewArcs}
            labelPercent={73}
            thresholds={s.thresholds}
            size={s.sizes.ringSize}
            stroke={s.sizes.ringStroke}
            showPercentLabel={s.showPercentLabel}
            percentMode={s.percentMode}
          >
            {#snippet logo(logoSize)}
              <ProviderLogo provider="claude" size={logoSize} />
            {/snippet}
          </Ring>
        </div>
      </div>
    </aside>
  </div>
</article>

<style>
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }

  h3 {
    margin: 0;
    font-size: 0.875rem;
    font-weight: 600;
  }

  h4 {
    margin: 0.75rem 0 0;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .split {
    display: flex;
    align-items: flex-start;
    gap: 1.5rem;
  }

  .controls {
    flex: 1 1 auto;
    min-width: 0;
  }

  input[type='range'] {
    width: 8rem;
    accent-color: var(--focus);
  }

  .n {
    width: 4.5rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .hex {
    width: 6.5rem;
    text-transform: lowercase;
  }

  /* a native colour input styled down to a plain rounded swatch */
  .swatch {
    width: 2rem;
    height: 1.75rem;
    padding: 0.125rem;
    border: 1px solid var(--border);
    border-radius: var(--r-control);
    background: var(--surface-2);
    cursor: pointer;
  }

  .swatch:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .swatch::-webkit-color-swatch-wrapper {
    padding: 0;
  }

  .swatch::-webkit-color-swatch {
    border: none;
    border-radius: calc(var(--r-control) - 0.125rem);
  }

  .chk {
    display: inline-flex;
    align-items: center;
    gap: 0.3125rem;
    font-size: 0.75rem;
    color: var(--muted);
    white-space: nowrap;
    cursor: pointer;
  }

  .note {
    margin: 0.75rem 0 0;
    font-size: 0.6875rem;
    color: var(--faint);
  }

  .preview {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
    position: sticky;
    top: 0;
  }

  .ptitle {
    font-size: 0.6875rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  /* the dashboard is opaque, so give the (translucent) glass something to sit
     on — a dark plate that stands in for the desktop wallpaper */
  .backdrop {
    display: grid;
    place-items: center;
    padding: 0.75rem;
    border-radius: var(--r-card);
    background: linear-gradient(135deg, #2a2f3a 0%, #14161c 55%, #241c2a 100%);
  }

  /* mirrors the sidebar pill: same padding/radius variables, same material */
  .stage {
    display: grid;
    place-items: center;
    padding: calc(var(--bar-padding) + 0.25rem) var(--bar-padding);
    border-radius: var(--r-pill);
  }

  @media (max-width: 760px) {
    .split {
      flex-direction: column;
    }

    .preview {
      position: static;
      align-self: flex-start;
    }
  }
</style>
