<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { clearSessionAnalysis, getAnalysisSettings, getSessionDetail, listSessions, onIngestProgress, saveAnalysisSettings, setSessionAlias, type Unlisten } from '$lib/api';
  import { formatDuration, formatEstimatedCost, formatTokens } from '$lib/format';
  import { localDateInput, modelVariantLabel, projectName, sessionModelVariants } from '$lib/history';
  import { providerDisplayName } from '$lib/providers';
  import { st } from '$lib/session-labels.svelte';
  import type { AnalysisSettings as Config, SessionDetail, SessionListQuery, SessionListResult, SessionSummary } from '$lib/session-types';
  import { analysisDefaults, mergeMessagePages, pageBounds, sessionIdentity } from '$lib/sessions';
  import { filtersFromParams } from '$lib/session-insights';
  import { sessionViewState } from '$lib/session-ui-state';
  import { VIEW_REQUEST_EVENT, type ViewRequest } from '$lib/palette-nav';
  import AnalysisSettings from './AnalysisSettings.svelte';
  import SessionEvaluation from './SessionEvaluation.svelte';
  import Segmented from './history/Segmented.svelte';
  import InsightsView from './sessions/InsightsView.svelte';

  const initial = { ...sessionViewState.query };
  let query = $state<SessionListQuery>({ ...initial });
  let search = $state(initial.search ?? ''), provider = $state(initial.provider ?? ''), project = $state(initial.project ?? '');
  let from = $state(initial.from ? localDateInput(Date.parse(initial.from)) : ''), to = $state(initial.to ? localDateInput(Date.parse(initial.to) - 1) : '');
  let sort = $state<NonNullable<SessionListQuery['sort']>>(initial.sort ?? 'recent');
  let result = $state<SessionListResult | null>(null), detail = $state<SessionDetail | null>(null);
  let config = $state<Config>({ ...analysisDefaults });
  let selected = $state<{ provider: string; sessionId: string } | null>(sessionViewState.selected);
  let listLoading = $state(true), detailLoading = $state(false), moreLoading = $state(false), changing = $state(false);
  let error = $state(''), detailError = $state(''), notice = $state('');
  let configOpen = $state(false), confirmClear = $state(false), alias = $state('');
  let pane = $state<'messages' | 'metrics' | 'evaluation'>('messages');
  let view = $state<'browse' | 'insights'>(sessionViewState.view), fromInsights = $state(false);
  $effect(() => { sessionViewState.view = view; });
  const viewOptions = $derived([['browse', st('browse')], ['insights', st('insights')]] as const);
  let evaluationOpened = $state(false);
  let turnIds = $state<string[]>([]), highlighted = $state('');
  let listVersion = 0, detailVersion = 0, metadataVersion = 0, disposed = false;
  const bounds = $derived(pageBounds(result?.total ?? 0, query.offset ?? 0, query.limit ?? 25));
  const identity = $derived(selected ? sessionIdentity(selected.provider, selected.sessionId) : '');
  const session = $derived(detail?.session ?? null);
  const modelNames = (row: SessionSummary) => sessionModelVariants(row)
    .map(variant => modelVariantLabel(variant.model, variant.reasoningEffort, st('unknown'))).join(', ') || st('unknown');
  $effect(() => { if (pane === 'evaluation' && identity) evaluationOpened = true; });
  const cacheShare = $derived(session && session.inputTokens + session.cacheReadTokens + session.cacheWriteTokens > 0
    ? `${Math.round(session.cacheReadTokens / (session.inputTokens + session.cacheReadTokens + session.cacheWriteTokens) * 100)}%` : '—');

  async function loadList() {
    const version = ++listVersion; listLoading = true; error = '';
    sessionViewState.query = { ...$state.snapshot(query) };
    try {
      const next = await listSessions($state.snapshot(query));
      if (!disposed && version === listVersion) {
        if (next.total > 0 && (query.offset ?? 0) >= next.total) { query.offset = Math.floor((next.total - 1) / (query.limit ?? 25)) * (query.limit ?? 25); return void loadList(); }
        result = next;
        void refreshDetailMetadata();
      }
    } catch (e) { if (!disposed && version === listVersion) error = String(e); }
    finally { if (!disposed && version === listVersion) listLoading = false; }
  }
  async function refreshDetailMetadata() {
    if (!selected || !detail || detailLoading) return;
    const target = { ...selected }; const version = detailVersion; const request = ++metadataVersion;
    try {
      const next = await getSessionDetail(target.provider, target.sessionId, 0, 1);
      if (!disposed && request === metadataVersion && version === detailVersion && detail) {
        // Preserve message scroll, alias draft, turn selection and review drafts.
        detail = { ...detail, session: next.session, turns: next.turns, children: next.children,
          warnings: next.warnings, sourceUpdatedAt: next.sourceUpdatedAt, totalMessages: next.totalMessages,
          nextOffset: detail.nextOffset ?? (detail.messages.length < next.totalMessages ? detail.messages.length : null) };
      }
    } catch (e) { if (!disposed && request === metadataVersion && version === detailVersion) detailError = String(e); }
  }
  async function openSession(providerId: string, sessionId: string) {
    const version = ++detailVersion;
    metadataVersion++; evaluationOpened = pane === 'evaluation';
    selected = { provider: providerId, sessionId }; sessionViewState.selected = { ...selected };
    detail = null; detailLoading = true; moreLoading = false; detailError = ''; notice = ''; turnIds = []; highlighted = ''; confirmClear = false;
    try {
      const next = await getSessionDetail(providerId, sessionId);
      if (!disposed && version === detailVersion) { detail = next; alias = next.session.titleSource === 'alias' ? next.session.title : ''; }
    } catch (e) { if (!disposed && version === detailVersion) detailError = String(e); }
    finally { if (!disposed && version === detailVersion) detailLoading = false; }
  }
  async function more() {
    if (!selected || detail?.nextOffset == null || moreLoading) return;
    const version = detailVersion; const target = { ...selected }; const offset = detail.nextOffset; moreLoading = true; detailError = '';
    try { const next = await getSessionDetail(target.provider, target.sessionId, offset); if (!disposed && version === detailVersion && detail) detail = { ...next, messages: mergeMessagePages(detail.messages, next.messages) }; }
    catch (e) { if (!disposed && version === detailVersion) detailError = String(e); }
    finally { if (!disposed && version === detailVersion) moreLoading = false; }
  }
  function apply(event?: SubmitEvent) {
    event?.preventDefault();
    commitFilters(); result = null; void loadList();
  }
  function commitFilters() {
    const end = to ? new Date(`${to}T00:00:00`) : null;
    if (end) end.setDate(end.getDate() + 1);
    query = { search: search.trim(), provider: provider || null, project: project.trim() || null, sort, offset: 0, limit: 25,
      from: from ? new Date(`${from}T00:00:00`).toISOString() : null, to: end?.toISOString() ?? null };
  }
  /** Insights rows open the same detail pane the list uses. */
  function openFromInsights(providerId: string, sessionId: string) {
    fromInsights = true; view = 'browse'; void openSession(providerId, sessionId);
  }
  function page(offset: number) { query.offset = offset; void loadList(); }
  async function settingsSaved(next: Config) {
    config = next; configOpen = false; detail = null; detailVersion++; notice = st('saved');
    await loadList(); if (selected) await openSession(selected.provider, selected.sessionId);
  }
  async function enableContent() {
    if (changing) return; changing = true; detailError = '';
    try { await settingsSaved(await saveAnalysisSettings({ ...$state.snapshot(config), contentEnabled: true })); }
    catch (e) { detailError = String(e); } finally { changing = false; }
  }
  async function saveAlias() {
    if (!selected || changing) return; changing = true; const target = { ...selected }; const version = detailVersion;
    try { await setSessionAlias(target.provider, target.sessionId, alias); await loadList(); if (version === detailVersion) await openSession(target.provider, target.sessionId); }
    catch (e) { if (version === detailVersion) detailError = String(e); } finally { changing = false; }
  }
  async function clear() {
    if (!selected || changing) return; changing = true; const target = { ...selected }; const version = detailVersion;
    try { await clearSessionAnalysis(target.provider, target.sessionId); if (version === detailVersion) await openSession(target.provider, target.sessionId); await loadList(); }
    catch (e) { if (version === detailVersion) detailError = String(e); } finally { changing = false; }
  }
  async function evidence(id: string) {
    const version = detailVersion;
    pane = 'messages'; highlighted = id; notice = '';
    // Evidence IDs are opaque text. Never interpolate them into CSS selectors.
    while (version === detailVersion && detail && !detail.messages.some(m => m.id === id) && detail.nextOffset !== null && !moreLoading) {
      const before = detail.nextOffset; await more(); if (detail?.nextOffset === before || detailError) break;
    }
    if (version !== detailVersion || disposed) return;
    await tick();
    if (version !== detailVersion || disposed) return;
    const element = document.getElementById(`session-message-${id}`);
    if (element) element.scrollIntoView({ block: 'nearest', behavior: 'instant' }); else notice = st('evidenceMissing');
  }
  onMount(() => {
    // the command palette asks an already mounted tab to switch sub-view
    const onViewRequest = (e: Event) => { const r = (e as CustomEvent<ViewRequest>).detail; if (r.tab === 'sessions') { view = r.view; fromInsights = false; } };
    window.addEventListener(VIEW_REQUEST_EVENT, onViewRequest);
    const params = new URLSearchParams(window.location.search);
    const linkedSession = params.get('session');
    const linkedProvider = params.get('provider');
    if (linkedSession && (linkedProvider === 'claude' || linkedProvider === 'codex')) selected = { provider: linkedProvider, sessionId: linkedSession };
    const linkedFilters = filtersFromParams(params);
    if (linkedFilters) {
      provider = linkedFilters.provider ?? ''; project = linkedFilters.project ?? '';
      from = linkedFilters.from ? localDateInput(Date.parse(linkedFilters.from)) : ''; to = linkedFilters.to ? localDateInput(Date.parse(linkedFilters.to) - 1) : '';
      commitFilters();
    }
    if (params.get('view') === 'insights') view = 'insights'; else if (params.get('view') === 'browse') view = 'browse';
    const linkedPane = params.get('pane');
    if (linkedPane === 'metrics' || linkedPane === 'evaluation') pane = linkedPane;
    void loadList();
    void getAnalysisSettings().then(next => { if (!disposed) config = next; }).catch(e => { if (!disposed) error = String(e); });
    if (selected) void openSession(selected.provider, selected.sessionId);
    let unlisten: Unlisten | null = null; let timer: ReturnType<typeof setTimeout> | undefined; let dirty = false;
    const refresh = () => { if (document.hidden) { dirty = true; return; } dirty = false; clearTimeout(timer); timer = setTimeout(() => void loadList(), 300); };
    const resume = () => { if (!document.hidden && dirty) refresh(); };
    // A title/prompt-only change need not add token events.
    void onIngestProgress(stats => { if (!stats.running) refresh(); }).then(un => disposed ? un() : unlisten = un).catch(() => {});
    document.addEventListener('visibilitychange', resume);
    return () => { disposed = true; listVersion++; detailVersion++; clearTimeout(timer); unlisten?.(); document.removeEventListener('visibilitychange', resume); window.removeEventListener(VIEW_REQUEST_EVENT, onViewRequest); };
  });
</script>

<section class="sessions">
  <header class="workspace-head"><div><span class="eyebrow">{st('workspace')}</span><h2>{st('title')}</h2><p>{st('subtitle')}</p></div><div class="head-actions"><Segmented options={viewOptions} value={view} label={st('viewLabel')} onchange={(v) => { view = v; fromInsights = false; }} /><button class="btn" onclick={() => configOpen = !configOpen}>{st('settings')}</button></div></header>
  {#if configOpen}<AnalysisSettings value={config} onsave={(next) => void settingsSaved(next)} onclose={() => configOpen = false} />{/if}
  <form class="filters card" onsubmit={apply}>
    <input class="field search" aria-label={st('search')} placeholder={st('search')} bind:value={search} />
    <select class="field" aria-label={st('all')} bind:value={provider}><option value="">{st('all')}</option><option value="claude">{providerDisplayName('claude')}</option><option value="codex">{providerDisplayName('codex')}</option></select>
    <select class="field" aria-label={st('recent')} bind:value={sort} disabled={view === 'insights'}><option value="recent">{st('recent')}</option><option value="tokens">{st('tokens')}</option><option value="title">{st('name')}</option></select>
    <input class="field project-filter" aria-label={st('project')} placeholder={st('project')} bind:value={project} />
    <label>{st('from')}<input class="field" type="date" bind:value={from} max={to || undefined} /></label><label>{st('to')}<input class="field" type="date" bind:value={to} min={from || undefined} /></label>
    <button class="btn" type="submit">{st('apply')}</button><button class="btn" type="button" onclick={() => { search = ''; provider = ''; project = ''; from = ''; to = ''; sort = 'recent'; apply(); }}>{st('reset')}</button>
  </form>
  {#if error}<p class="error" role="alert">{error} <button class="btn" onclick={() => void loadList()}>{st('retry')}</button></p>{/if}
  {#if view === 'insights'}
    <InsightsView {query} onopen={openFromInsights} />
  {:else}
  {#if fromInsights}<button class="link back" onclick={() => { view = 'insights'; fromInsights = false; }}>← {st('backToList')}</button>{/if}
  <div class="workspace">
    <aside class="card list-panel" aria-label={st('title')} aria-busy={listLoading}>
      <div class="list-heading"><span>{bounds.first}–{bounds.last} / {result?.total ?? 0}</span><button class="link" onclick={() => void loadList()} disabled={listLoading}>{listLoading ? st('loading') : st('refresh')}</button></div>
      {#if result?.rows.length}<div class="session-list">{#each result.rows as row (sessionIdentity(row.provider, row.sessionId))}{@const models = modelNames(row)}<button class="session-row" class:active={identity === sessionIdentity(row.provider, row.sessionId)} aria-pressed={identity === sessionIdentity(row.provider, row.sessionId)} onclick={() => void openSession(row.provider, row.sessionId)}><span class="row-meta"><span>{providerDisplayName(row.provider)}</span><span>{new Date(row.lastTs).toLocaleDateString()}</span></span><strong>{row.title}</strong><span class="row-project" title={row.project}>{projectName(row.project)} · {st(row.titleSource)}</span><span class="row-models" title={models}>{st('models')}: {models}</span><span class="row-stats"><b>{formatTokens(row.totalTokens)}</b> {st('tokens')} <span>{formatEstimatedCost(row)}</span></span></button>{/each}</div>{:else}<p class="empty">{listLoading ? st('loading') : st('empty')}</p>{/if}
      <nav class="pager" aria-label={st('title')}><button class="btn" disabled={bounds.previous === null || listLoading} onclick={() => page(bounds.previous!)}>{st('previous')}</button><button class="btn" disabled={bounds.next === null || listLoading} onclick={() => page(bounds.next!)}>{st('next')}</button></nav>
    </aside>
    <div class="card detail" aria-busy={detailLoading}>
      {#if detailError}<p class="error" role="alert">{detailError}{#if selected}<button class="btn" onclick={() => void openSession(selected!.provider, selected!.sessionId)}>{st('retry')}</button>{/if}</p>{/if}
      {#if detailLoading}<p class="empty">{st('loading')}</p>{:else if session && detail}
        <header class="detail-head"><span class="eyebrow">{providerDisplayName(session.provider)} · {st(session.titleSource)}</span><h3>{session.title}</h3><p title={session.project}>{session.project || '—'}</p><p>{st('models')}: {modelNames(session)}</p><code>{st('sessionId')}: {session.sessionId || '—'}</code></header>
        <form class="alias-form" onsubmit={(e) => { e.preventDefault(); void saveAlias(); }}><input class="field" aria-label={st('alias')} placeholder={st('alias')} maxlength="200" bind:value={alias} /><button class="btn" type="submit" disabled={changing}>{st('save')}</button></form>
        <div class="metrics"><div><span>{st('tokens')}</span><strong>{formatTokens(session.totalTokens)}</strong></div><div><span>{st('estimatedCost')}</span><strong>{formatEstimatedCost(session)}</strong></div><div><span>{st('turns')}</span><strong>{session.userTurns}</strong></div><div><span>{st('cache')}</span><strong>{cacheShare}</strong></div></div>
        <p class="muted small">{st('ownUsage')}</p>
        {#if session.parentSessionId}<p class="relation">{st('parent')}: <button class="link" onclick={() => void openSession(session.provider, session.parentSessionId!)}>{session.parentSessionId}</button></p>{/if}
        {#if detail.children.length}<div class="relations"><span>{st('children')}</span>{#each detail.children as child (sessionIdentity(child.provider, child.sessionId))}<button class="link" onclick={() => void openSession(child.provider, child.sessionId)}>{child.title} · {formatTokens(child.totalTokens)}</button>{/each}</div>{/if}
        {#each detail.warnings as warning}<p class="callout">{warning}</p>{/each}
        {#if notice}<p class="callout" role="status">{notice}</p>{/if}
        <nav class="detail-tabs" aria-label={st('title')}>{#each ['messages', 'metrics', 'evaluation'] as id}<button class:active={pane === id} aria-pressed={pane === id} onclick={() => pane = id as typeof pane}>{st(id as 'messages')}</button>{/each}</nav>
        {#if pane === 'messages'}
          {#if !config.contentEnabled}<div class="empty"><p>{st('contentOff')}</p><button class="btn" disabled={changing} onclick={() => void enableContent()}>{st('enable')}</button></div>{:else if !session.transcriptAvailable}<p class="empty">{st('contentUnavailable')}</p>{:else}<div class="messages">{#each detail.messages as message (message.id)}<article id={`session-message-${message.id}`} class="message" class:highlighted={highlighted === message.id} class:tool={message.role === 'tool'}><header><strong>{st(message.role)}</strong><span>{message.toolName ?? ''}</span><time>{message.timestamp ? new Date(message.timestamp).toLocaleString() : '—'}</time></header><code>{message.id}</code><pre>{message.text}</pre>{#if message.isError}<span class="error">{st('failures')}</span>{/if}{#if message.truncated}<span class="muted small">{st('truncated')}</span>{/if}</article>{/each}</div>{#if detail.nextOffset !== null}<button class="btn" disabled={moreLoading} onclick={() => void more()}>{moreLoading ? st('loading') : st('more')}</button>{/if}<p class="muted small">{detail.messages.length} / {detail.totalMessages}</p>{/if}
        {:else if pane === 'metrics'}
          <div class="metrics"><div><span>{st('calls')}</span><strong>{session.requests}</strong></div><div><span>{st('tools')}</span><strong>{session.toolCalls}</strong></div><div><span>{st('failures')}</span><strong>{session.toolFailures}</strong></div><div><span>{st('repeated')}</span><strong>{session.repeatedToolCalls}</strong></div><div><span>{st('active')}</span><strong>{session.activeDurationMs === null ? st('unknown') : formatDuration(session.activeDurationMs)}</strong></div><div><span>{st('span')}</span><strong>{formatDuration(session.durationMs)}</strong></div></div><p class="muted small">{st('durationNote')}</p>
          <p class="muted small">{st('selectedTurns')}: {turnIds.length}. {st('allTurns')}</p>
          {#if !detail.turns.length}<p class="empty">{st('noTurns')}</p>{/if}
          <div class="turns">{#each detail.turns as turn (turn.id)}<label class="turn"><input type="checkbox" value={turn.id} bind:group={turnIds} /><span><strong>{turn.id}</strong><small>{turn.startedAt ? new Date(turn.startedAt).toLocaleString() : '—'}</small></span><span>{turn.totals ? formatTokens(turn.totals.totalTokens) : '—'} tokens <small>{turn.totals ? formatEstimatedCost(turn.totals) : st('unknown')}</small></span><span>{turn.durationMs === null ? '—' : formatDuration(turn.durationMs)}<small>{st('tools')}: {turn.toolCalls} · {st('failures')}: {turn.toolFailures}</small></span></label>{/each}</div>
        {/if}
        {#if evaluationOpened}<div hidden={pane !== 'evaluation'}>{#key identity}<SessionEvaluation {session} {config} {turnIds} sourceUpdatedAt={detail.sourceUpdatedAt} onsetup={() => configOpen = true} onevidence={(id) => void evidence(id)} />{/key}</div>{/if}
        <footer class="clear"><button class="link" onclick={() => confirmClear = !confirmClear}>{st('clear')}</button>{#if confirmClear}<p>{st('clearNote')}</p><button class="btn" disabled={changing} onclick={() => void clear()}>{st('confirmClear')}</button>{/if}</footer>
      {:else}<div class="empty selection"><span class="selection-icon" aria-hidden="true">↗</span><p>{st('select')}</p></div>{/if}
    </div>
  </div>
  {/if}
</section>

<style>
  .sessions { display: grid; gap: 1rem; } .head-actions { display:flex; align-items:center; gap:.6rem; flex-wrap:wrap; } .back { justify-self:start; } .workspace-head { display:flex; justify-content:space-between; align-items:center; gap:1rem; }
  h2,h3,p { margin:0; } h2 { font-size:1.55rem; letter-spacing:-.035em; margin:.3rem 0; } .workspace-head p { color:var(--muted); font-size:.85rem; }
  .eyebrow { font: .66rem ui-monospace,monospace; letter-spacing:.08em; color:var(--focus); } .filters { display:flex; flex-wrap:wrap; gap:.55rem; padding:.8rem; align-items:center; }
  .filters .search { flex:1 1 260px; } .filters label { display:flex; align-items:center; gap:.35rem; color:var(--muted); font-size:.75rem; } .project-filter { flex:1 1 180px; }
  .field { min-width:0; } .workspace { display:grid; grid-template-columns: minmax(235px, .75fr) minmax(0, 1.8fr); gap:1rem; align-items:start; }
  .list-panel { position:sticky; top:0; overflow:hidden; } .list-heading { display:flex; justify-content:space-between; padding:.9rem; color:var(--muted); font-size:.75rem; border-bottom:1px solid var(--border); }
  .session-list { max-height:70vh; overflow:auto; } .session-row { width:100%; text-align:left; display:grid; gap:.5rem; padding:1rem; border-bottom:1px solid var(--border); border-left:3px solid transparent; }
  .session-row:hover { background:var(--hover); } .session-row.active { border-left-color:var(--focus); background:var(--surface-2); }
  .session-row strong { font-size:.86rem; line-height:1.4; overflow-wrap:anywhere; } .row-meta,.row-stats { display:flex; justify-content:space-between; align-items:center; gap:.35rem; font-size:.7rem; color:var(--muted); }
  .row-stats { justify-content:flex-start; font-variant-numeric:tabular-nums; } .row-stats b { color:var(--text); font-size:.85rem; } .row-stats span { margin-left:auto; } .row-project,.row-models { min-width:0; font-size:.7rem; color:var(--muted); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .pager { display:flex; justify-content:space-between; gap:.5rem; padding:.75rem; } .detail { padding:1.25rem; display:grid; gap:1rem; min-width:0; }
  .detail-head { display:grid; gap:.5rem; } h3 { font-size:1.35rem; line-height:1.35; overflow-wrap:anywhere; letter-spacing:-.02em; } .detail-head p,.detail-head code { color:var(--muted); font-size:.72rem; overflow-wrap:anywhere; }
  .alias-form { display:flex; gap:.5rem; } .alias-form input { flex:1; } .metrics { display:grid; grid-template-columns:repeat(4,minmax(0,1fr)); gap:.75rem; }
  .metrics div { display:grid; gap:.4rem; } .metrics span { color:var(--muted); font-size:.68rem; line-height:1.5; } .metrics strong { font-size:1.3rem; letter-spacing:-.03em; font-variant-numeric:tabular-nums; }
  .small { font-size:.72rem; line-height:1.6; } .muted { color:var(--muted); } .error { color:var(--danger,#e66); font-size:.8rem; overflow-wrap:anywhere; }
  .link { color:var(--focus); text-decoration:underline; text-align:left; font-size:.75rem; overflow-wrap:anywhere; } .relations { display:grid; gap:.4rem; } .relations>span,.relation { font-size:.75rem; color:var(--muted); }
  .detail-tabs { display:flex; gap:.3rem; border-bottom:1px solid var(--border); } .detail-tabs button { padding:.6rem .7rem; font-size:.8rem; color:var(--muted); border-bottom:2px solid transparent; } .detail-tabs button.active { color:var(--text); border-bottom-color:var(--focus); }
  .messages { display:grid; gap:.8rem; } .message { border:1px solid var(--border); border-radius:var(--r-control); padding:.85rem; display:grid; gap:.6rem; scroll-margin:1rem; }
  .message.highlighted { outline:2px solid var(--focus); } .message.tool { background:var(--surface-2); } .message header { display:flex; flex-wrap:wrap; gap:.5rem; align-items:center; font-size:.72rem; }.message time { margin-left:auto; color:var(--muted); font-size:.65rem; }
  .message code { color:var(--muted); font-size:.6rem; overflow-wrap:anywhere; } pre { margin:0; white-space:pre-wrap; overflow-wrap:anywhere; font-family:inherit; font-size:.8rem; line-height:1.7; max-height:24rem; overflow:auto; }
  .turns { display:grid; gap:.5rem; } .turn { display:grid; grid-template-columns:auto 1fr 1fr 1fr; align-items:center; gap:.75rem; border-bottom:1px solid var(--border); padding:.75rem 0; font-size:.75rem; } .turn small { display:block; color:var(--muted); font-size:.65rem; margin-top:.3rem; } input[type=checkbox] { accent-color:var(--focus); }
  .callout { padding:.75rem; border-left:2px solid var(--focus); background:var(--surface-2); font-size:.8rem; line-height:1.6; } .empty { padding:2rem 1rem; text-align:center; color:var(--muted); font-size:.85rem; line-height:1.7; }.empty .btn { margin-top:1rem; }.selection { min-height:23rem; display:grid; align-content:center; gap:1rem; }.selection-icon { font-size:3rem; color:var(--border); }
  .clear { border-top:1px solid var(--border); padding-top:1rem; display:grid; gap:.7rem; }.clear p { color:var(--muted); font-size:.75rem; }.clear .btn { justify-self:start; }
  @media(max-width:1050px) { .metrics { grid-template-columns:repeat(2,minmax(0,1fr)); }.detail { padding:1rem; }.workspace { grid-template-columns:minmax(215px,.7fr) minmax(0,1.4fr); } }
  @media(max-width:730px) { .workspace { grid-template-columns:1fr; }.list-panel { position:static; }.session-list { max-height:20rem; }.turn { grid-template-columns:auto 1fr 1fr; }.turn>span:last-child { grid-column:2/-1; }.workspace-head { align-items:flex-start; } }
</style>
