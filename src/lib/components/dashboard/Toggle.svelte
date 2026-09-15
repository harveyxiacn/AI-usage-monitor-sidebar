<!-- Accessible on/off switch built on a real checkbox. [FRONTEND] -->
<script lang="ts">
  interface Props {
    checked: boolean;
    label: string;
    disabled?: boolean;
    onchange: (next: boolean) => void;
  }

  let { checked, label, disabled = false, onchange }: Props = $props();
</script>

<label class="switch" class:disabled>
  <input
    type="checkbox"
    {checked}
    {disabled}
    aria-label={label}
    onchange={(e) => onchange(e.currentTarget.checked)}
  />
  <span class="track" aria-hidden="true"><span class="knob"></span></span>
</label>

<style>
  .switch {
    display: inline-flex;
    align-items: center;
    cursor: pointer;
  }

  .switch.disabled {
    opacity: 0.5;
    cursor: default;
  }

  input {
    position: absolute;
    opacity: 0;
    width: 0;
    height: 0;
  }

  .track {
    display: block;
    width: 2.25rem;
    height: 1.25rem;
    padding: 0.125rem;
    border-radius: 999px;
    background: var(--surface-3);
    border: 1px solid var(--border);
    transition: background var(--dur-ui) var(--ease-out);
  }

  .knob {
    display: block;
    width: 0.875rem;
    height: 0.875rem;
    border-radius: 999px;
    background: var(--text);
    transition: transform var(--dur-ui) var(--ease-out), background var(--dur-ui) var(--ease-out);
  }

  input:checked + .track {
    background: var(--focus);
    border-color: transparent;
  }

  input:checked + .track .knob {
    background: #fff;
    transform: translateX(1rem);
  }

  input:focus-visible + .track {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
</style>
