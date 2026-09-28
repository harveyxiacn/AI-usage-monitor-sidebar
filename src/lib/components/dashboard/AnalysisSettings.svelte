<script lang="ts">
  import { untrack } from 'svelte';
  import { saveAnalysisSettings } from '$lib/api';
  import type { AnalysisSettings } from '$lib/session-types';
  import { st } from '$lib/session-labels.svelte';
  let { value, onsave, onclose }: { value: AnalysisSettings; onsave: (settings: AnalysisSettings) => void; onclose: () => void } = $props();
  let draft = $state<AnalysisSettings>(untrack(() => structuredClone($state.snapshot(value))));
  let saving = $state(false);
  let error = $state('');
  async function save(event: SubmitEvent) {
    event.preventDefault(); if (saving) return;
    saving = true; error = '';
    try { onsave(await saveAnalysisSettings({ ...draft, inputUsdPerMillion: draft.inputUsdPerMillion ?? null, outputUsdPerMillion: draft.outputUsdPerMillion ?? null })); }
    catch (e) { error = String(e); } finally { saving = false; }
  }
</script>

<form class="card config" onsubmit={save} aria-label={st('settings')}>
  <header><div><h3>{st('settings')}</h3><p>{st('privacy')}</p></div><button class="btn" type="button" onclick={onclose}>{st('cancel')}</button></header>
  <label class="check"><input type="checkbox" bind:checked={draft.contentEnabled} />{st('contentEnabled')}</label>
  <div class="fields">
    <label class="wide">{st('endpoint')}<input class="field" type="url" required bind:value={draft.endpoint} spellcheck="false" /></label>
    <label>{st('model')}<input class="field" bind:value={draft.model} placeholder="model-id" spellcheck="false" /></label>
    <label>{st('keyEnv')}<input class="field" required pattern="[A-Za-z_][A-Za-z0-9_]*" bind:value={draft.apiKeyEnv} placeholder="OPENAI_API_KEY" spellcheck="false" autocomplete="off" /></label>
    <p class="wide">{st('keyNote')}</p>
    <label>{st('inputLimit')}<input class="field" type="number" required min="1000" max="200000" bind:value={draft.maxInputChars} /></label>
    <label>{st('maxOutput')}<input class="field" type="number" required min="256" max="16000" bind:value={draft.maxOutputTokens} /></label>
    <label>{st('inputRate')}<input class="field" type="number" min="0" max="100000" step="any" bind:value={draft.inputUsdPerMillion} /></label>
    <label>{st('outputRate')}<input class="field" type="number" min="0" max="100000" step="any" bind:value={draft.outputUsdPerMillion} /></label>
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <button class="btn primary" type="submit" disabled={saving}>{saving ? st('loading') : st('save')}</button>
</form>

<style>
  .config { padding: 1.25rem; display: grid; gap: 1rem; border-left: 3px solid var(--focus); }
  header { display: flex; align-items: flex-start; justify-content: space-between; gap: 1rem; }
  h3 { margin: 0 0 .4rem; font-size: 1rem; } p { margin: 0; color: var(--muted); font-size: .8rem; line-height: 1.6; }
  .fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 1rem; }
  label { display: grid; gap: .4rem; color: var(--muted); font-size: .8rem; }
  .check { display: flex; align-items: center; gap: .6rem; color: var(--text); }
  input { min-width: 0; width: 100%; } input[type="checkbox"] { width: auto; accent-color: var(--focus); }
  .wide { grid-column: 1 / -1; } .primary { justify-self: start; color: var(--focus); } .error { color: var(--danger, #e66); }
  @media(max-width:650px) { .fields { grid-template-columns: 1fr; } }
</style>
