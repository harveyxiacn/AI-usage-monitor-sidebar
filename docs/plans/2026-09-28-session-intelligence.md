# Session intelligence and performance implementation plan

**Goal:** Deliver all three proposed phases, push the verified result to GitHub main, and publish Windows x64, Linux x64, macOS Apple Silicon and Intel releases.

**Architecture:** Retain Tauri/Svelte/SQLite and the lightweight sidebar. Add a paginated session workspace backed by local metadata and source-message references, with separate opt-in content access and explicitly requested remote evaluations. Incremental ingestion, query caches, lazy modules and reusable charts keep analysis out of the sidebar's critical path.

**Tech Stack:** Rust, rusqlite, reqwest, Svelte 5, TypeScript, Chart.js; no new UI framework. Existing GitHub Actions build the four release targets.

## Product and data contracts

User approved implementation of the proposal and chose local statistics with on-demand AI evaluation. No extra approval is needed for implementation, main integration, tagging or publishing. Do not read agent authentication files, simulate mouse/keyboard input, send private transcripts during development, or rotate updater keys.

### Sessions

- New dashboard Sessions tab; list of title, title source, project, models, last activity, tokens, estimated API-equivalent cost. Real server pagination/filtering/sorting, not a sorted Top-200 subset.
- Title priority: local alias, native title, first genuine user request excerpt (only after content opt-in), project/date fallback. Never label an excerpt as the native title. Index supported Codex session_index.jsonl thread_name and transcript metadata, Claude custom-title/summary records. Unknown log variants degrade to fallback.
- Identity is provider + session ID; missing IDs remain explicitly unassigned. Preserve parent/root/subagent links when present; separate own usage from descendant usage to avoid double counting.
- Incremental metadata indexing stores message locations/roles/times and counters, not full transcript copies. Load bounded message pages on opening a session. Detect expired/replaced source files and show a clear limitation. No arbitrary frontend-supplied filesystem paths.
- Normalize genuine user text separately from tool results, injected environment/context, progress, and assistant messages. Deduplicate streamed/replayed messages and usage records. Preserve turn IDs, tool call/result links, failures and per-turn usage where identifiable. Missing linkage yields unknown, never invented per-turn attribution.
- Metrics: model calls, user turns, cache ratio, per-turn tokens/cost, tool failures, repeated tool calls, observed turn duration, estimated active time using capped gaps. Activity span is not working time or tokens/sec. Parent and children stay independently visible.
- Local content setting defaults off for existing users; title excerpts and transcript display require enabling it. Disabling removes derived content caches; clearing analysis deletes saved assessments/aliases without deleting provider logs or token history.

### Evaluation

- Configuration lives in analysis-settings.json, separate from quota settings: contentEnabled=false, endpoint=https://api.openai.com/v1/chat/completions, model="", apiKeyEnv=OPENAI_API_KEY, maxInputChars=60000, maxOutputTokens=3000, optional input/output USD-per-million rates. Never persist an API key in this file; read the named environment variable only on explicit evaluation.
- User selects a session/turns, prepares an editable redacted preview, and explicitly sends that exact preview. Show endpoint/model, character count, approximate input-token budget and estimated cost when rates are configured. No remote calls during ingestion, navigation or preview.
- Output: prompt strengths/gaps/actionable suggestions; requirement checklist with verified/partial/unmet/unknown, evidence IDs and rationale; efficiency notes and limitations. No universal completion percentage. AI 'verified' means proposed evidence-backed assessment, separate from user confirmation; invalid evidence references are rejected/downgraded.
- Treat conversation text as untrusted evidence, not evaluator instructions. No tools or filesystem execution in evaluations. Bound input/output/time, prevent redirects leaking authorization, sanitize returned errors, allow HTTPS or loopback HTTP for a local compatible service.
- Cache assessments by exact preview + endpoint + model + rubric version. Record time, source coverage, actual evaluator input/output tokens, configured-rate cost, and user confirmations. Changed content/config invalidates reuse. UI can cancel waiting; never silently retry paid calls.
- Tests use synthetic records and a local HTTP test server only. Live API verification requires configured credentials and actual user initiation; a missing key must yield a clear configuration message.

### Wire contract

New shared TS declarations: src/lib/session-types.ts; Rust session types: src-tauri/src/sessions/model.rs. CamelCase JSON.

SessionListQuery = {from?: string|null, to?: string|null, provider?: string|null, project?: string|null, search?: string, sort?: 'recent'|'tokens'|'title', offset?: number, limit?: number}.
SessionSummary extends existing SessionRow with {title: string, titleSource: 'alias'|'native'|'prompt'|'fallback', parentSessionId: string|null, userTurns: number, toolCalls: number, toolFailures: number, repeatedToolCalls: number, activeDurationMs: number|null, transcriptAvailable: boolean}.
SessionListResult = {rows: SessionSummary[], total: number, offset: number, limit: number}.
SessionMessage = {id: string, turnId: string|null, role: 'user'|'assistant'|'tool', text: string, timestamp: number|null, toolName: string|null, isError: boolean, truncated: boolean}.
SessionTurn = {id: string, startedAt: number|null, finishedAt: number|null, userMessageId: string|null, totals: TokenTotals|null, toolCalls: number, toolFailures: number, durationMs: number|null}.
SessionDetail = {session: SessionSummary, messages: SessionMessage[], totalMessages: number, nextOffset: number|null, turns: SessionTurn[], children: SessionSummary[], warnings: string[]}.

Commands (all explicitly registered):
- list_sessions(query: SessionListQuery) -> SessionListResult
- get_session_detail(provider, sessionId, offset?: number, limit?: number) -> SessionDetail
- set_session_alias(provider, sessionId, alias: string) -> ()
- get_analysis_settings() -> AnalysisSettings
- save_analysis_settings(settings: AnalysisSettings) -> AnalysisSettings
- prepare_session_evaluation(provider, sessionId, turnIds?: string[]) -> EvaluationPreview
- evaluate_session(preview: EvaluationPreview) -> EvaluationReport (explicit action only)
- get_session_evaluations(provider, sessionId) -> EvaluationReport[]
- save_evaluation_review(id, requirements: RequirementAssessment[]) -> EvaluationReport
- clear_session_analysis(provider, sessionId) -> ()

Root defines evaluation TypeScript/Rust contract before frontend integration. Existing token-history APIs remain compatible.

## Work packages and verification

1. **Session backend:** add sessions model/parser/store/commands; extend ingestion integration and contract docs. First add synthetic Claude/Codex tests for title precedence, prompts vs tool returns, resumed streams, malformed/partial lines, missing/expired sources, parent linkage and turn usage. Observe missing behavior, implement, run focused Rust tests. Pagination tests must include >200 sessions with a low-token recent session.
2. **Ingestion and query performance:** persist versioned Codex checkpoints (context, cumulative usage, mode and offset), stream bounded reads, reset safely on rewrite/truncation; dirty-file queue plus periodic reconciliation. Cache history/calendar/sessions by data generation and pricing, bound caches and preserve local-time/DST and repricing correctness. Add append/restart/rewrite/mixed-mode regression tests and synthetic benchmark commands/results.
3. **Session UI and frontend performance:** implement Sessions tab in the existing themes with localized labels and responsive list/detail layout. Add content opt-in, alias editor, pagination/filter state, turn selection, evaluation preview/settings/report/review. Lazy-load mock/tabs, reuse charts, stable row keys, paginate long history tables, coalesce refresh and pause hidden-window fallback polling. Verify pure UI logic tests, typecheck, production build and read-only browser inspection (no mouse/keyboard simulation).
4. **Evaluation backend:** test redaction, payload validation, prompt-injection isolation, evidence validation, missing-key behavior, localhost mock success/error/timeout/cache and review persistence. Implement bounded explicit HTTP call, settings, preparation, cache, result history and user confirmation. Keep credentials out of logs/IPC/storage.
5. **Integration/review:** verify fixture coverage for old/new log variants and schema upgrades; no text stored before opt-in; unknown stays unknown; parent aggregation has no double counting. Run independent spec and quality review, fix findings. Update README, ARCHITECTURE, validation guide, release notes and privacy documentation.
6. **Release:** bump all four version files to next unoccupied minor version (planned 0.5.0). Run frontend check/unit/build, cargo fmt/clippy/test and relevant no-input smoke/benchmark checks. Commit feature branch, fast-forward main after fetching remote, push main; inspect CI. Tag only validated commit, wait for all four release jobs, inspect assets and updater manifest/signatures, publish draft and verify published URLs/version. Browser validation uses navigation and assertions without input simulation, as required by AGENTS.md. Existing interactive browser scenarios remain available through an explicit manual workflow option; default CI runs the no-input render suite, unit tests, backend checks and native startup smoke test.

## Completion evidence checklist

- [x] Local session titles, aliases and genuine prompts with backwards compatible parsing.
- [x] Server pagination/search/sort; per-turn metrics and parent/child display.
- [x] Opt-in content, bounded reads, expired-source handling, analysis clearing.
- [x] Explicit AI preview/send, configuration, evidence-based report, caching and user review.
- [x] Frontend and backend performance changes measured on repeatable synthetic data.
- [x] Tests/build/typecheck/lint and independent review complete (266 Rust tests, 86 frontend unit tests, 6 no-input render tests, 2 separate benchmarks; Clippy/fmt/typecheck/build passed).
- [ ] main pushed and exact commit verified on remote.
- [ ] Linux x64, Windows x64, macOS arm64/x64 published assets verified; updater manifest checked.

## Sources checked

- OpenAI Codex app-server official documentation (thread/read, names, userMessage/commandExecution/fileChange): https://learn.chatgpt.com/docs/app-server
- Claude transcript and session naming documentation: https://code.claude.com/docs/en/claude-directory and https://code.claude.com/docs/en/commands
- Chat Completions request contract: https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create
- Chart.js update lifecycle: https://www.chartjs.org/docs/latest/developers/updates.html
