import type { SessionRow, TokenTotals } from './types';

export interface SessionListQuery {
  from?: string | null; to?: string | null; provider?: string | null;
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
