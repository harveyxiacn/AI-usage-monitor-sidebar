<!--
  Settings → Presets: built-in and custom presets. Choosing one shows exactly
  what it would change; Apply writes the whole patch in one go. [FRONTEND]
-->
<script lang="ts">
  import ChangeList from './ChangeList.svelte';
  import SettingsBlock from './SettingsBlock.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import {
    BUILTIN_IDS,
    BUILTIN_PRESETS,
    MAX_CUSTOM_PRESETS,
    MAX_PRESET_NAME,
    addCustomPreset,
    capturePreset,
    presetChanges,
    removeCustomPreset,
  } from '$lib/presets';
  import type { SettingsPatch } from '$lib/settings-writer';
  import { settings } from '$lib/stores/settings.svelte';

  const s = $derived(settings.value);

  /** The preset being previewed. */
  let pending = $state<{ key: string; label: string; patch: SettingsPatch } | null>(null);
  let newName = $state('');
  let message = $state<string | null>(null);
  let problem = $state<string | null>(null);

  const customNames = $derived(Object.keys(s.customPresets ?? {}).sort());
  const changes = $derived(pending ? presetChanges(s, pending.patch) : []);

  function preview(key: string, label: string, patch: SettingsPatch) {
    message = null;
    problem = null;
    pending = pending?.key === key ? null : { key, label, patch };
  }

  async function apply() {
    if (!pending) return;
    const { patch } = pending;
    pending = null;
    await settings.patch(patch);
  }

  async function saveCurrent() {
    message = null;
    problem = null;
    const result = addCustomPreset(s.customPresets ?? {}, newName, capturePreset(s));
    if (!result.ok) {
      problem = result.reason === 'name'
        ? t('settings.presets.errName', { max: MAX_PRESET_NAME })
        : t('settings.presets.errFull', { max: MAX_CUSTOM_PRESETS });
      return;
    }
    const name = newName.trim();
    newName = '';
    await settings.patch({ customPresets: result.presets });
    message = t('settings.presets.saved', { name });
  }

  async function remove(name: string) {
    if (pending?.key === `custom:${name}`) pending = null;
    await settings.patch({ customPresets: removeCustomPreset(s.customPresets ?? {}, name) });
  }

  const searchText = $derived(
    [
      t('settings.presets.hint'),
      ...BUILTIN_IDS.flatMap((id) => [tDyn(`settings.preset.${id}`), tDyn(`settings.preset.${id}.hint`)]),
      ...customNames,
      t('settings.presets.saveCurrent'),
    ].join(' ')
  );
</script>

<SettingsCard id="presets" title={t('settings.card.presets')} wide>
  <SettingsBlock text={searchText}>
    <p class="note">{t('settings.presets.hint')}</p>

    <h4>{t('settings.presets.builtin')}</h4>
    <ul class="presets">
      {#each BUILTIN_IDS as id (id)}
        <li>
          <div class="name">
            <span>{tDyn(`settings.preset.${id}`)}</span>
            <span class="hint">{tDyn(`settings.preset.${id}.hint`)}</span>
          </div>
          <button
            class="btn"
            aria-pressed={pending?.key === id}
            onclick={() => preview(id, tDyn(`settings.preset.${id}`), BUILTIN_PRESETS[id])}
          >{t('settings.presets.choose')}</button>
        </li>
      {/each}
    </ul>

    <h4>{t('settings.presets.custom')}</h4>
    {#if customNames.length === 0}
      <p class="muted small">{t('settings.presets.none')}</p>
    {:else}
      <ul class="presets">
        {#each customNames as name (name)}
          <li>
            <div class="name"><span>{name}</span></div>
            <span class="row">
              <button
                class="btn"
                aria-pressed={pending?.key === `custom:${name}`}
                onclick={() => preview(`custom:${name}`, name, s.customPresets[name] as SettingsPatch)}
              >{t('settings.presets.choose')}</button>
              <button class="btn icon" aria-label={`${t('common.remove')}: ${name}`} onclick={() => void remove(name)}>×</button>
            </span>
          </li>
        {/each}
      </ul>
    {/if}

    {#if pending}
      <div class="preview" role="region" aria-label={t('settings.presets.preview')}>
        <h4>{pending.label}: {t('settings.presets.preview')}</h4>
        {#if changes.length === 0}
          <p class="muted small">{t('settings.presets.nothing')}</p>
        {:else}
          <p class="muted small">{t('settings.changes.count', { n: changes.length })}</p>
          <ChangeList {changes} />
        {/if}
        <div class="row end">
          <button class="btn" onclick={() => (pending = null)}>{t('common.cancel')}</button>
          <button class="btn btn-primary" disabled={changes.length === 0} onclick={() => void apply()}>{t('common.apply')}</button>
        </div>
      </div>
    {/if}

    <h4>{t('settings.presets.saveCurrent')}</h4>
    <p class="note">{t('settings.presets.saveHint')}</p>
    <form class="row" onsubmit={(e) => { e.preventDefault(); void saveCurrent(); }}>
      <input
        class="field name-input"
        bind:value={newName}
        maxlength={MAX_PRESET_NAME}
        placeholder={t('settings.presets.name')}
        aria-label={t('settings.presets.name')}
      />
      <button class="btn" type="submit" disabled={newName.trim() === ''}>{t('settings.presets.save')}</button>
    </form>
    {#if problem}<p class="err" role="alert">{problem}</p>{/if}
    {#if message}<p class="ok" role="status">{message}</p>{/if}
  </SettingsBlock>
</SettingsCard>

<style>
  h4 {
    margin: 0.75rem 0 0.25rem;
    font-size: 0.8125rem;
    font-weight: 600;
  }

  .presets {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .presets li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.4375rem 0;
    border-bottom: 1px solid var(--border);
  }

  .presets li:last-child {
    border-bottom: none;
  }

  .name {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    min-width: 0;
  }

  .hint {
    font-size: 0.75rem;
    color: var(--muted);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .row.end {
    justify-content: flex-end;
    margin-top: 0.5rem;
  }

  .btn[aria-pressed='true'] {
    border-color: var(--focus);
  }

  .preview {
    margin-top: 0.75rem;
    padding: 0.5rem 0.75rem 0.75rem;
    border: 1px solid var(--border-strong);
    border-radius: var(--r-control);
    background: var(--surface-2);
  }

  .preview h4 {
    margin-top: 0.25rem;
  }

  .name-input {
    flex: 1 1 12rem;
    min-width: 0;
  }
</style>
