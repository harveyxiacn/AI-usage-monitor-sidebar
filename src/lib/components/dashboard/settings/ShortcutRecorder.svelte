<!--
  "Press the keys" recorder for a global shortcut. [FRONTEND]

  Click it, then press the combination: it produces the same accelerator string
  the settings file has always held (`Ctrl+Alt+U`). Esc cancels, Backspace
  clears. The key logic is `$lib/shortcut-recorder` (unit-tested); this
  component only owns the focus handling and the registration badge.
-->
<script lang="ts">
  import { t } from '$lib/i18n/i18n.svelte';
  import { modifiersOf, recordKey } from '$lib/shortcut-recorder';
  import type { ShortcutRegistration } from '$lib/types';

  interface Props {
    value: string;
    label: string;
    registration: ShortcutRegistration;
    onchange: (value: string) => void;
  }

  let { value, label, registration, onchange }: Props = $props();

  let recording = $state(false);
  let held = $state<string[]>([]);
  let complaint = $state<string | null>(null);

  function stop() {
    recording = false;
    held = [];
  }

  function onkeydown(e: KeyboardEvent) {
    if (!recording) return;
    // Everything typed while recording belongs to the recorder, including Tab.
    e.preventDefault();
    e.stopPropagation();
    if (e.repeat) return;
    const result = recordKey(e);
    switch (result.kind) {
      case 'cancel':
        complaint = null;
        stop();
        break;
      case 'clear':
        complaint = null;
        onchange('');
        stop();
        break;
      case 'accelerator':
        complaint = null;
        onchange(result.value);
        stop();
        break;
      case 'pending':
        held = result.modifiers;
        complaint =
          result.reason === 'no-modifier'
            ? t('settings.shortcut.noModifier')
            : result.reason === 'unsupported-key'
              ? t('settings.shortcut.unsupportedKey')
              : null;
        break;
    }
  }

  function onkeyup(e: KeyboardEvent) {
    if (recording) held = modifiersOf(e);
  }

  function begin() {
    complaint = null;
    held = [];
    recording = true;
  }

  const stateText = $derived(t(`settings.shortcut.state.${registration.state}` as 'settings.shortcut.state.off'));
</script>

<span class="recorder">
  <button
    type="button"
    class="field keys"
    class:recording
    aria-label={label}
    onclick={begin}
    {onkeydown}
    {onkeyup}
    onblur={stop}
  >
    {#if recording}
      {held.length > 0 ? `${held.join('+')}+…` : t('settings.shortcut.recording')}
    {:else if value}
      <kbd>{value}</kbd>
    {:else}
      <span class="muted">{t('settings.shortcut.none')}</span>
    {/if}
  </button>
  {#if value && !recording}
    <button type="button" class="btn icon" aria-label={`${label}: ${t('settings.shortcut.clear')}`} onclick={() => onchange('')}>×</button>
  {/if}
  <span class="state {registration.state}" role="status" title={registration.message ?? undefined}>{stateText}</span>
  {#if complaint}<span class="complaint" role="alert">{complaint}</span>{/if}
</span>

<style>
  .recorder {
    display: inline-flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.375rem;
    justify-content: flex-end;
  }

  .keys {
    min-width: 9rem;
    text-align: center;
    cursor: pointer;
  }

  .keys.recording {
    border-color: var(--focus);
    outline: 2px solid color-mix(in srgb, var(--focus) 40%, transparent);
  }

  kbd {
    font: inherit;
    font-variant-numeric: tabular-nums;
  }

  .icon {
    padding: 0.125rem 0.4375rem;
    line-height: 1.2;
  }

  .state {
    font-size: 0.6875rem;
    color: var(--muted);
  }

  .state.registered {
    color: var(--ok, var(--focus));
  }

  .state.failed,
  .state.unsupported {
    color: var(--warn);
  }

  .complaint {
    flex-basis: 100%;
    text-align: right;
    font-size: 0.6875rem;
    color: var(--warn);
  }
</style>
