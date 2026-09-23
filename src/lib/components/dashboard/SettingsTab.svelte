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
    getProviders,
    getShortcutStatus,
    isTauri,
    quitApp,
    reingestLogs,
    setPricing,
    useSourcePricing,
  } from '$lib/api';
  import { formatAgo } from '$lib/format';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { providerDisplayName } from '$lib/providers';
  import { shortcutProblem } from '$lib/shortcuts';
  import { defaultSidebarItems, settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import { update } from '$lib/stores/update.svelte';
  import { pricingUpdate } from '$lib/stores/pricing-update.svelte';
  import type {
    AppInfo,
    CyberAccent,
    Edge,
    Language,
    MonitorInfo,
    PercentMode,
    PercentPosition,
    PricingEntry,
    PricingTable,
    ProviderId,
    ProviderInfo,
    RingMode,
    ShortcutStatus,
    SidebarItems,
    SurfaceStyle,
    Theme,
    VerticalAlign,
  } from '$lib/types';

  const GITHUB_URL = 'https://github.com/harveyxiacn/AI-usage-monitor-sidebar';

  const s = $derived(settings.value);

  let monitors = $state<MonitorInfo[]>([]);
  let providerInfos = $state<ProviderInfo[]>([]);
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
  /** Why a global shortcut is not active, as reported by the backend. */
  let shortcuts = $state<ShortcutStatus>({ toggleSidebar: null, openDashboard: null });

  /** Provider rows: every provider the backend reports, ordered by settings. */
  const providerRows = $derived.by(() => {
    const ids = new Set<ProviderId>((snapshot.value?.providers ?? []).map((p) => p.provider));
    for (const info of providerInfos) ids.add(info.id);
    for (const id of Object.keys(s.providers)) ids.add(id as ProviderId);
    return [...ids].sort((a, b) => (s.providers[a]?.order ?? 0) - (s.providers[b]?.order ?? 0));
  });

  /** `get_providers` is the authority on which providers exist and their state. */
  const providerInfoOf = (id: ProviderId) => providerInfos.find((p) => p.id === id) ?? null;

  onMount(() => {
    const disposeUpdate = update.init();
    const disposePricingUpdate = pricingUpdate.init();
    void getProviders().then((p) => (providerInfos = p)).catch((e) => (actionError = String(e)));
    void getMonitors().then((m) => (monitors = m)).catch((e) => (actionError = String(e)));
    void getAppInfo().then((i) => (appInfo = i)).catch((e) => (actionError = String(e)));
    void loadPricing();
    void refreshShortcutStatus();
    return () => {
      clearTimeout(savedTimer);
      disposeUpdate();
      disposePricingUpdate();
    };
  });

  /** The backend re-registers on `settings-updated`; ask it what happened. */
  async function refreshShortcutStatus() {
    try { shortcuts = await getShortcutStatus(); }
    catch (e) { actionError = String(e); }
  }

  async function patchShortcut(key: 'shortcutToggleSidebar' | 'shortcutOpenDashboard', value: string) {
    await settings.patch({ [key]: value.trim() });
    await refreshShortcutStatus();
  }

  /** Local complaint first (instant), then whatever registration reported. */
  function shortcutHint(value: string, failure: string | null): string | undefined {
    if (shortcutProblem(value)) return t('settings.shortcutInvalid');
    if (failure) return t('settings.shortcutFailed', { message: failure });
    return undefined;
  }

  const u = $derived(update.value);
  const pu = $derived(pricingUpdate.value);

  const updateSummary = $derived.by(() => {
    if (!u) return t('update.unknown');
    if (u.checking) return t('update.checking');
    if (u.available) return t('update.available', { version: u.available });
    if (!u.checkedAt) return t('update.unknown');
    return t('update.upToDate');
  });

  async function openUrl(url: string) {
    if (isTauri()) {
      const { openUrl: open } = await import('@tauri-apps/plugin-opener');
      await open(url);
    } else {
      window.open(url, '_blank', 'noopener');
    }
  }

  const providerName = (id: ProviderId) =>
    snapshot.value?.providers.find((p) => p.provider === id)?.displayName ??
    providerInfos.find((p) => p.id === id)?.displayName ??
    providerDisplayName(id);

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

  async function checkPricingUpdates() {
    await pricingUpdate.check();
  }

  /** A source application must never overwrite edits that have not been saved. */
  async function applyPricingUpdate() {
    if (pricingDirty) return;
    const table = await pricingUpdate.apply();
    if (!table || pricingDirty) return;
    pricing = table;
    pricingSaved = true;
    clearTimeout(savedTimer);
    savedTimer = setTimeout(() => (pricingSaved = false), 1500);
  }

  /** Deliberately separate from Apply: this discards the active manual table. */
  async function switchToSourcePricing() {
    if (pricingDirty) return;
    if (!window.confirm(t('settings.pricing.useSourceConfirm'))) return;
    pricingError = null;
    try {
      const table = await useSourcePricing();
      if (!pricingDirty) pricing = table;
      await pricingUpdate.check();
    } catch (e) { pricingError = String(e); }
  }

  async function patchPricingUrl(value: string) {
    await settings.patch({ pricingUrl: value.trim() });
    // A different source cannot reuse the previous source's offer. The check
    // replaces the status immediately after the setting has been persisted.
    await pricingUpdate.check();
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

  const openGithub = () => openUrl(GITHUB_URL);

  async function runAction(action: () => Promise<unknown>) {
    actionError = null;
    try { await action(); } catch (e) { actionError = String(e); }
  }

  const THEMES: Theme[] = ['auto', 'dark', 'light'];
  const LANGUAGES: Language[] = ['auto', 'en', 'zh-CN'];
  const EDGES: Edge[] = ['left', 'right', 'top', 'bottom'];
  const ALIGNS: VerticalAlign[] = ['top', 'center', 'bottom'];
  /**
   * `verticalAlign`/`verticalOffset` describe the position *along* the docked
   * edge, so on a top/bottom edge they are horizontal. The wire values stay
   * `top|center|bottom`; only the labels follow the orientation (top → left,
   * bottom → right).
   */
  const alongIsHorizontal = $derived(s.edge === 'top' || s.edge === 'bottom');
  const ALIGN_LABELS: Record<VerticalAlign, string> = { top: 'left', center: 'center', bottom: 'right' };
  const alignLabel = (v: VerticalAlign) =>
    alongIsHorizontal ? `settings.horizontalAlign.${ALIGN_LABELS[v]}` : `settings.verticalAlign.${v}`;
  const RING_MODES: RingMode[] = ['concentric', 'primary', 'all'];
  /** "Sidebar items" rows, in the order they are declared in the contract. */
  const SIDEBAR_ITEM_KEYS = Object.keys(defaultSidebarItems) as (keyof SidebarItems)[];
  const SURFACE_STYLES: SurfaceStyle[] = ['glass', 'solid', 'cyber'];
  const CYBER_ACCENTS: CyberAccent[] = ['neon', 'matrix', 'amber', 'ice', 'synthwave'];
  const PERCENT_MODES: PercentMode[] = ['used', 'remaining'];
  const PERCENT_POSITIONS: PercentPosition[] = ['below', 'center'];
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
  </article>

  <article class="card group">
    <h3>{t('settings.sidebarItems')}</h3>
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
  </article>

  <SizeColourGroup />

  <article class="card group">
    <h3>{t('settings.position')}</h3>

    <Field label={t('settings.edge')}>
      <select aria-label={t('settings.edge')} class="field" value={s.edge} onchange={(e) => void settings.patch({ edge: e.currentTarget.value as Edge })}>
        {#each EDGES as v (v)}<option value={v}>{tDyn(`settings.edge.${v}`)}</option>{/each}
      </select>
    </Field>

    <Field label={tDyn(alongIsHorizontal ? 'settings.horizontalAlign' : 'settings.verticalAlign')}>
      <select aria-label={tDyn(alongIsHorizontal ? 'settings.horizontalAlign' : 'settings.verticalAlign')} class="field" value={s.verticalAlign} onchange={(e) => void settings.patch({ verticalAlign: e.currentTarget.value as VerticalAlign })}>
        {#each ALIGNS as v (v)}<option value={v}>{tDyn(alignLabel(v))}</option>{/each}
      </select>
    </Field>

    <Field label={tDyn(alongIsHorizontal ? 'settings.horizontalOffset' : 'settings.verticalOffset')}>
      <input
        class="field num"
        type="number"
        step="1"
        value={s.verticalOffset}
        onchange={(e) => void settings.patch({ verticalOffset: Math.round(num(e)) })}
        aria-label={tDyn(alongIsHorizontal ? 'settings.horizontalOffset' : 'settings.verticalOffset')}
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

    <Field label={t('settings.popoverTimeoutSec')} hint={t('settings.popoverTimeoutSec.hint')}>
      <input
        class="field num"
        type="number"
        min="0"
        max="600"
        step="1"
        value={s.popoverTimeoutSec}
        onchange={(e) => void settings.patch({ popoverTimeoutSec: Math.min(600, Math.max(0, Math.round(num(e)))) })}
        aria-label={t('settings.popoverTimeoutSec')}
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

    <Field label={t('settings.refreshIntervalSec')} hint={t('settings.refreshIntervalSec.hint')}>
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

    <Field label={t('settings.adaptiveRefresh')} hint={t('settings.adaptiveRefresh.hint')}>
      <Toggle
        checked={s.adaptiveRefresh}
        label={t('settings.adaptiveRefresh')}
        onchange={(v) => void settings.patch({ adaptiveRefresh: v })}
      />
    </Field>

    <Field label={t('settings.notifications')}>
      <Toggle
        checked={s.notifications}
        label={t('settings.notifications')}
        onchange={(v) => void settings.patch({ notifications: v })}
      />
    </Field>

    <Field label={t('settings.forecastNotifications')}>
      <Toggle
        checked={s.forecastNotifications}
        disabled={!s.notifications}
        label={t('settings.forecastNotifications')}
        onchange={(v) => void settings.patch({ forecastNotifications: v })}
      />
    </Field>
    <Field label={t('settings.hideAccountEmail')} hint={t('settings.hideAccountEmail.hint')}>
      <Toggle
        checked={s.hideAccountEmail}
        label={t('settings.hideAccountEmail')}
        onchange={(v) => void settings.patch({ hideAccountEmail: v })}
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

    <Field label={t('settings.autoUpdateCheck')}>
      <Toggle
        checked={s.autoUpdateCheck}
        label={t('settings.autoUpdateCheck')}
        onchange={(v) => void settings.patch({ autoUpdateCheck: v })}
      />
    </Field>

    <!-- The overlays are dock windows and never take focus, so a global
         shortcut is the only keyboard path to them. Empty = not registered. -->
    <Field
      label={t('settings.shortcutToggleSidebar')}
      hint={shortcutHint(s.shortcutToggleSidebar, shortcuts.toggleSidebar) ?? t('settings.shortcutHint')}
    >
      <input
        class="field shortcut"
        type="text"
        spellcheck="false"
        placeholder="Ctrl+Alt+U"
        value={s.shortcutToggleSidebar}
        onchange={(e) => void patchShortcut('shortcutToggleSidebar', e.currentTarget.value)}
        aria-label={t('settings.shortcutToggleSidebar')}
      />
    </Field>

    <Field
      label={t('settings.shortcutOpenDashboard')}
      hint={shortcutHint(s.shortcutOpenDashboard, shortcuts.openDashboard) ?? t('settings.shortcutWayland')}
    >
      <input
        class="field shortcut"
        type="text"
        spellcheck="false"
        placeholder="Ctrl+Alt+D"
        value={s.shortcutOpenDashboard}
        onchange={(e) => void patchShortcut('shortcutOpenDashboard', e.currentTarget.value)}
        aria-label={t('settings.shortcutOpenDashboard')}
      />
    </Field>
  </article>

  <article class="card group">
    <h3>{t('settings.providers')}</h3>
    {#each providerRows as id, i (id)}
      {@const info = providerInfoOf(id)}
      <div class="prow">
        <span class="plogo"><ProviderLogo provider={id} size={20} /></span>
        <span class="pname">{providerName(id)}</span>
        {#if info?.experimental}
          <span class="badge" title={t('settings.provider.experimental.hint')}>
            {t('settings.provider.experimental')}
          </span>
        {/if}
        <button class="btn icon" disabled={i === 0} onclick={() => void move(id, -1)} aria-label={t('common.up')}>↑</button>
        <button class="btn icon" disabled={i === providerRows.length - 1} onclick={() => void move(id, 1)} aria-label={t('common.down')}>↓</button>
        <span class="pcol" title={t('settings.providerEnabled')}>
          <span class="pcap">{t('settings.providerEnabled.short')}</span>
          <Toggle
            checked={s.providers[id]?.enabled ?? true}
            label={`${providerName(id)} — ${t('settings.providerEnabled')}`}
            onchange={(v) => void settings.patchProvider(id, { enabled: v })}
          />
        </span>
        <span class="pcol" title={t('settings.sidebarItems.provider')}>
          <span class="pcap">{t('settings.sidebarItems.provider.short')}</span>
          <Toggle
            checked={s.providers[id]?.showInSidebar ?? true}
            label={`${providerName(id)} — ${t('settings.sidebarItems.provider')}`}
            disabled={!(s.providers[id]?.enabled ?? true)}
            onchange={(v) => void settings.patchProvider(id, { showInSidebar: v })}
          />
        </span>
      </div>
    {/each}
    <p class="note">{t('settings.providers.hint')}</p>
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

    <Field label={t('settings.autoPricingCheck')} hint={t('settings.autoPricingCheck.hint')}>
      <Toggle
        checked={s.autoPricingCheck}
        label={t('settings.autoPricingCheck')}
        onchange={(v) => void settings.patch({ autoPricingCheck: v })}
      />
    </Field>

    <Field label={t('settings.pricingUrl')} hint={t('settings.pricingUrl.hint')} wide>
      <input
        class="field url"
        type="url"
        inputmode="url"
        placeholder="https://…/pricing.json"
        value={s.pricingUrl}
        onchange={(e) => void patchPricingUrl((e.currentTarget as HTMLInputElement).value)}
        aria-label={t('settings.pricingUrl')}
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
        <button class="btn" onclick={() => void checkPricingUpdates()} disabled={pricingSaving || pu?.checking}>
          {pu?.checking ? t('pricingUpdate.checking') : t('pricingUpdate.check')}
        </button>
        {#if pu?.customPricing}
          <span class="muted small">{t('settings.pricing.customSource')}</span>
          <button
            class="btn"
            onclick={() => void switchToSourcePricing()}
            disabled={pricingSaving || pricingDirty}
            title={pricingDirty ? t('settings.pricing.saveFirst') : undefined}
          >{t('settings.pricing.useSource')}</button>
        {:else}
          {#if pu?.available}
            <button
              class="btn btn-primary"
              onclick={() => void applyPricingUpdate()}
              disabled={pricingSaving || pricingDirty || pu.applying}
              title={pricingDirty ? t('settings.pricing.saveFirst') : undefined}
            >{pu.applying ? t('pricingUpdate.applying') : t('pricingUpdate.apply')}</button>
          {/if}
        {/if}
        <button class="btn" onclick={addPricingRow} disabled={!pricing || pricingSaving}>{t('settings.pricing.add')}</button>
        <button
          class="btn btn-primary"
          onclick={() => void savePricing()}
          disabled={!pricing || pricingSaving}
          title={t('settings.pricing.saveCustomHint')}
        >
          {pricingSaving ? t('common.saving') : pricingSaved ? t('common.saved') : t('settings.pricing.save')}
        </button>
      </div>
      <div class="pricing-update-status" role="status">
        {#if pu?.error || pricingUpdate.error}
          <span class="err">{t('common.error', { message: pu?.error ?? pricingUpdate.error ?? '' })}</span>
        {:else if pu?.checking}
          <span class="muted small">{t('pricingUpdate.checking')}</span>
        {:else if pu?.customPricing && pu.available}
          <span class="offer">{t('pricingUpdate.customAvailable', { revision: pu.revision ?? '—' })}</span>
        {:else if pu?.customPricing}
          <span class="muted small">{t('settings.pricing.customSource')}</span>
        {:else if pu?.available}
          <span class="offer">{t('pricingUpdate.available', { revision: pu.revision ?? '—' })}</span>
        {:else if pu?.checkedAt}
          <span class="muted small">{t('pricingUpdate.upToDate', { ago: formatAgo(pu.checkedAt) })}</span>
        {:else}
          <span class="muted small">{t('pricingUpdate.unknown')}</span>
        {/if}
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

    <!-- Updates are never installed without this click; a build owned by a
         package manager only gets the release link. -->
    <Field
      label={t('settings.about.programUpdates')}
      hint={u?.checkedAt ? t('update.checkedAt', { ago: formatAgo(u.checkedAt) }) : undefined}
    >
      <span class="upd-state" class:offer={!!u?.available}>{updateSummary}</span>
      <button class="btn" disabled={u?.checking || u?.installing} onclick={() => void update.check()}>
        {t('update.check')}
      </button>
    </Field>

    {#if u?.available}
      <p class="muted small">
        {#if u.canInstall}
          <button class="btn btn-primary" disabled={u.installing} onclick={() => void update.install()}>
            {u.installing ? t('update.installing') : t('update.install')}
          </button>
        {:else}
          {t('update.managed')}
        {/if}
        <button class="btn link" onclick={() => void runAction(() => openUrl(u.releaseUrl))}>
          {u.canInstall ? t('update.notes') : t('update.openRelease')}
        </button>
      </p>
    {/if}
    {#if update.error}<p class="err" role="alert">{t('common.error', { message: update.error })}</p>{/if}

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

  /* "experimental": the provider's quota source could not be verified against
     a live account, so the row says so rather than the README alone */
  .badge {
    flex: none;
    padding: 0.0625rem 0.375rem;
    border: 1px solid var(--border-strong);
    border-radius: 999px;
    font-size: 0.625rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--warn);
    white-space: nowrap;
  }

  .icon {
    padding: 0.125rem 0.4375rem;
    line-height: 1.2;
  }

  /* the two provider switches need a caption each to be told apart */
  .pcol {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.125rem;
    flex: none;
  }

  .pcap {
    font-size: 0.625rem;
    line-height: 1;
    color: var(--muted);
    white-space: nowrap;
  }

  .note {
    margin: 0.375rem 0;
    font-size: 0.6875rem;
    color: var(--faint);
  }

  .danger {
    color: var(--critical);
  }

  .shortcut {
    width: 9rem;
    font-variant-numeric: tabular-nums;
  }

  /* an offer is worth noticing, but this is not an alert */
  .upd-state.offer {
    color: var(--focus);
    font-weight: 500;
  }

  .link {
    padding-inline: 0;
    border: none;
    background: none;
    color: var(--focus);
    text-decoration: underline;
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

  .url {
    width: 100%;
    min-width: 0;
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

  .pricing-update-status {
    min-height: 1.1rem;
    font-size: 0.75rem;
  }

  .pricing-update-status .offer {
    color: var(--focus);
    font-weight: 500;
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
