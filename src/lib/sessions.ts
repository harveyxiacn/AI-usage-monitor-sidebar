import type { AnalysisSettings, SessionMessage } from './session-types';

export const analysisDefaults: AnalysisSettings = {
  contentEnabled: false, endpoint: 'https://api.openai.com/v1/chat/completions', model: '',
  apiKeyEnv: 'OPENAI_API_KEY', maxInputChars: 60000, maxOutputTokens: 3000,
  inputUsdPerMillion: null, outputUsdPerMillion: null,
};
export function sessionIdentity(provider: string, sessionId: string) { return JSON.stringify([provider, sessionId]); }
export function pageBounds(total: number, offset: number, limit: number) {
  return { first: total ? offset + 1 : 0, last: Math.min(total, offset + limit),
    previous: offset > 0 ? Math.max(0, offset - limit) : null, next: offset + limit < total ? offset + limit : null };
}
export function mergeMessagePages(current: SessionMessage[], next: SessionMessage[]): SessionMessage[] {
  return [...new Map([...current, ...next].map(message => [message.id, message])).values()];
}
export function previewBudget(text: string, settings: AnalysisSettings) {
  const inputTokens = Math.ceil(text.length / 4);
  const costUsd = settings.inputUsdPerMillion == null || settings.outputUsdPerMillion == null ? null
    : (inputTokens * settings.inputUsdPerMillion + settings.maxOutputTokens * settings.outputUsdPerMillion) / 1e6;
  return { inputTokens, costUsd };
}
export function evaluationReady(settings: AnalysisSettings, text: string) {
  return settings.contentEnabled && Boolean(settings.model.trim()) && Boolean(text.trim()) && text.length <= settings.maxInputChars;
}
