<!--
  Segmented button group (one choice out of a few). [FRONTEND]
  Buttons carry aria-pressed, so the state is not colour-only.
-->
<script lang="ts" generics="T extends string">
  interface Props {
    options: ReadonlyArray<readonly [T, string]>;
    value: T;
    label: string;
    onchange: (value: T) => void;
  }

  let { options, value, label, onchange }: Props = $props();
</script>

<div class="segmented" role="group" aria-label={label}>
  {#each options as [id, text] (id)}
    <button class:active={value === id} aria-pressed={value === id} onclick={() => onchange(id)}>{text}</button>
  {/each}
</div>

<style>
  .segmented {
    display: inline-flex;
    padding: 0.125rem;
    gap: 0.125rem;
    border-radius: var(--r-control);
    background: var(--surface-2);
    border: 1px solid var(--border);
    flex-wrap: wrap;
  }

  button {
    padding: 0.1875rem 0.625rem;
    border-radius: calc(var(--r-control) - 0.125rem);
    font-size: 0.8125rem;
    color: var(--muted);
    white-space: nowrap;
    transition: background var(--dur-ui) var(--ease-out), color var(--dur-ui) var(--ease-out);
  }

  button:hover {
    color: var(--text);
  }

  button.active {
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow-card);
    font-weight: 600;
  }
</style>
