<!--
  First-run wizard, three skippable steps: language and theme, screen edge,
  providers (with optional notifications / start at login). Shown only while
  `settings.onboarded` is false; an upgrading user's existing settings file
  counts as onboarded (backend, settings::load). Finishing or skipping writes
  `onboarded: true` and records the running version for "What's new". Every
  choice is applied immediately, so nothing is lost by skipping. [FRONTEND]
-->
<script lang="ts">
  import { onMount, tick } from 'svelte';
  import ProviderSetupList from './ProviderSetupList.svelte';
  import Field from './Field.svelte';
  import Toggle from './Toggle.svelte';
  import { getAppInfo, getProviderSetup } from '$lib/api';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { nextStep, previousStep, WIZARD_STEPS, type WizardStep } from '$lib/onboarding';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import type { Edge, Language, ProviderSetup, Theme } from '$lib/types';

  const LANGUAGES: Language[] = ['auto', 'en', 'zh-CN'];
  const THEMES: Theme[] = ['dark', 'light', 'auto'];
  const EDGES: Edge[] = ['left', 'right', 'top', 'bottom'];

  const s = $derived(settings.value);
  let step = $state<WizardStep>('basics');
  let setups = $state<ProviderSetup[]>([]);
  let checking = $state(false);
  let dialog = $state<HTMLElement | null>(null);

  const index = $derived(WIZARD_STEPS.indexOf(step));
  const next = $derived(nextStep(step));
  const previous = $derived(previousStep(step));

  async function loadSetups() {
    try {
      setups = await getProviderSetup();
    } catch {
      setups = [];
    }
  }

  async function checkAgain() {
    checking = true;
    try {
      await Promise.all([snapshot.refresh(), loadSetups()]);
    } finally {
      checking = false;
    }
  }

  onMount(() => {
    void loadSetups();
    void tick().then(() => dialog?.querySelector<HTMLElement>('select, button')?.focus());
  });

  async function done() {
    let version = '';
    try {
      version = (await getAppInfo()).version;
    } catch {
      /* the version only decides whether "What's new" shows later */
    }
    await settings.patch({ onboarded: true, ...(version ? { lastSeenVersion: version } : {}) });
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      void done();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="backdrop">
  <div class="dialog card" role="dialog" aria-modal="true" aria-labelledby="wiz-title" bind:this={dialog}>
    <header>
      <div>
        <h2 id="wiz-title">{t('wiz.title')}</h2>
        <p class="muted step">{t('wiz.step', { n: index + 1, total: WIZARD_STEPS.length })}</p>
      </div>
      <button class="btn link" onclick={() => void done()}>{t('wiz.skip')}</button>
    </header>

    <ol class="dots" aria-hidden="true">
      {#each WIZARD_STEPS as id, i (id)}<li class:on={i <= index}></li>{/each}
    </ol>

    <div class="body">
      {#if step === 'basics'}
        <h3>{t('wiz.basics.title')}</h3>
        <p class="muted hint">{t('wiz.basics.hint')}</p>
        <Field label={t('settings.language')}>
          <select class="field" aria-label={t('settings.language')} value={s.language} onchange={(e) => void settings.patch({ language: e.currentTarget.value as Language })}>
            {#each LANGUAGES as v (v)}<option value={v}>{tDyn(`settings.language.${v}`)}</option>{/each}
          </select>
        </Field>
        <Field label={t('settings.theme')}>
          <select class="field" aria-label={t('settings.theme')} value={s.theme} onchange={(e) => void settings.patch({ theme: e.currentTarget.value as Theme })}>
            {#each THEMES as v (v)}<option value={v}>{tDyn(`settings.theme.${v}`)}</option>{/each}
          </select>
        </Field>
      {:else if step === 'edge'}
        <h3>{t('wiz.edge.title')}</h3>
        <p class="muted hint">{t('wiz.edge.hint')}</p>
        <div class="edges" role="radiogroup" aria-label={t('settings.edge')}>
          {#each EDGES as edge (edge)}
            <button
              class="edge"
              class:picked={s.edge === edge}
              role="radio"
              aria-checked={s.edge === edge}
              onclick={() => void settings.patch({ edge })}
            >
              <span class="screen" aria-hidden="true"><span class={`bar ${edge}`}></span></span>
              <span>{tDyn(`settings.edge.${edge}`)}</span>
            </button>
          {/each}
        </div>
      {:else}
        <h3>{t('wiz.providers.title')}</h3>
        <p class="muted hint">{t('wiz.providers.hint')}</p>
        <ProviderSetupList {setups} />
        <div class="check">
          <button class="btn" disabled={checking || snapshot.refreshing !== null} onclick={() => void checkAgain()}>
            {checking ? t('gs.checking') : t('gs.check')}
          </button>
        </div>
        <h3 class="extras">{t('wiz.extras')}</h3>
        <Field label={t('wiz.notifications')}>
          <Toggle checked={s.notifications} label={t('wiz.notifications')} onchange={(v) => void settings.patch({ notifications: v })} />
        </Field>
        <Field label={t('wiz.autostart')}>
          <Toggle checked={s.autostart} label={t('wiz.autostart')} onchange={(v) => void settings.patch({ autostart: v })} />
        </Field>
        <p class="privacy muted">{t('gs.privacy')}</p>
      {/if}
    </div>

    <footer>
      <button class="btn" disabled={!previous} onclick={() => previous && (step = previous)}>{t('wiz.back')}</button>
      {#if next}
        <button class="btn btn-primary" onclick={() => (step = next)}>{t('wiz.next')}</button>
      {:else}
        <button class="btn btn-primary" onclick={() => void done()}>{t('wiz.finish')}</button>
      {/if}
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: 1rem;
    background: rgb(0 0 0 / 0.55);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    width: min(100%, 34rem);
    max-height: 100%;
    padding: 1.25rem;
    overflow: hidden;
  }

  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1rem;
  }

  h2 {
    margin: 0;
    font-size: 1.125rem;
    font-weight: 600;
  }

  h3 {
    margin: 0;
    font-size: 0.9375rem;
    font-weight: 600;
  }

  .step,
  .hint {
    margin: 0.125rem 0 0;
    font-size: 0.75rem;
  }

  .link {
    border: none;
    background: none;
    color: var(--focus);
    text-decoration: underline;
  }

  .dots {
    display: flex;
    gap: 0.375rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .dots li {
    flex: 1;
    height: 0.1875rem;
    border-radius: 999px;
    background: var(--surface-3);
  }

  .dots li.on {
    background: var(--focus);
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-height: 12rem;
    overflow-y: auto;
  }

  .extras {
    margin-top: 0.75rem;
  }

  .check {
    padding-top: 0.5rem;
  }

  .privacy {
    margin: 0.5rem 0 0;
    font-size: 0.75rem;
  }

  footer {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
  }

  /* ---- edge picker: a miniature screen with the bar drawn on one side ---- */
  .edges {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(6rem, 1fr));
    gap: 0.625rem;
    margin-top: 0.5rem;
  }

  .edge {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.375rem;
    padding: 0.625rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--r-control);
    background: var(--surface-2);
    color: var(--text);
    font-size: 0.8125rem;
  }

  .edge:hover {
    background: var(--surface-3);
  }

  .edge.picked {
    border-color: var(--focus);
    box-shadow: 0 0 0 1px var(--focus);
  }

  .edge:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }

  .screen {
    position: relative;
    display: block;
    width: 4.5rem;
    height: 3rem;
    border: 2px solid var(--border-strong);
    border-radius: 0.25rem;
    background: var(--bg);
  }

  .bar {
    position: absolute;
    border-radius: 999px;
    background: var(--focus);
  }

  .bar.left,
  .bar.right {
    top: 22%;
    bottom: 22%;
    width: 0.25rem;
  }

  .bar.left {
    left: 0.1875rem;
  }

  .bar.right {
    right: 0.1875rem;
  }

  .bar.top,
  .bar.bottom {
    left: 22%;
    right: 22%;
    height: 0.25rem;
  }

  .bar.top {
    top: 0.1875rem;
  }

  .bar.bottom {
    bottom: 0.1875rem;
  }
</style>
