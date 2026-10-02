import type { AnalysisSettings, EvaluationPreview, EvaluationReport, RequirementAssessment, SessionDetail, SessionListQuery, SessionMessage, SessionSummary } from './session-types';
import { buildInsights } from './session-insights';
import { analysisDefaults, previewBudget, sessionIdentity } from './sessions';

let config = structuredClone(analysisDefaults);
const aliases = new Map<string, string>();
const reports = new Map<string, EvaluationReport[]>();
const now = Date.now();
const rows: SessionSummary[] = Array.from({ length: 83 }, (_, i) => ({
  provider: i % 2 ? 'claude' : 'codex', sessionId: `demo-${i + 1}`, project: i % 3 ? '/workspace/sidebar' : '/workspace/api',
  title: ['Improve startup loading', 'Investigate session pagination', 'Add keyboard accessibility', 'Review API error handling'][i % 4], titleSource: 'native',
  firstTs: new Date(now - (i + 1) * 3600000).toISOString(), lastTs: new Date(now - i * 3600000).toISOString(),
  durationMs: 3600000, models: [i % 2 ? 'claude-example' : 'codex-example'], parentSessionId: i === 2 ? 'demo-1' : null,
  inputTokens: 7000 + i * 123, outputTokens: 1600, cacheReadTokens: 3000, cacheWriteTokens: 0, reasoningTokens: 500,
  totalTokens: 11600 + i * 123, requests: 6, estimatedCostUsd: Math.round((0.02 + ((i * 37) % 53) ** 2 / 90) * 100) / 100, userTurns: 1 + (i * 7) % 13, toolCalls: 3 + (i * 5) % 31,
  toolFailures: i % 5 === 0 ? 1 + (i % 4) : 0, repeatedToolCalls: i % 7, activeDurationMs: 60000 + ((i * 29) % 41) ** 2 * 6000, transcriptAvailable: true,
}));
function summary(row: SessionSummary): SessionSummary {
  const alias = aliases.get(sessionIdentity(row.provider, row.sessionId));
  return { ...row, title: alias || row.title, titleSource: alias ? 'alias' : row.titleSource };
}
function messages(sessionId: string): SessionMessage[] {
  return Array.from({ length: 9 }, (_, i) => ({ id: `${sessionId}-m${i + 1}`, turnId: `turn-${Math.floor(i / 3) + 1}`,
    role: (['user', 'tool', 'assistant'] as const)[i % 3], timestamp: now - 3600000 + i * 60000,
    toolName: i % 3 === 1 ? 'run_tests' : null, isError: false, truncated: false,
    text: ['Please improve the session list loading time and preserve the selected filters. Add a regression test.', 'Synthetic test output: all assertions passed.', 'Implemented paginated queries and retained filter state. The synthetic regression check passed.'][i % 3] }));
}
export async function sessionMockInvoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
  const provider = String(args.provider ?? 'codex');
  const sessionId = String(args.sessionId ?? 'demo-1');
  const identity = sessionIdentity(provider, sessionId);
  const row = rows.find(r => r.provider === provider && r.sessionId === sessionId);
  switch (cmd) {
    case 'list_sessions': {
      const q = (args.query ?? {}) as SessionListQuery;
      const offset = q.offset ?? 0; const limit = q.limit ?? 25;
      const filtered = rows.map(summary).filter(r => (!q.provider || r.provider === q.provider) && (!q.project || r.project === q.project)
        && (!q.from || r.lastTs >= q.from) && (!q.to || r.firstTs < q.to)
        && (!q.search || `${r.title} ${r.sessionId} ${r.project}`.toLowerCase().includes(q.search.toLowerCase())));
      filtered.sort((a, b) => q.sort === 'tokens' ? b.totalTokens - a.totalTokens : q.sort === 'title' ? a.title.localeCompare(b.title) : b.lastTs.localeCompare(a.lastTs));
      return { rows: filtered.slice(offset, offset + limit), total: filtered.length, offset, limit };
    }
    case 'get_session_insights': {
      const q = (args.query ?? {}) as SessionListQuery;
      const matching = rows.map(summary).filter(r => (!q.provider || r.provider === q.provider) && (!q.project || r.project === q.project)
        && (!q.from || r.lastTs >= q.from) && (!q.to || r.firstTs < q.to)
        && (!q.search || `${r.title} ${r.sessionId} ${r.project}`.toLowerCase().includes(q.search.toLowerCase())));
      const tools = ['run_tests', 'read_file', 'shell', 'apply_patch'].map((tool, n) => {
        const calls = matching.length * (6 - n); const failures = Math.floor(calls / (8 + n * 3));
        return { tool, calls, failures, failureRate: calls ? failures / calls : 0, sessions: matching.length };
      }).filter(t => t.calls > 0);
      return buildInsights(matching, tools);
    }
    case 'get_session_detail': {
      if (!row) throw new Error('Session not found');
      const all = config.contentEnabled ? messages(sessionId) : [];
      const offset = Number(args.offset ?? 0); const limit = Number(args.limit ?? 40);
      return { session: summary(row), sourceUpdatedAt: Date.parse(row.lastTs), messages: all.slice(offset, offset + limit), totalMessages: all.length,
        nextOffset: offset + limit < all.length ? offset + limit : null, warnings: [],
        children: rows.filter(r => r.parentSessionId === sessionId && r.provider === provider).map(summary),
        turns: [1, 2, 3].map(i => ({ id: `turn-${i}`, startedAt: now - 3600000 + (i - 1) * 180000, finishedAt: now - 3600000 + i * 180000,
          userMessageId: `${sessionId}-m${(i - 1) * 3 + 1}`, totals: null, toolCalls: i === 1 ? 2 : 1, toolFailures: 0, durationMs: 180000 })) } satisfies SessionDetail;
    }
    case 'set_session_alias': aliases.set(identity, String(args.alias ?? '').trim()); return;
    case 'get_analysis_settings': return structuredClone(config);
    case 'save_analysis_settings': config = structuredClone(args.settings as AnalysisSettings); return structuredClone(config);
    case 'get_session_evaluations': return structuredClone(reports.get(identity) ?? []);
    case 'clear_session_analysis': aliases.delete(identity); reports.delete(identity); return;
    case 'prepare_session_evaluation': {
      if (!config.contentEnabled) throw new Error('Enable local content access first');
      if (!config.model.trim()) throw new Error('Configure an evaluation model first');
      const turnIds = (args.turnIds ?? []) as string[];
      const selected = messages(sessionId).filter(m => !turnIds.length || turnIds.includes(m.turnId ?? ''));
      const text = selected.map(m => `[${m.id}] ${m.role}: ${m.text}`).join('\n\n').slice(0, config.maxInputChars);
      const budget = previewBudget(text, config);
      return { provider, sessionId, turnIds, text, messageIds: selected.map(m => m.id), endpoint: config.endpoint, model: config.model,
        estimatedInputTokens: budget.inputTokens, estimatedCostUsd: budget.costUsd, maxOutputTokens: config.maxOutputTokens,
        coverage: 'Synthetic browser preview data only', sourceUpdatedAt: Date.parse(row?.lastTs ?? new Date(now).toISOString()) } satisfies EvaluationPreview;
    }
    case 'evaluate_session': {
      const p = args.preview as EvaluationPreview;
      const key = sessionIdentity(p.provider, p.sessionId);
      const existing = (reports.get(key) ?? []).find(r => r.id === `mock-${p.model}-${p.text}`);
      if (existing) return { ...existing, cached: true };
      const report: EvaluationReport = { id: `mock-${p.model}-${p.text}`, provider: p.provider, sessionId: p.sessionId, createdAt: Date.now(),
        model: p.model, endpoint: p.endpoint, sourceUpdatedAt: p.sourceUpdatedAt, coverage: p.coverage, inputTokens: p.estimatedInputTokens,
        outputTokens: 350, estimatedCostUsd: 0, cached: false, analysis: { summary: 'Synthetic assessment — no network request was made.',
          promptAssessment: { strengths: ['Clear behavior and validation request.'], gaps: ['No numerical performance target.'], suggestions: ['Specify a representative dataset and target latency.'] },
          requirements: [{ id: 'r1', text: 'Preserve filters while loading sessions', status: 'partial', evidenceIds: p.messageIds.slice(0, 1), explanation: 'The request is recorded; independent runtime verification is not present.', confirmedByUser: false }],
          efficiencyNotes: ['Compare paginated query timings on the same dataset.'], limitations: ['Synthetic demonstration; no actual project result was evaluated.'] } };
      reports.set(key, [report, ...(reports.get(key) ?? [])]); return structuredClone(report);
    }
    case 'save_evaluation_review': {
      for (const list of reports.values()) {
        const report = list.find(r => r.id === args.id);
        if (report) { report.analysis.requirements = structuredClone(args.requirements as RequirementAssessment[]); return structuredClone(report); }
      }
      throw new Error('Assessment not found');
    }
    default: throw new Error(`Unsupported session mock command: ${cmd}`);
  }
}
