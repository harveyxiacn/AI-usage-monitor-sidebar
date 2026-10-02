import type { SessionRow, TokenTotals } from './types';

export interface SessionListQuery {
  from?: string | null; to?: string | null; provider?: string | null;
  /** null/absent = every account; '' = the primary account only; else that extra account */
  account?: string | null;
  project?: string | null; search?: string; sort?: 'recent' | 'tokens' | 'title';
  offset?: number; limit?: number;
}
export interface SessionSummary extends SessionRow {
  title: string;
  titleSource: 'alias' | 'native' | 'prompt' | 'fallback';
  parentSessionId: string | null;
  userTurns: number; toolCalls: number; toolFailures: number; repeatedToolCalls: number;
  activeDurationMs: number | null; transcriptAvailable: boolean;
}
export interface SessionListResult { rows: SessionSummary[]; total: number; offset: number; limit: number }
export interface SessionMessage {
  id: string; turnId: string | null; role: 'user' | 'assistant' | 'tool'; text: string;
  timestamp: number | null; toolName: string | null; isError: boolean; truncated: boolean;
}
export interface SessionTurn {
  id: string; startedAt: number | null; finishedAt: number | null; userMessageId: string | null;
  totals: TokenTotals | null; toolCalls: number; toolFailures: number; durationMs: number | null;
}
export interface SessionDetail {
  session: SessionSummary; messages: SessionMessage[]; totalMessages: number;
  nextOffset: number | null; turns: SessionTurn[]; children: SessionSummary[]; warnings: string[];
  /** Latest indexed transcript modification time, independent of token events. */
  sourceUpdatedAt?: number | null;
}
export interface AnalysisSettings {
  contentEnabled: boolean; endpoint: string; model: string; apiKeyEnv: string;
  maxInputChars: number; maxOutputTokens: number;
  inputUsdPerMillion: number | null; outputUsdPerMillion: number | null;
}
export interface EvaluationPreview {
  provider: string; sessionId: string; turnIds: string[]; text: string; messageIds: string[];
  endpoint: string; model: string; estimatedInputTokens: number; maxOutputTokens: number;
  estimatedCostUsd: number | null; coverage: string; sourceUpdatedAt: number;
}
export interface RequirementAssessment {
  id: string; text: string; status: 'verified' | 'partial' | 'unmet' | 'unknown';
  evidenceIds: string[]; explanation: string; confirmedByUser: boolean;
}
export interface EvaluationAnalysis {
  summary: string;
  promptAssessment: { strengths: string[]; gaps: string[]; suggestions: string[] };
  requirements: RequirementAssessment[]; efficiencyNotes: string[]; limitations: string[];
}
export interface EvaluationReport {
  id: string; provider: string; sessionId: string; createdAt: number; model: string;
  endpoint: string; sourceUpdatedAt: number; coverage: string; analysis: EvaluationAnalysis;
  inputTokens: number | null; outputTokens: number | null; estimatedCostUsd: number | null;
  cached: boolean;
}

/** One session reduced to the metrics the Insights view plots (no content). */
export interface InsightSession {
  provider: string; sessionId: string; title: string; project: string; lastTs: string;
  totalTokens: number; costUsd: number | null; activeDurationMs: number | null;
  userTurns: number; toolCalls: number; toolFailures: number; repeatedToolCalls: number;
  failureRate: number | null; repeatRate: number | null;
  flags: Array<'failures' | 'repeats'>;
}
export interface InsightKpis {
  sessions: number; pricedSessions: number; medianCostUsd: number | null; medianActiveMs: number | null;
  medianTurns: number | null; toolCalls: number; toolFailures: number; repeatedToolCalls: number;
  failureRate: number | null; repeatRate: number | null;
}
export interface HistogramBin { from: number; to: number; count: number }
export interface Histogram { bins: HistogramBin[]; zeroCount: number; sample: number; median: number | null; p90: number | null }
export interface ToolStat { tool: string; calls: number; failures: number; failureRate: number; sessions: number }
export interface InsightThresholds { minFailures: number; failureRate: number; minRepeats: number; repeatRate: number }
export interface SessionInsights {
  totalSessions: number; truncated: boolean; kpis: InsightKpis;
  costHistogram: Histogram; durationHistogram: Histogram; points: InsightSession[];
  topCost: InsightSession[]; topDuration: InsightSession[]; topFailures: InsightSession[]; topRepeats: InsightSession[];
  tools: ToolStat[]; thresholds: InsightThresholds;
}
