<!--
  "Share card": renders this month's usage as a 1200x630 PNG, previews it, and
  copies it (clipboard image) or saves it (native save dialog). [FRONTEND]

  Only numbers and provider display names go onto the card: never an account
  e-mail, never a project path or a session title. The cost is an estimate and
  labelled as one; "Hide cost" leaves it off entirely.
-->
<script lang="ts">
  import { tick } from 'svelte';
  import { getUsageHistory, saveShareCard } from '$lib/api';
  import { providerAccent, resolveColor } from '$lib/colors';
  import { trapTab } from '$lib/focus-trap';
  import { formatCost, formatTokens, shortPercent } from '$lib/format';
  import { intlLocale, t } from '$lib/i18n/i18n.svelte';
  import { providerDisplayName } from '$lib/providers';
  import {
    CARD_HEIGHT,
    CARD_WIDTH,
    buildShareModel,
    drawShareCard,
    layoutShareCard,
    monthBounds,
    shareFileName,
    type ShareModel,
  } from '$lib/share-card';
  import { overlays } from '$lib/stores/overlays.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import { snapshot } from '$lib/stores/snapshot.svelte';
  import type { HistoryResult } from '$lib/types';

  let canvas = $state<HTMLCanvasElement | null>(null);
  let dialog = $state<HTMLElement | null>(null);
  let hideCost = $state(false);
  let usage = $state<HistoryResult | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let status = $state<string | null>(null);
  let busy = $state(false);
  let restoreTo: HTMLElement | null = null;
  let requestId = 0;

  const canCopy = typeof ClipboardItem !== 'undefined' && typeof navigator !== 'undefined' && !!navigator.clipboard?.write;

  async function load() {
    const id = ++requestId;
    loading = true;
    error = null;
    try {
      const { from, to } = monthBounds(Date.now());
      const result = await getUsageHistory({
        from: new Date(from).toISOString(),
        to: new Date(to).toISOString(),
        bucket: 'month',
        groupByModel: true,
        provider: null,
      });
      if (id === requestId) usage = result;
    } catch (e) {
      // the rings do not need the history: draw the card without the month totals
      if (id === requestId) {
        usage = null;
        error = String(e);
      }
    } finally {
      if (id === requestId) loading = false;
    }
  }

  function model(): ShareModel {
    const s = settings.value;
    const now = Date.now();
    return buildShareModel({
      providers: snapshot.value?.providers ?? [],
      providerSettings: s.providers,
      thresholds: s.thresholds,
      percentMode: s.percentMode,
      accentOf: (provider) => resolveColor(providerAccent(provider)),
      monthLabel: new Date(now).toLocaleDateString(intlLocale(), { month: 'long', year: 'numeric' }),
      totals: usage?.totals ?? null,
      rows: usage?.rows ?? [],
      hideCost,
      theme: document.documentElement.dataset.theme === 'light' ? 'light' : 'dark',
      labels: {
        title: t('share.title'),
        thisMonth: t('share.thisMonth'),
        tokens: t('share.tokens'),
        cost: t('share.cost'),
        estimate: t('share.estimate'),
        topModel: t('share.topModel'),
        appName: t('app.name'),
        costNote: t('share.costNote'),
        noData: t('share.noData'),
      },
      format: {
        tokens: formatTokens,
        cost: formatCost,
        percent: (used) => shortPercent(used, s.percentMode),
      },
    });
  }

  function draw() {
    const ctx = canvas?.getContext('2d');
    if (!canvas || !ctx) return;
    const m = model();
    drawShareCard(ctx, m, layoutShareCard(m), {
      warn: settings.value.colors.warn || undefined,
      critical: settings.value.colors.critical || undefined,
    });
  }

  // fetch when opened; repaint whenever an input of the card changes
  $effect(() => {
    if (overlays.share) void load();
  });
  $effect(() => {
    void [usage, hideCost, snapshot.value, settings.value, canvas];
    if (overlays.share && canvas) draw();
  });

  // focus: into the dialog on open, back to the opener on close
  let wasOpen = false;
  $effect(() => {
    const open = overlays.share;
    if (open && !wasOpen) {
      restoreTo = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      status = null;
      void tick().then(() => dialog?.querySelector<HTMLElement>('input, button')?.focus());
    } else if (!open && wasOpen) {
      const target = restoreTo;
      restoreTo = null;
      requestId++;
      usage = null;
      void tick().then(() => target?.isConnected && target.focus());
    }
    wasOpen = open;
  });

  function close() {
    overlays.share = false;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (dialog) trapTab(e, dialog);
  }

  function toBlob(): Promise<Blob> {
    return new Promise((resolve, reject) => {
      if (!canvas) return reject(new Error('no canvas'));
      canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error('PNG encoding failed'))), 'image/png');
    });
  }

  async function copyImage() {
    busy = true;
    status = null;
    try {
      await navigator.clipboard.write([new ClipboardItem({ 'image/png': await toBlob() })]);
      status = t('share.copied');
    } catch (e) {
      status = t('common.error', { message: String(e) });
    } finally {
      busy = false;
    }
  }

  async function savePng() {
    busy = true;
    status = null;
    try {
      const bytes = new Uint8Array(await (await toBlob()).arrayBuffer());
      const saved = await saveShareCard(bytes, shareFileName(Date.now()));
      status = saved ? t('share.saved', { path: saved }) : null;
    } catch (e) {
      status = t('common.error', { message: String(e) });
    } finally {
      busy = false;
    }
  }
</script>

{#if overlays.share}
  <div class="backdrop" role="presentation" onmousedown={(e) => { if (e.target === e.currentTarget) close(); }}>
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="share-title" tabindex="-1" bind:this={dialog} onkeydown={onKey}>
      <header>
        <h2 id="share-title">{t('share.heading')}</h2>
        <button class="btn" type="button" onclick={close}>{t('common.close')}</button>
      </header>

      <canvas
        bind:this={canvas}
        width={CARD_WIDTH}
        height={CARD_HEIGHT}
        class="preview"
      >{t('share.previewLabel')}</canvas>

      {#if loading}<p class="muted" role="status">{t('common.loading')}</p>{/if}
      {#if error}<p class="err" role="alert">{t('common.error', { message: error })}</p>{/if}

      <p class="note muted">{t('share.privacy')}</p>

      <div class="actions">
        <label class="check">
          <input type="checkbox" bind:checked={hideCost} />
          {t('share.hideCost')}
        </label>
        <span class="spacer"></span>
        <button class="btn" type="button" disabled={busy || !canCopy} title={canCopy ? undefined : t('share.copyUnavailable')} onclick={() => void copyImage()}>
          {t('share.copy')}
        </button>
        <button class="btn btn-primary" type="button" disabled={busy} onclick={() => void savePng()}>{t('share.save')}</button>
      </div>
      <p class="status muted" role="status" aria-live="polite">{status ?? ''}</p>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    justify-content: center;
    align-items: center;
    padding: 1rem;
    background: rgb(0 0 0 / 0.5);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    width: min(46rem, 100%);
    max-height: 100%;
    padding: 1rem 1.25rem 1rem;
    border: 1px solid var(--border-strong, var(--border));
    border-radius: var(--r-card);
    background: var(--surface);
    box-shadow: var(--shadow-card);
    overflow-y: auto;
  }

  .dialog:focus {
    outline: none;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }

  h2 {
    margin: 0;
    font-size: 1rem;
  }

  .preview {
    width: 100%;
    height: auto;
    aspect-ratio: 1200 / 630;
    border: 1px solid var(--border);
    border-radius: var(--r-control);
  }

  .note,
  .status {
    margin: 0;
    font-size: 0.75rem;
    min-height: 1em;
  }

  .err {
    margin: 0;
    color: var(--critical);
    font-size: 0.8125rem;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }

  .spacer {
    flex: 1 1 auto;
  }

  .check {
    display: inline-flex;
    align-items: center;
    gap: 0.4375rem;
    font-size: 0.8125rem;
  }
</style>
