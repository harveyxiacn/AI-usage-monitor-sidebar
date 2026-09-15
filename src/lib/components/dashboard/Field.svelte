<!-- One settings row: label (+ hint) on the left, control on the right. [FRONTEND] -->
<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    label: string;
    hint?: string;
    /** stretch the control to the full row width (sliders, editors) */
    wide?: boolean;
    children: Snippet;
  }

  let { label, hint, wide = false, children }: Props = $props();
</script>

<div class="field" class:wide>
  <div class="text">
    <span class="label">{label}</span>
    {#if hint}<span class="hint">{hint}</span>{/if}
  </div>
  <div class="control">{@render children()}</div>
</div>

<style>
  .field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--border);
  }

  .field:last-child {
    border-bottom: none;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    min-width: 0;
  }

  .label {
    color: var(--text);
  }

  .hint {
    font-size: 0.75rem;
    color: var(--muted);
  }

  .control {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex: none;
  }

  .wide {
    flex-direction: column;
    align-items: stretch;
  }

  .wide .control {
    flex: 1 1 auto;
  }

  @media (max-width: 620px) {
    .field {
      flex-direction: column;
      align-items: stretch;
    }
  }
</style>
