<!--
  The one filter bar shared by every History sub-view: range, provider and
  project. It stays at the top of the scrolling dashboard so a filter can be
  changed while looking at any chart. [FRONTEND]
-->
<script lang="ts">
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import type { HistoryPreset } from '$lib/history';
  import { HISTORY_VIEWS, type HistoryView } from '$lib/history-view-state';
  import type { ProviderId } from '$lib/types';
  import Segmented from './Segmented.svelte';

  interface Props {
    view: HistoryView;
    onview: (next: HistoryView) => void;
    preset: HistoryPreset;
    customFrom: string;
    customTo: string;
    provider: ProviderId | '';
    project: string | null;
    /** false when the custom dates do not form a range */
    rangeValid: boolean;
    providerOptions: ProviderId[];
    projectOptions: string[];
    providerName: (id: ProviderId) => string;
    projectLabel: (path: string) => string;
    onpreset: (next: HistoryPreset) => void;
  }

  let {
    view,
    onview,
    preset,
    customFrom = $bindable(),
    customTo = $bindable(),
    provider = $bindable(),
    project = $bindable(),
    rangeValid,
    providerOptions,
    projectOptions,
    providerName,
    projectLabel,
    onpreset,
  }: Props = $props();

  const PRESETS: Array<readonly [HistoryPreset, string]> = [
    ['today', 'history.range.today'],
    ['7d', 'history.range.7d'],
    ['30d', 'history.range.30d'],
    ['90d', 'history.range.90d'],
    ['custom', 'history.range.custom'],
  ];
  const presetOptions = $derived(PRESETS.map(([id, key]) => [id, tDyn(key)] as const));
  const viewOptions = $derived(HISTORY_VIEWS.map((id) => [id, t(`history.view.${id}` as 'history.view.usage')] as const));
  const filtered = $derived(provider !== '' || project !== null);
</script>

<div class="filters card" role="search" aria-label={t('history.filters')}>
  <div class="group">
    <Segmented options={viewOptions} value={view} label={t('history.views')} onchange={onview} />
  </div>

  <div class="group">
    <span class="ctl-label">{t('history.range')}</span>
    <Segmented options={presetOptions} value={preset} label={t('history.range')} onchange={onpreset} />
  </div>

  {#if preset === 'custom'}
    <div class="group">
      <label class="ctl-label" for="from">{t('history.from')}</label>
      <input id="from" class="field" type="date" bind:value={customFrom} max={customTo} aria-invalid={!rangeValid} aria-describedby={!rangeValid ? 'range-error' : undefined} />
      <label class="ctl-label" for="to">{t('history.to')}</label>
      <input id="to" class="field" type="date" bind:value={customTo} min={customFrom} aria-invalid={!rangeValid} aria-describedby={!rangeValid ? 'range-error' : undefined} />
    </div>
  {/if}

  <div class="group">
    <label class="ctl-label" for="provider">{t('history.provider')}</label>
    <select id="provider" class="field" bind:value={provider}>
      <option value="">{t('common.all')}</option>
      {#each providerOptions as id (id)}
        <option value={id}>{providerName(id)}</option>
      {/each}
    </select>

    <label class="ctl-label" for="project">{t('history.project')}</label>
    <select id="project" class="field project-select"
      value={project === null ? 'all' : `project:${project}`}
      title={project === null ? t('history.project.all') : project || t('history.project.unassigned')}
      onchange={(e) => (project = e.currentTarget.value === 'all' ? null : e.currentTarget.value.slice('project:'.length))}>
      <option value="all">{t('history.project.all')}</option>
      {#each projectOptions as path (path)}
        <option value={`project:${path}`} title={path || t('history.project.unassigned')}>{projectLabel(path)}</option>
      {/each}
    </select>

    {#if filtered}
      <button class="btn" onclick={() => { provider = ''; project = null; }}>{t('history.filters.clear')}</button>
    {/if}
  </div>
</div>

<style>
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    column-gap: 1.25rem;
    row-gap: 0.5rem;
    padding: 0.625rem 1rem;
    /* stays in view while the page scrolls; opaque so charts never show through */
    position: sticky;
    top: 0;
    z-index: 5;
    background: var(--surface);
  }

  .group {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  .ctl-label {
    font-size: 0.75rem;
    color: var(--muted);
    white-space: nowrap;
  }

  .project-select {
    min-width: 8rem;
    /* a deep path must never widen the controls row past the card */
    max-width: min(100%, 22rem);
    text-overflow: ellipsis;
  }
</style>
