<script lang="ts">
  import type { AnalysisSettings, EvaluationPreview } from '$lib/session-types';
  import { evaluationReady, previewBudget } from '$lib/sessions';
  import { st } from '$lib/session-labels.svelte';
  import { formatCost, formatTokens } from '$lib/format';
  let { preview, config, sending, ontextchange, onsend, oncancel, onstop }: {
    preview: EvaluationPreview; config: AnalysisSettings; sending: boolean;
    ontextchange: (text: string) => void; onsend: () => void; oncancel: () => void; onstop: () => void;
  } = $props();
  const budget = $derived(previewBudget(preview.text, config));
  const ready = $derived(evaluationReady(config, preview.text));
</script>

<div class="preview">
  <h4>{st('preview')}</h4><p>{st('previewNote')}</p>
  <div class="destination"><strong>{preview.model}</strong><code>{preview.endpoint}</code></div>
  <label class="sr-only" for="assessment-preview">{st('preview')}</label>
  <textarea id="assessment-preview" class="field" value={preview.text} oninput={(event) => ontextchange(event.currentTarget.value)} disabled={sending} spellcheck="false" rows="14"></textarea>
  <div class="budget"><span>{st('chars')}: {preview.text.length.toLocaleString()} / {config.maxInputChars.toLocaleString()}</span><span>{st('approximate')}: ~{formatTokens(budget.inputTokens)}</span><span>{st('maxOutput')}: {preview.maxOutputTokens}</span><span>{st('budget')}: {budget.costUsd === null ? st('unknown') : formatCost(budget.costUsd)}</span></div>
  <p>{st('coverage')}: {preview.coverage}</p>
  <div class="actions"><button class="btn primary" disabled={!ready || sending} onclick={onsend}>{sending ? st('sending') : st('send')}</button>{#if sending}<button class="btn" onclick={onstop}>{st('stop')}</button>{:else}<button class="btn" onclick={oncancel}>{st('cancel')}</button>{/if}</div>
</div>

<style>
  .sr-only { position:absolute; width:1px; height:1px; padding:0; margin:-1px; overflow:hidden; clip:rect(0,0,0,0); white-space:nowrap; border:0; }
  .preview { display: grid; gap: .8rem; padding: 1rem; border: 1px solid var(--focus); border-radius: var(--r-card); min-width: 0; }
  h4,p { margin:0; } h4 { font-size:.85rem; } p { font-size:.8rem; line-height:1.7; color:var(--muted); }
  .destination { display: flex; flex-wrap: wrap; gap: .5rem; font-size: .8rem; overflow-wrap: anywhere; } code { color: var(--muted); }
  textarea { width:100%; min-width:0; box-sizing:border-box; resize:vertical; font:.8rem/1.65 ui-monospace,monospace; white-space:pre-wrap; }
  .budget { display:flex; gap:.4rem 1rem; flex-wrap:wrap; font-size:.7rem; color:var(--muted); }
  .actions { display:flex; flex-wrap:wrap; gap:.5rem; } .primary { color:var(--focus); border-color:var(--focus); }
</style>
