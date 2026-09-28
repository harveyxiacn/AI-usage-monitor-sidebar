<script lang="ts">
  import { onMount } from 'svelte';
  import { evaluateSession, getSessionEvaluations, prepareSessionEvaluation, saveEvaluationReview } from '$lib/api';
  import type { AnalysisSettings, EvaluationPreview, EvaluationReport, SessionSummary } from '$lib/session-types';
  import { evaluationReady, previewBudget } from '$lib/sessions';
  import { st } from '$lib/session-labels.svelte';
  import { formatCost } from '$lib/format';
  import EvaluationPreviewPanel from './EvaluationPreviewPanel.svelte';
  let { session, config, turnIds, sourceUpdatedAt, onsetup, onevidence }: { session: SessionSummary; config: AnalysisSettings; turnIds: string[]; sourceUpdatedAt?: number | null; onsetup: () => void; onevidence: (id: string) => void } = $props();
  let reports = $state<EvaluationReport[]>([]);
  let report = $state<EvaluationReport | null>(null);
  let preview = $state<EvaluationPreview | null>(null);
  let preparing = $state(false), sending = $state(false), reviewing = $state(false), loading = $state(false);
  let error = $state(''), notice = $state('');
  let generation = 0, listGeneration = 0, disposed = false;
  const budget = $derived(previewBudget(preview?.text ?? '', config));
  const ready = $derived(preview !== null && evaluationReady(config, preview.text));
  async function loadReports() {
    const id = ++listGeneration; loading = true; error = '';
    try {
      const next = await getSessionEvaluations(session.provider, session.sessionId);
      if (!disposed && id === listGeneration) { reports = next; report = structuredClone(next.find(r => r.id === report?.id) ?? next[0] ?? null); }
    } catch (e) { if (!disposed && id === listGeneration) error = String(e); }
    finally { if (!disposed && id === listGeneration) loading = false; }
  }
  onMount(() => { void loadReports(); return () => { disposed = true; generation++; listGeneration++; }; });
  async function prepare() {
    if (preparing || sending || reviewing) return;
    const id = ++generation; preparing = true; error = ''; notice = ''; preview = null;
    try { const result = await prepareSessionEvaluation(session.provider, session.sessionId, turnIds); if (!disposed && id === generation) preview = result; }
    catch (e) { if (!disposed && id === generation) error = String(e); }
    finally { if (!disposed && id === generation) preparing = false; }
  }
  async function send() {
    if (!preview || !ready || sending || reviewing || preparing) return;
    const id = ++generation; sending = true; error = ''; notice = '';
    // Freeze the exact reviewed content; subsequent edits cannot alter the request.
    const payload = structuredClone($state.snapshot(preview));
    payload.estimatedInputTokens = budget.inputTokens; payload.estimatedCostUsd = budget.costUsd;
    try { const result = await evaluateSession(payload); if (!disposed && id === generation) { report = structuredClone(result); reports = [result, ...reports.filter(r => r.id !== result.id)]; preview = null; } }
    catch (e) { if (!disposed && id === generation) error = String(e); }
    finally { if (!disposed && id === generation) sending = false; }
  }
  function stop() { generation++; sending = false; notice = st('cancelled'); preview = null; }
  async function review() {
    if (!report || reviewing || sending || preparing) return; reviewing = true; error = '';
    const id = ++generation;
    try { const saved = await saveEvaluationReview(report.id, $state.snapshot(report.analysis.requirements)); if (!disposed && id === generation) { report = structuredClone(saved); reports = reports.map(r => r.id === saved.id ? saved : r); notice = st('saved'); } }
    catch (e) { if (!disposed && id === generation) error = String(e); }
    finally { if (!disposed && id === generation) reviewing = false; }
  }
  function changed(id: string) {
    const edited = report?.analysis.requirements.find(r => r.id === id);
    const original = reports.find(r => r.id === report?.id)?.analysis.requirements.find(r => r.id === id);
    return Boolean(edited && original && (edited.status !== original.status || edited.explanation !== original.explanation));
  }
</script>

<section class="assessment">
  <div class="actions"><button class="btn primary" onclick={() => void prepare()} disabled={!config.contentEnabled || !config.model || preparing || sending || reviewing}>{preparing ? st('loading') : st('prepare')}</button><button class="btn" onclick={onsetup}>{st('settings')}</button><button class="btn" onclick={() => void loadReports()} disabled={loading || reviewing}>{st('refresh')}</button></div>
  <p>{st('selectedTurns')}: {turnIds.length}. {st('allTurns')}</p>
  {#if !config.contentEnabled || !config.model}<p class="callout">{!config.contentEnabled ? st('contentOff') : st('setup')}</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if notice}<p class="callout" role="status">{notice}</p>{/if}
  {#if preview}
    <EvaluationPreviewPanel {preview} {config} {sending} ontextchange={(text) => { if (preview) preview.text = text; }} onsend={() => void send()} oncancel={() => preview = null} onstop={stop} />
  {/if}
  {#if reports.length}
    <label class="report-picker">{st('reports')}<select class="field" value={report?.id ?? ''} disabled={reviewing || sending} onchange={(e) => report = structuredClone($state.snapshot(reports.find(r => r.id === e.currentTarget.value)!))}>{#each reports as r (r.id)}<option value={r.id}>{new Date(r.createdAt).toLocaleString()} · {r.model}</option>{/each}</select></label>
  {/if}
  {#if report}
    <article class="report">
      <header><span class="eyebrow">{report.model} {report.cached ? `· ${st('cached')}` : ''}</span><p class="summary">{report.analysis.summary}</p></header>
      <p class="callout">{st('aiNote')}</p>
      {#if sourceUpdatedAt != null && sourceUpdatedAt !== report.sourceUpdatedAt}<p class="callout">{st('sourceStale')}</p>{/if}
      <div class="prompt-grid">{#each ['strengths', 'gaps', 'suggestions'] as key}<section><h4>{st(key as 'strengths')}</h4><ul>{#each report.analysis.promptAssessment[key as 'strengths'] as item}<li>{item}</li>{/each}</ul></section>{/each}</div>
      <h4>{st('requirements')}</h4>
      <div class="requirements">{#each report.analysis.requirements as requirement (requirement.id)}<section class="requirement"><span class="eyebrow">{st(requirement.confirmedByUser ? 'humanReview' : 'proposed')}</span><strong>{requirement.text}</strong><label class="review-note">{st('reviewNote')}<textarea class="field" rows="3" maxlength="8000" bind:value={requirement.explanation} oninput={() => requirement.confirmedByUser = true}></textarea></label><div class="evidence">{#each requirement.evidenceIds as id (id)}<button class="link" onclick={() => onevidence(id)}>{st('evidence')}: {id}</button>{/each}</div><div class="review"><select class="field" aria-label={requirement.text} value={requirement.status} onchange={(event) => { requirement.status = event.currentTarget.value as typeof requirement.status; requirement.confirmedByUser = true; }}>{#each ['verified', 'partial', 'unmet', 'unknown'] as status}<option value={status}>{st(status as 'verified')}</option>{/each}</select><label><input type="checkbox" bind:checked={requirement.confirmedByUser} disabled={changed(requirement.id)} />{st('confirmed')}</label></div>{#if changed(requirement.id)}<p>{st('reviewChanged')}</p>{/if}</section>{/each}</div>
      <button class="btn" disabled={reviewing || sending || preparing} onclick={() => void review()}>{reviewing ? st('loading') : st('saveReview')}</button>
      <div class="prompt-grid">{#each ['efficiencyNotes', 'limitations'] as key}<section><h4>{st(key === 'efficiencyNotes' ? 'efficiency' : 'limitations')}</h4><ul>{#each report.analysis[key as 'efficiencyNotes'] as item}<li>{item}</li>{/each}</ul></section>{/each}</div>
      <footer>{st('coverage')}: {report.coverage}<br />{st('reportUsage')}: {report.inputTokens ?? '—'} → {report.outputTokens ?? '—'} · {report.estimatedCostUsd === null ? st('unknown') : formatCost(report.estimatedCostUsd)}</footer>
    </article>
  {:else if loading}<p>{st('loading')}</p>{:else}<p class="empty">{st('noReports')}</p>{/if}
</section>

<style>
  .assessment { display: grid; gap: 1rem; min-width: 0; } .actions { display: flex; flex-wrap: wrap; gap: .5rem; } .primary { color: var(--focus); border-color: var(--focus); }
  p, h4, ul { margin: 0; } p, li, footer { font-size: .8rem; line-height: 1.7; color: var(--muted); } h4 { font-size: .85rem; } ul { padding-left: 1.15rem; }
  .callout { background: var(--surface-2); border-left: 2px solid var(--focus); padding: .75rem; } .error { color: var(--danger, #e66); }
  textarea { width: 100%; resize: vertical; font: .8rem/1.65 ui-monospace, monospace; white-space: pre-wrap; }
  .report-picker { display: grid; gap: .5rem; font-size: .8rem; color: var(--muted); } .report-picker select { width: 100%; }
  .report { display: grid; gap: 1rem; } .summary { color: var(--text); font-size: .95rem; margin-top: .5rem; } .eyebrow { color: var(--focus); font: .7rem ui-monospace, monospace; }
  .prompt-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(170px, 1fr)); gap: 1rem; } .prompt-grid h4 { margin-bottom: .5rem; }
  .requirements { display: grid; gap: .75rem; } .requirement { display: grid; gap: .55rem; border: 1px solid var(--border); border-radius: var(--r-control); padding: .85rem; font-size: .85rem; }
  .review-note { display: grid; gap: .4rem; font-size: .7rem; color: var(--muted); } .review-note textarea { font-family: inherit; }
  .review, .evidence { display: flex; gap: .5rem; flex-wrap: wrap; align-items: center; } .review label { display: flex; align-items: center; gap: .4rem; font-size: .75rem; }
  .review select { max-width: 100%; font-size: .75rem; } input { accent-color: var(--focus); } .link { font: .7rem ui-monospace, monospace; color: var(--focus); text-decoration: underline; overflow-wrap: anywhere; text-align: left; }
  .empty { padding: 1.5rem 0; } footer { border-top: 1px solid var(--border); padding-top: .8rem; }
</style>
