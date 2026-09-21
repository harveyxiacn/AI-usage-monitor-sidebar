<!--
  Dashboard → Settings. Every control applies immediately through
  `settings.patch()` (which persists via `update_settings` and, for the
  window-related keys, calls `apply_window_settings`). [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Field from './Field.svelte';
  import SizeColourGroup from './SizeColourGroup.svelte';
  import Toggle from './Toggle.svelte';
  import ProviderLogo from '$lib/components/ProviderLogo.svelte';
  import {
    getAppInfo,
    getMonitors,
    getPricing,
    isTauri,
    quitApp,
    reingestLogs,
    setPricing,
  } from '$lib/api';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import type {
    AppInfo,
    Edge,
    Language,
    MonitorInfo,
    PercentMode,
    PricingEntry,
    PricingTable,
    ProviderId,
    RingMode,
    SurfaceStyle,
    Theme,
    VerticalAlign,
  } from '$lib/types';

  const GITHUB_URL = 'https://github.com/harveyxiacn/AI-usage-monitor-sidebar';

  const s = $derived(settings.value);

  let monitors = $state<MonitorInfo[]>([]);
  let appInfo = $state<AppInfo | null>(null);
  let pricing = $state<PricingTable | null>(null);
  let pricingSaved = $state(false);
  let pricingLoading = $state(true);
  let pricingSaving = $state(false);
  let pricingError = $state<string | null>(null);
  let pricingDirty = $state(false);
  let actionError = $state<string | null>(null);
  let rescanResult = $state<string | null>(null);
  let rescanning = $state(false);
  let savedTimer: ReturnType<typeof setTimeout> | undefined;

  /** Provider rows: known providers from the snapshot, ordered by settings. */
  const providerRows = $derived.by(() => {
    const ids = new Set<ProviderId>((snapshot.value?.providers ?? []).map((p) => p.provider));
    for (const id of Object.keys(s.providers)) ids.add(id as ProviderId);
    return [...ids].sort((a, b) => (s.providers[a]?.order ?? 0) - (s.providers[b]?.order ?? 0));
  });

  onMount(() => {
    void getMonitors().then((m) => (monitors = m)).catch((e) => (actionError = String(e)));
    void getAppInfo().then((i) => (appInfo = i)).catch((e) => (actionError = String(e)));
    void loadPricing();
    return () => clearTimeout(savedTimer);
  });

  const providerName = (id: ProviderId) =>
    snapshot.value?.providers.find((p) => p.provider === id)?.displayName ??
    (id === 'claude' ? 'Claude' : 'Codex');

  /** Swap the `order` of two adjacent providers. */
  async function move(id: ProviderId, delta: -1 | 1) {
    const list = providerRows;
    const i = list.indexOf(id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= list.length) return;
    const order = [...list];
    [order[i], order[j]] = [order[j], order[i]];
    await settings.patch({ providers: Object.fromEntries(order.map((provider, index) => [provider, { order: index }])) });
  }

  function num(e: Event): number {
    return Number((e.currentTarget as HTMLInputElement).value);
  }

  async function loadPricing() {
    pricingLoading = true;
    pricingError = null;
    try { pricing = await getPricing(); }
    catch (e) { pricingError = String(e); }
    finally { pricingLoading = false; }
  }

  async function savePricing() {
    if (!pricing || pricingSaving) return;
    pricingError = null;
    const patterns = new Set<string>();
    for (const entry of pricing.entries) {
      const pattern = entry.modelPattern.trim().toLowerCase();
      const prices = [entry.inputPerM, entry.outputPerM, entry.cacheWritePerM, entry.cacheReadPerM];
      if (!pattern || patterns.has(pattern) || prices.some((value) => !Number.isFinite(value) || value < 0)) {
        pricingError = t('settings.pricing.invalid');
        return;
      }
      patterns.add(pattern);
    }
    pricingSaving = true;
    try {
      pricing = await setPricing({ ...pricing, entries: pricing.entries.map((entry) => ({ ...entry, modelPattern: entry.modelPattern.trim() })) });
      pricingSaved = true;
      pricingDirty = false;
      clearTimeout(savedTimer);
      savedTimer = setTimeout(() => (pricingSaved = false), 1500);
    } catch (e) { pricingError = String(e); }
    finally { pricingSaving = false; }
  }

  function addPricingRow() {
    if (!pricing) return;
    const entry: PricingEntry = {
      modelPattern: '',
      inputPerM: 0,
      outputPerM: 0,
      cacheWritePerM: 0,
      cacheReadPerM: 0,
    };
    pricing = { ...pricing, entries: [...pricing.entries, entry] };
    pricingDirty = true;
    pricingSaved = false;
  }

  function removePricingRow(index: number) {
    if (!pricing) return;
    pricing = { ...pricing, entries: pricing.entries.filter((_, i) => i !== index) };
    pricingDirty = true;
    pricingSaved = false;
  }

  function editPricing(index: number, key: keyof PricingEntry, value: string) {
    if (!pricing) return;
    const entries = pricing.entries.map((e, i) =>
      i === index
        ? { ...e, [key]: key === 'modelPattern' ? value : (value === '' ? NaN : Number(value)) }
        : e
    );
    pricing = { ...pricing, entries };
    pricingDirty = true;
    pricingSaved = false;
  }

  async function rescan() {
    rescanning = true;
    actionError = null;
    rescanResult = null;
    try {
      const stats = await reingestLogs();
      rescanResult = t('history.ingestStats', { files: stats.filesScanned, updated: stats.filesUpdated, events: stats.eventsAdded, ms: stats.durationMs });
      if (stats.errors.length > 0) actionError = stats.errors.join('\n');
    } catch (e) {
      actionError = String(e);
    } finally {
      rescanning = false;
    }
  }

  async function openGithub() {
    if (isTauri()) {
      const { openUrl } = await import('@tauri-apps/plugin-opener');
      await openUrl(GITHUB_URL);
    } else {
      window.open(GITHUB_URL, '_blank', 'noopener');
    }
  }

  async function runAction(action: () => Promise<unknown>) {
    actionError = null;
    try { await action(); } catch (e) { actionError = String(e); }
  }

  const THEMES: Theme[] = ['auto', 'dark', 'light'];
  const LANGUAGES: Language[] = ['auto', 'en', 'zh-CN'];
  const EDGES: Edge[] = ['left', 'right'];
  const ALIGNS: VerticalAlign[] = ['top', 'center', 'bottom'];
  const RING_MODES: RingMode[] = ['concentric', 'primary', 'all'];
  const SURFACE_STYLES: SurfaceStyle[] = ['glass', 'solid', 'cyber'];
  const PERCENT_MODES: PercentMode[] = ['used', 'remaining'];
</script>

<section class="settings">
  {#if settings.error}
    <p class="err" role="alert">{t('common.error', { message: settings.error })}</p>
  {/if}
  {#if actionError}
    <p class="err" role="alert">{t('common.error', { message: actionError })}</p>
  {/if}
  {#if settings.saving}
    <p class="save-state muted" role="status">{t('common.saving')}</p>
  {/if}

  <article class="card group">
    <h3>{t('settings.appearance')}</h3>

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

    <Field label={t('settings.showPercentLabel')}>
      <Toggle
        checked={s.showPercentLabel}
        label={t('settings.showPercentLabel')}
        onchange={(v) => void settings.patch({ showPercentLabel: v })}
      />
    </Field>

    <Field label={t('settings.percentMode')}>
      <select aria-label={t('settings.percentMode')} class="field" value={s.percentMode} onchange={(e) => void settings.patch({ percentMode: e.currentTarget.value as PercentMode })}>
        {#each PERCENT_MODES as v (v)}<option value={v}>{tDyn(`settings.percentMode.${v}`)}</option>{/each}
      </select>
    </Field>

    <Field label={t('settings.ringMode')}>
      <select aria-label={t('settings.ringMode')} class="field" value={s.ringMode} onchange={(e) => void settings.patch({ ringMode: e.currentTarget.value as RingMode })}>
        {#each RING_MODES as v (v)}<option value={v}>{tDyn(`settings.ringMode.${v}`)}</option>{/each}
      </select>
    </Field>

    <Field label={t('settings.showScopedRing')}>
      <Toggle
        checked={s.showScopedRing}
        label={t('settings.showScopedRing')}
        disabled={s.ringMode !== 'concentric'}
        onchange={(v) => void settings.patch({ showScopedRing: v })}
      />
    </Field>

    <Field label={t('settings.surfaceStyle')}>
      <select aria-label={t('settings.surfaceStyle')} class="field" value={s.surfaceStyle} onchange={(e) => void settings.patch({ surfaceStyle: e.currentTarget.value as SurfaceStyle })}>
        {#each SURFACE_STYLES as v (v)}<option value={v}>{tDyn(`settings.surfaceStyle.${v}`)}</option>{/each}
      </select>
    </Field>
  </article>

  <SizeColourGroup />

  <article class="card group">
    <h3>{t('settings.position')}</h3>

    <Field label={t('settings.edge')}>
      <select aria-label={t('settings.edge')} class="field" value={s.edge} onchange={(e) => void settings.patch({ edge: e.currentTarget.value as Edge })}>
        {#each EDGES as v (v)}<option value={v}>{tDyn(`settings.edge.${v}`)}</option>{/each}
      </select>
    </Field>

    <Field label={t('settings.verticalAlign')}>
      <select aria-label={t('settings.verticalAlign')} class="field" value={s.verticalAlign} onchange={(e) => void settings.patch({ verticalAlign: e.currentTarget.value as VerticalAlign })}>
        {#each ALIGNS as v (v)}<option value={v}>{tDyn(`settings.verticalAlign.${v}`)}</option>{/each}
      </select>
    </Field>

    <Field label={t('settings.verticalOffset')}>
      <input
        class="field num"
        type="number"
        step="1"
        value={s.verticalOffset}
        onchange={(e) => void settings.patch({ verticalOffset: Math.round(num(e)) })}
        aria-label={t('settings.verticalOffset')}
      />
    </Field>

    <Field label={t('settings.monitor')}>
      <select aria-label={t('settings.monitor')}
        class="field"
        value={s.monitor ?? ''}
        onchange={(e) => void settings.patch({ monitor: e.currentTarget.value || null })}
      >
        <option value="">{t('settings.monitor.primary')}</option>
        {#each monitors as m (m.name)}
          <option value={m.name}>{m.name} — {m.width}×{m.height}{m.isPrimary ? ' ★' : ''}</option>
        {/each}
      </select>
    </Field>

    <Field label={t('settings.alwaysOnTop')}>
      <Toggle
        checked={s.alwaysOnTop}
        label={t('settings.alwaysOnTop')}
        onchange={(v) => void settings.patch({ alwaysOnTop: v })}
      />
    </Field>
  </article>

  <article class="card group">
    <h3>{t('settings.behaviour')}</h3>

    <Field label={t('settings.autoHide')}>
      <Toggle
        checked={s.autoHide}
        label={t('settings.autoHide')}
        onchange={(v) => void settings.patch({ autoHide: v })}
      />
    </Field>

    <Field label={t('settings.autoHideDelayMs')}>
      <input
        class="field num"
        type="number"
        min="0"
        max="10000"
        step="100"
        value={s.autoHideDelayMs}
        disabled={!s.autoHide}
        onchange={(e) => void settings.patch({ autoHideDelayMs: Math.round(num(e)) })}
        aria-label={t('settings.autoHideDelayMs')}
      />
    </Field>

    <Field label={t('settings.collapsedWidth')}>
      <input
        class="field num"
        type="number"
        min="2"
        max="40"
        step="1"
        value={s.collapsedWidth}
        disabled={!s.autoHide}
        onchange={(e) => void settings.patch({ collapsedWidth: Math.round(num(e)) })}
        aria-label={t('settings.collapsedWidth')}
      />
    </Field>

    <Field label={t('settings.refreshIntervalSec')}>
      <input
        class="field num"
        type="number"
        min="15"
        max="3600"
        step="5"
        value={s.refreshIntervalSec}
        onchange={(e) => void settings.patch({ refreshIntervalSec: Math.round(num(e)) })}
        aria-label={t('settings.refreshIntervalSec')}
      />
    </Field>

    <Field label={t('settings.notifications')}>
      <Toggle
        checked={s.notifications}
        label={t('settings.notifications')}
        onchange={(v) => void settings.patch({ notifications: v })}
      />
    </Field>

    <Field label={t('settings.warnThreshold')}>
      <input class="field num" type="number" min="1" max={s.thresholds.critical - 1} step="1"
        value={s.thresholds.warn} aria-label={t('settings.warnThreshold')}
        onchange={(e) => void settings.patch({ thresholds: { warn: Math.max(1, Math.min(s.thresholds.critical - 1, Math.round(num(e)))) } })} />
    </Field>

    <Field label={t('settings.criticalThreshold')}>
      <input class="field num" type="number" min={s.thresholds.warn + 1} max="100" step="1"
        value={s.thresholds.critical} aria-label={t('settings.criticalThreshold')}
        onchange={(e) => void settings.patch({ thresholds: { critical: Math.min(100, Math.max(s.thresholds.warn + 1, Math.round(num(e)))) } })} />
    </Field>

    <Field label={t('settings.autostart')}>
      <Toggle
        checked={s.autostart}
        label={t('settings.autostart')}
        onchange={(v) => void settings.patch({ autostart: v })}
      />
    </Field>
  </article>

  <article class="card group">
    <h3>{t('settings.providers')}</h3>
    {#each providerRows as id, i (id)}
      <div class="prow">
        <span class="plogo"><ProviderLogo provider={id} size={20} /></span>
        <span class="pname">{providerName(id)}</span>
        <button class="btn icon" disabled={i === 0} onclick={() => void move(id, -1)} aria-label={t('common.up')}>↑</button>
        <button class="btn icon" disabled={i === providerRows.length - 1} onclick={() => void move(id, 1)} aria-label={t('common.down')}>↓</button>
        <Toggle
          checked={s.providers[id]?.enabled ?? true}
          label={`${providerName(id)} — ${t('settings.providerEnabled')}`}
          onchange={(v) => void settings.patchProvider(id, { enabled: v })}
        />
      </div>
    {/each}
  </article>

  <article class="card group wide">
    <h3>{t('settings.data')}</h3>

    <Field label={t('settings.ingestEnabled')}>
      <Toggle
        checked={s.ingestEnabled}
        label={t('settings.ingestEnabled')}
        onchange={(v) => void settings.patch({ ingestEnabled: v })}
      />
    </Field>

    <Field label={t('settings.monthlyBudgetUsd')} hint={t('settings.monthlyBudgetHint')}>
      <input
        class="field num"
        type="number"
        min="0"
        max="1000000"
        step="1"
        value={s.monthlyBudgetUsd}
        onchange={(e) => void settings.patch({ monthlyBudgetUsd: Math.min(1_000_000, Math.max(0, e.currentTarget.valueAsNumber || 0)) })}
      />
    </Field>

    <Field label={t('history.rescan')}>
      <button class="btn" disabled={rescanning} onclick={() => void rescan()}>
        {rescanning ? t('history.ingestRunning') : t('history.rescan')}
      </button>
    </Field>

    {#if rescanResult}<p class="muted small" role="status">{rescanResult}</p>{/if}

    <div class="pricing">
      <header class="pricing-head">
        <span class="label">{t('settings.pricing')}</span>
        {#if pricing?.updatedAt}
          <span class="muted small">
            {t('settings.pricing.updated', { when: new Date(pricing.updatedAt).toLocaleDateString() })}
          </span>
        {/if}
      </header>

      {#if pricingError}
        <p class="err" role="alert">{t('common.error', { message: pricingError })}</p>
        {#if !pricing}<button class="btn" onclick={() => void loadPricing()} disabled={pricingLoading}>{t('common.retry')}</button>{/if}
      {/if}
      {#if pricingLoading}
        <p class="muted small">{t('common.loading')}</p>
      {:else if pricing?.entries.length === 0}
        <p class="muted small">{t('settings.pricing.none')}</p>
      {:else if pricing}
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th>{t('settings.pricing.model')}</th>
                <th class="n">{t('settings.pricing.input')}</th>
                <th class="n">{t('settings.pricing.output')}</th>
                <th class="n">{t('settings.pricing.cacheWrite')}</th>
                <th class="n">{t('settings.pricing.cacheRead')}</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {#each pricing.entries as e, i (i)}
                <tr>
                  <td>
                    <input
                      class="field pattern"
                      value={e.modelPattern}
                      disabled={pricingSaving}
                      oninput={(ev) => editPricing(i, 'modelPattern', ev.currentTarget.value)}
                      aria-label={t('settings.pricing.model')}
                    />
                  </td>
                  {#each [['inputPerM', 'settings.pricing.input'], ['outputPerM', 'settings.pricing.output'], ['cacheWritePerM', 'settings.pricing.cacheWrite'], ['cacheReadPerM', 'settings.pricing.cacheRead']] as [key, label] (key)}
                    <td class="n">
                      <input
                        class="field price"
                        type="number"
                        min="0"
                        step="0.01"
                        value={e[key as keyof PricingEntry]}
                        disabled={pricingSaving}
                        oninput={(ev) => editPricing(i, key as keyof PricingEntry, ev.currentTarget.value)}
                        aria-label={tDyn(label)}
                      />
                    </td>
                  {/each}
                  <td class="n">
                    <button class="btn icon" onclick={() => removePricingRow(i)} disabled={pricingSaving} aria-label={t('common.remove')}>×</button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}

      <div class="pricing-actions">
        {#if pricingDirty}<span class="muted small">{t('settings.pricing.unsaved')}</span>{/if}
        <button class="btn" onclick={addPricingRow} disabled={!pricing || pricingSaving}>{t('settings.pricing.add')}</button>
        <button class="btn btn-primary" onclick={() => void savePricing()} disabled={!pricing || pricingSaving}>
          {pricingSaving ? t('common.saving') : pricingSaved ? t('common.saved') : t('settings.pricing.save')}
        </button>
      </div>
    </div>
  </article>

  <article class="card group">
    <h3>{t('settings.about')}</h3>
    <Field label={t('settings.about.version')}>
      <span class="mono">{appInfo?.version ?? '—'}</span>
    </Field>
    <Field label={t('settings.about.platform')}>
      <span class="mono">{appInfo?.platform ?? '—'}</span>
    </Field>
    <Field label={t('settings.about.backend')}>
      <span class="mono">{appInfo?.backend ?? '—'}</span>
    </Field>
    <Field label={t('settings.about.dataDir')}>
      <span class="mono path" title={appInfo?.dataDir ?? ''}>{appInfo?.dataDir ?? '—'}</span>
    </Field>
    <Field label={t('settings.about.configDir')}>
      <span class="mono path" title={appInfo?.configDir ?? ''}>{appInfo?.configDir ?? '—'}</span>
    </Field>
    <div class="actions">
      <button class="btn" onclick={() => void runAction(openGithub)}>{t('settings.about.github')}</button>
      <button class="btn danger" onclick={() => void runAction(quitApp)}>{t('settings.about.quit')}</button>
    </div>
  </article>
</section>

<style>
  .settings {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 24rem), 1fr));
    align-items: start;
    gap: 1rem;
  }

  .group {
    padding: 0.75rem 1rem 1rem;
    min-width: 0;
  }

  /* the pricing editor needs the full grid width on a two-column layout */
  .group.wide {
    grid-column: 1 / -1;
  }

  .actions {
    display: flex;
    gap: 0.5rem;
    padding-top: 0.625rem;
    margin-top: 0.25rem;
    border-top: 1px solid var(--border);
  }

  h3 {
    margin: 0 0 0.25rem;
    font-size: 0.875rem;
    font-weight: 600;
  }

  input[type='range'] {
    width: 9rem;
    accent-color: var(--focus);
  }

  .num {
    width: 6rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .prow {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4375rem 0;
    border-bottom: 1px solid var(--border);
  }

  .prow:last-child {
    border-bottom: none;
  }

  .plogo {
    display: grid;
    place-items: center;
    width: 1.75rem;
    height: 1.75rem;
    border-radius: 999px;
    background: var(--surface-2);
    flex: none;
  }

  .pname {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .icon {
    padding: 0.125rem 0.4375rem;
    line-height: 1.2;
  }

  .danger {
    color: var(--critical);
  }

  .pricing {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding-top: 0.75rem;
    margin-top: 0.25rem;
    border-top: 1px solid var(--border);
  }

  .pricing-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .label {
    font-weight: 500;
  }

  .small {
    font-size: 0.75rem;
  }

  .table-wrap {
    overflow-x: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.8125rem;
  }

  th {
    text-align: left;
    font-weight: 500;
    color: var(--muted);
    padding: 0 0.25rem 0.25rem;
    white-space: nowrap;
  }

  th.n,
  td.n {
    text-align: right;
  }

  td {
    padding: 0.125rem 0.25rem;
  }

  .pattern {
    width: 100%;
    min-width: 10rem;
  }

  .price {
    width: 5.25rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .pricing-actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
    align-items: center;
    flex-wrap: wrap;
  }

  .path {
    max-width: 18rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.75rem;
  }

  .err {
    grid-column: 1 / -1;
    margin: 0;
    color: var(--critical);
    font-size: 0.8125rem;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .save-state {
    grid-column: 1 / -1;
    margin: 0;
  }
</style>
