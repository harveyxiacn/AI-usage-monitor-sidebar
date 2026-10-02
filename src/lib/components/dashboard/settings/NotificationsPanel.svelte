<!--
  Settings → Notifications. [FRONTEND]

  Standalone and props-free: it reads and writes the settings store like the
  other settings cards. It holds the master switch, one switch per alert type
  (threshold crossing, forecast, monthly budget, weekly summary), the warning /
  critical dual slider with a colour preview, the webhook channel with test
  buttons, the focus-mode status and the OS permission hint.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Field from '../Field.svelte';
  import Toggle from '../Toggle.svelte';
  import { getNotificationPermission, sendTestNotification } from '$lib/api';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { focusStatus, moveThreshold, thresholdZones, webhookUrlProblem } from '$lib/notifications';
  import { settings } from '$lib/stores/settings.svelte';
  import type { Thresholds, WebhookKind } from '$lib/types';

  const s = $derived(settings.value);
  const KINDS: WebhookKind[] = ['generic', 'ntfy', 'slack'];

  // ---- thresholds: the preview follows the thumb while it moves ----
  let draft = $state<Thresholds | null>(null);
  const levels = $derived(draft ?? s.thresholds);
  const zones = $derived(thresholdZones(levels));

  function dragThreshold(which: keyof Thresholds, value: number) {
    draft = moveThreshold(levels, which, value);
  }

  function commitThresholds() {
    if (!draft) return;
    const next = draft;
    draft = null;
    void settings.patch({ thresholds: next });
  }

  // ---- webhook ----
  let urlDraft = $state<string | null>(null);
  const url = $derived(urlDraft ?? s.webhook.url);
  const urlProblem = $derived(webhookUrlProblem(url));
  // an empty field is neutral, not an error
  const urlMessage = $derived(url.trim() && urlProblem ? urlProblem : null);

  async function commitUrl() {
    if (urlDraft === null) return;
    const next = urlDraft.trim();
    urlDraft = null;
    if (next !== s.webhook.url) await settings.patch({ webhook: { url: next } });
  }

  // ---- test buttons ----
  type Channel = 'native' | 'webhook';
  let test = $state<{ channel: Channel; status: 'sending' | 'ok' | 'error'; message: string } | null>(null);

  async function sendTest(channel: Channel) {
    test = { channel, status: 'sending', message: '' };
    try {
      if (channel === 'webhook') await commitUrl();
      await sendTestNotification(channel);
      test = { channel, status: 'ok', message: '' };
    } catch (e) {
      test = { channel, status: 'error', message: String(e instanceof Error ? e.message : e) };
    }
    if (channel === 'native') void loadPermission();
  }

  // ---- OS permission ----
  let permission = $state<'granted' | 'denied' | 'prompt' | 'unknown'>('granted');
  async function loadPermission() {
    try {
      permission = await getNotificationPermission();
    } catch {
      permission = 'unknown';
    }
  }
  onMount(() => void loadPermission());

  // ---- focus ----
  const focus = $derived(focusStatus(s.focusUntil, Date.now()));
  const focusLine = $derived(
    focus.kind === 'forever'
      ? t('notif.focus.forever')
      : focus.kind === 'until'
        ? t('notif.focus.until', { time: new Date(focus.until).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) })
        : t('notif.focus.off')
  );
</script>

<div class="notif-panel">

  {#if permission === 'denied' || permission === 'prompt'}
    <p class="banner warn" role="status">{t(permission === 'denied' ? 'notif.permission.denied' : 'notif.permission.prompt')}</p>
  {/if}

  <Field label={t('notif.master')} hint={t('notif.master.hint')}>
    <Toggle checked={s.notifications} label={t('notif.master')} onchange={(v) => void settings.patch({ notifications: v })} />
  </Field>

  <Field label={t('notif.threshold')} hint={t('notif.threshold.hint')}>
    <Toggle
      checked={s.thresholdNotifications}
      disabled={!s.notifications}
      label={t('notif.threshold')}
      onchange={(v) => void settings.patch({ thresholdNotifications: v })}
    />
  </Field>

  <Field label={t('notif.forecast')} hint={t('notif.forecast.hint')}>
    <Toggle
      checked={s.forecastNotifications}
      disabled={!s.notifications}
      label={t('notif.forecast')}
      onchange={(v) => void settings.patch({ forecastNotifications: v })}
    />
  </Field>

  <Field label={t('notif.budget')} hint={t('notif.budget.hint')}>
    <Toggle
      checked={s.budgetNotifications}
      disabled={!s.notifications}
      label={t('notif.budget')}
      onchange={(v) => void settings.patch({ budgetNotifications: v })}
    />
  </Field>

  <Field label={t('notif.weekly')} hint={t('notif.weekly.hint')}>
    <Toggle
      checked={s.weeklySummary}
      disabled={!s.notifications}
      label={t('notif.weekly')}
      onchange={(v) => void settings.patch({ weeklySummary: v })}
    />
  </Field>

  <Field label={t('notif.levels')} wide>
    <div class="levels">
      <div class="dual">
        <div class="zones" role="img" aria-label={t('notif.levels.preview')}>
          <span class="zone ok" style:flex-grow={zones.ok}></span>
          <span class="zone warn" style:flex-grow={zones.warn}></span>
          <span class="zone critical" style:flex-grow={zones.critical}></span>
        </div>
        <input
          class="thumb"
          type="range"
          min="1"
          max="100"
          step="1"
          value={levels.warn}
          aria-label={t('notif.levels.warn', { value: levels.warn })}
          oninput={(e) => dragThreshold('warn', e.currentTarget.valueAsNumber)}
          onchange={commitThresholds}
        />
        <input
          class="thumb"
          type="range"
          min="1"
          max="100"
          step="1"
          value={levels.critical}
          aria-label={t('notif.levels.critical', { value: levels.critical })}
          oninput={(e) => dragThreshold('critical', e.currentTarget.valueAsNumber)}
          onchange={commitThresholds}
        />
      </div>
      <div class="legend">
        <span class="swatch warn"></span>{t('notif.levels.warn', { value: levels.warn })}
        <span class="swatch critical"></span>{t('notif.levels.critical', { value: levels.critical })}
      </div>
    </div>
  </Field>

  <Field label={t('notif.webhook.enable')} hint={t('notif.webhook.hint')}>
    <Toggle
      checked={s.webhook.enabled}
      disabled={!s.notifications || !!urlProblem}
      label={t('notif.webhook.enable')}
      onchange={(v) => void settings.patch({ webhook: { enabled: v } })}
    />
  </Field>

  <Field label={t('notif.webhook.url')} wide>
    <input
      class="field url"
      type="url"
      inputmode="url"
      autocomplete="off"
      spellcheck="false"
      placeholder="https://ntfy.sh/…"
      value={url}
      aria-label={t('notif.webhook.url')}
      aria-invalid={urlMessage ? 'true' : undefined}
      oninput={(e) => (urlDraft = e.currentTarget.value)}
      onchange={() => void commitUrl()}
      onblur={() => void commitUrl()}
    />
    {#if urlMessage}<p class="problem" role="alert">{tDyn(`notif.webhook.problem.${urlMessage}`)}</p>{/if}
  </Field>

  <Field label={t('notif.webhook.kind')}>
    <select
      class="field"
      aria-label={t('notif.webhook.kind')}
      value={s.webhook.kind}
      onchange={(e) => void settings.patch({ webhook: { kind: e.currentTarget.value as WebhookKind } })}
    >
      {#each KINDS as kind (kind)}<option value={kind}>{tDyn(`notif.webhook.kind.${kind}`)}</option>{/each}
    </select>
  </Field>

  <div class="tests">
    <button class="btn" disabled={test?.status === 'sending'} onclick={() => void sendTest('native')}>
      {test?.channel === 'native' && test.status === 'sending' ? t('notif.test.sending') : t('notif.test.native')}
    </button>
    <button class="btn" disabled={test?.status === 'sending' || !!urlProblem} onclick={() => void sendTest('webhook')}>
      {test?.channel === 'webhook' && test.status === 'sending' ? t('notif.test.sending') : t('notif.test.webhook')}
    </button>
  </div>
  {#if test?.status === 'ok'}
    <p class="result ok" role="status">{t('notif.test.ok')}</p>
  {:else if test?.status === 'error'}
    <p class="result bad" role="alert">{t('notif.test.failed', { message: test.message })}</p>
  {/if}

  <p class="focus-line" class:on={focus.kind !== 'off'} role="status">{focusLine}</p>
</div>

<style>
  .banner {
    margin: 0.375rem 0;
    padding: 0.4375rem 0.625rem;
    border: 1px solid var(--border-strong);
    border-radius: var(--r-control);
    background: var(--surface-2);
    font-size: 0.75rem;
  }

  .banner.warn {
    border-color: var(--warn);
  }

  /* ---- dual slider ---- */
  .levels {
    display: flex;
    flex-direction: column;
    gap: 0.375rem;
    width: 100%;
  }

  .dual {
    position: relative;
    height: 1.5rem;
  }

  .zones {
    position: absolute;
    inset: 0.5rem 0;
    display: flex;
    border-radius: 999px;
    overflow: hidden;
  }

  .zone {
    flex-basis: 0;
    min-width: 0;
  }

  .zone.ok {
    background: var(--ok);
  }

  .zone.warn {
    background: var(--warn);
  }

  .zone.critical {
    background: var(--critical);
  }

  /* two native ranges laid over each other: the tracks ignore the pointer so
     only the thumbs can be grabbed, and each stays keyboard-operable */
  .thumb {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    appearance: none;
    background: transparent;
    pointer-events: none;
  }

  .thumb::-webkit-slider-runnable-track {
    background: transparent;
  }

  .thumb::-moz-range-track {
    background: transparent;
  }

  .thumb::-webkit-slider-thumb {
    appearance: none;
    pointer-events: auto;
    width: 1rem;
    height: 1.25rem;
    border-radius: 0.375rem;
    border: 2px solid var(--bg);
    background: var(--text);
    cursor: ew-resize;
  }

  .thumb::-moz-range-thumb {
    pointer-events: auto;
    width: 0.75rem;
    height: 1rem;
    border-radius: 0.375rem;
    border: 2px solid var(--bg);
    background: var(--text);
    cursor: ew-resize;
  }

  .thumb:focus-visible {
    outline: none;
  }

  .thumb:focus-visible::-webkit-slider-thumb {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }

  .thumb:focus-visible::-moz-range-thumb {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }

  .legend {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    font-size: 0.75rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .swatch {
    width: 0.625rem;
    height: 0.625rem;
    border-radius: 999px;
  }

  .swatch.warn {
    background: var(--warn);
  }

  .swatch.critical {
    background: var(--critical);
    margin-left: 0.5rem;
  }

  /* ---- webhook ---- */
  .url {
    width: 100%;
    font-family: var(--font-mono);
  }

  .problem {
    margin: 0.25rem 0 0;
    font-size: 0.75rem;
    color: var(--critical);
  }

  .tests {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    padding-top: 0.625rem;
  }

  .result {
    margin: 0.375rem 0 0;
    font-size: 0.75rem;
  }

  .result.ok {
    color: var(--ok);
  }

  .result.bad {
    color: var(--critical);
  }

  .focus-line {
    margin: 0.625rem 0 0;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
    font-size: 0.75rem;
    color: var(--muted);
  }

  .focus-line.on {
    color: var(--warn);
  }
</style>
