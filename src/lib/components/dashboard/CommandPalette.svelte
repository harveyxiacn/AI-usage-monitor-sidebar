<!--
  Dashboard command palette (Ctrl+K / Cmd+K). [FRONTEND]

  A modal dialog with the combobox + listbox pattern: the search input keeps
  focus, ArrowUp/Down move the highlighted option (announced through
  aria-activedescendant), Enter runs it, Escape closes, Tab is trapped inside
  and focus returns to whatever had it before. The commands come from the pure
  registry in `$lib/palette-commands`; this file only wires them to the app.
-->
<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { exportUsageCsv, getDiagnostics, getUsageHistory, openFolder, reingestLogs, toggleSidebar } from '$lib/api';
  import { formatDiagnostics } from '$lib/diagnostics-format';
  import { requestDashboardTab } from '$lib/dashboard-nav';
  import { trapTab } from '$lib/focus-trap';
  import { historyCsv, localDateInput } from '$lib/history';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { rankCommands, type PaletteCommand } from '$lib/palette';
  import { buildCommands, type PaletteContext } from '$lib/palette-commands';
  import { requestHistoryView, requestSessionsView, requestSettingsCard } from '$lib/palette-nav';
  import { st } from '$lib/session-labels.svelte';
  import { overlays } from '$lib/stores/overlays.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';

  let query = $state('');
  let active = $state(0);
  let input = $state<HTMLInputElement | null>(null);
  let dialog = $state<HTMLElement | null>(null);
  let list = $state<HTMLElement | null>(null);
  /** what had focus before the palette opened, so closing can give it back */
  let restoreTo: HTMLElement | null = null;
  /** one-line outcome of the last action ("Diagnostics copied"), shown for a moment */
  let toast = $state('');
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  function say(message: string) {
    toast = message;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = ''), 3500);
  }
  onDestroy(() => clearTimeout(toastTimer));

  /** Run an async action and report the outcome instead of failing silently. */
  async function report(work: () => Promise<string | null>) {
    try {
      const message = await work();
      if (message) say(message);
    } catch (e) {
      say(t('common.error', { message: String(e) }));
    }
  }

  async function copyDiagnostics() {
    const text = formatDiagnostics(await getDiagnostics());
    await navigator.clipboard.writeText(text);
    return t('palette.done.diagnostics');
  }

  async function exportLastMonth() {
    const to = Date.now();
    const from = to - 30 * 24 * 3_600_000;
    const result = await getUsageHistory({
      from: new Date(from).toISOString(),
      to: new Date(to).toISOString(),
      bucket: 'day',
      groupByModel: true,
      provider: null,
    });
    const saved = await exportUsageCsv(historyCsv(result.rows), `ai-usage-${localDateInput(from)}-${localDateInput(to)}.csv`);
    return saved ? t('palette.done.csv', { path: saved }) : null;
  }

  function context(): PaletteContext {
    return {
      settings: settings.value,
      t: (key, params) => tDyn(key, params),
      sessionsLabel: (key) => st(key),
      now: () => Date.now(),
      navigate: requestDashboardTab,
      openHistoryView: requestHistoryView,
      openSessionsView: requestSessionsView,
      openSettingsCard: requestSettingsCard,
      refresh: () => void report(async () => { await snapshot.refresh(); return t('palette.done.refresh'); }),
      rescan: () => void report(async () => { await reingestLogs(); return t('palette.done.rescan'); }),
      toggleSidebar: () => void report(async () => { await toggleSidebar(); return null; }),
      copyDiagnostics: () => void report(copyDiagnostics),
      exportCsv: () => void report(exportLastMonth),
      openLogFolder: () => void report(async () => { await openFolder('log'); return null; }),
      shareCard: () => (overlays.share = true),
      patch: (patch) => void settings.patch(patch),
    };
  }

  // built only while open, so a closed palette costs nothing
  const commands = $derived(overlays.palette ? buildCommands(context()) : []);
  const results = $derived(rankCommands(query, commands));

  // a new query starts at the top again
  $effect(() => {
    void query;
    active = 0;
  });

  // open: remember focus, reset, focus the input. close: give focus back.
  let wasOpen = false;
  $effect(() => {
    const open = overlays.palette;
    if (open && !wasOpen) {
      restoreTo = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      query = '';
      active = 0;
      void tick().then(() => input?.focus());
    } else if (!open && wasOpen) {
      const target = restoreTo;
      restoreTo = null;
      void tick().then(() => target?.isConnected && target.focus());
    }
    wasOpen = open;
  });

  function close() {
    overlays.palette = false;
  }

  function run(command: PaletteCommand | undefined) {
    if (!command) return;
    close();
    // run after the dialog is gone so a navigation can take focus where it wants
    void tick().then(() => command.run());
  }

  function scrollActive() {
    void tick().then(() => list?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' }));
  }

  function onKey(e: KeyboardEvent) {
    if (dialog) trapTab(e, dialog);
    const n = results.length;
    switch (e.key) {
      case 'Escape':
        e.preventDefault();
        e.stopPropagation();
        close();
        break;
      case 'ArrowDown':
        e.preventDefault();
        if (n) active = (active + 1) % n;
        scrollActive();
        break;
      case 'ArrowUp':
        e.preventDefault();
        if (n) active = (active - 1 + n) % n;
        scrollActive();
        break;
      case 'Home':
        if (e.target === input && query === '') { e.preventDefault(); active = 0; scrollActive(); }
        break;
      case 'End':
        if (e.target === input && query === '') { e.preventDefault(); active = Math.max(0, n - 1); scrollActive(); }
        break;
      case 'Enter':
        if (!e.isComposing) {
          e.preventDefault();
          run(results[active]);
        }
        break;
    }
  }

  function onWindowKey(e: KeyboardEvent) {
    if (e.key.toLowerCase() === 'k' && (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey) {
      if (overlays.share) return;
      e.preventDefault();
      overlays.palette = !overlays.palette;
    }
  }

  const groupLabel = (group: PaletteCommand['group']) => tDyn(`palette.group.${group}`);
</script>

<svelte:window onkeydown={onWindowKey} />

{#if overlays.palette}
  <div class="backdrop" role="presentation" onmousedown={(e) => { if (e.target === e.currentTarget) close(); }}>
    <div class="dialog" role="dialog" aria-modal="true" aria-label={t('palette.title')} tabindex="-1" bind:this={dialog} onkeydown={onKey}>
      <input
        bind:this={input}
        bind:value={query}
        class="search"
        type="text"
        role="combobox"
        aria-expanded="true"
        aria-controls="palette-list"
        aria-autocomplete="list"
        aria-activedescendant={results.length > 0 ? `palette-opt-${active}` : undefined}
        aria-label={t('palette.title')}
        placeholder={t('palette.placeholder')}
        spellcheck="false"
        autocomplete="off"
      />
      <ul id="palette-list" class="list" role="listbox" aria-label={t('palette.results')} bind:this={list}>
        {#each results as command, i (command.id)}
          <!-- the input keeps focus and handles the keys; the option only needs the pointer -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li
            id="palette-opt-{i}"
            role="option"
            aria-selected={i === active}
            class:active={i === active}
            onclick={() => run(command)}
            onmousemove={() => { if (active !== i) active = i; }}
          >
            <span class="group">{groupLabel(command.group)}</span>
            <span class="title">{command.title}</span>
            {#if command.hint}<span class="hint">{command.hint}</span>{/if}
          </li>
        {/each}
      </ul>
      {#if results.length === 0}
        <p class="empty muted">{t('palette.empty')}</p>
      {/if}
      <p class="sr-only" role="status" aria-live="polite">{t('palette.count', { count: results.length })}</p>
      <footer class="keys muted" aria-hidden="true">
        <span><kbd>↑</kbd><kbd>↓</kbd> {t('palette.keys.move')}</span>
        <span><kbd>Enter</kbd> {t('palette.keys.run')}</span>
        <span><kbd>Esc</kbd> {t('palette.keys.close')}</span>
      </footer>
    </div>
  </div>
{/if}

{#if toast}
  <p class="toast" role="status">{toast}</p>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding: 12vh 1rem 1rem;
    background: rgb(0 0 0 / 0.45);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    width: min(38rem, 100%);
    max-height: min(32rem, 76vh);
    border: 1px solid var(--border-strong, var(--border));
    border-radius: var(--r-card);
    background: var(--surface);
    box-shadow: var(--shadow-card);
    overflow: hidden;
  }

  .dialog:focus {
    outline: none;
  }

  .search {
    flex: none;
    width: 100%;
    padding: 0.875rem 1rem;
    border: none;
    border-bottom: 1px solid var(--border);
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 1rem;
  }

  .search:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }

  .list {
    flex: 1 1 auto;
    min-height: 0;
    margin: 0;
    padding: 0.375rem;
    list-style: none;
    overflow-y: auto;
  }

  .list li {
    display: flex;
    align-items: baseline;
    gap: 0.625rem;
    padding: 0.5rem 0.625rem;
    border-radius: var(--r-control);
    cursor: pointer;
  }

  .list li.active {
    background: var(--surface-3, var(--hover));
    outline: 1px solid var(--focus);
    outline-offset: -1px;
  }

  .group {
    flex: none;
    min-width: 4.5rem;
    color: var(--faint, var(--muted));
    font-size: 0.6875rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .title {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hint {
    flex: none;
    color: var(--muted);
    font-size: 0.75rem;
  }

  .empty {
    margin: 0;
    padding: 1.25rem 1rem;
  }

  .keys {
    flex: none;
    display: flex;
    gap: 1rem;
    padding: 0.5rem 1rem;
    border-top: 1px solid var(--border);
    font-size: 0.6875rem;
  }

  kbd {
    margin-right: 0.1875rem;
    padding: 0 0.3125rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    font: inherit;
  }

  .toast {
    position: fixed;
    left: 50%;
    bottom: 1.5rem;
    z-index: 60;
    max-width: min(32rem, calc(100vw - 2rem));
    margin: 0;
    padding: 0.5rem 0.875rem;
    transform: translateX(-50%);
    border: 1px solid var(--border-strong, var(--border));
    border-radius: var(--r-control);
    background: var(--surface-2);
    box-shadow: var(--shadow-card);
    font-size: 0.8125rem;
    overflow-wrap: anywhere;
  }
</style>
