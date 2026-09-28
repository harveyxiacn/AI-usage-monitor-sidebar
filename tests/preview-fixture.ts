import { mount } from 'svelte';
import EvaluationPreviewPanel from '../src/lib/components/dashboard/EvaluationPreviewPanel.svelte';
import { analysisDefaults } from '../src/lib/sessions';
import type { EvaluationPreview } from '../src/lib/session-types';

/** Mount the production preview component with synthetic props, without input events. */
export function mountPreview() {
  const target = document.createElement('div');
  target.style.cssText = 'max-width:640px;padding:24px;margin:auto;box-sizing:border-box';
  document.body.replaceChildren(target);
  const preview: EvaluationPreview = { provider: 'codex', sessionId: 'synthetic', turnIds: [],
    text: '[synthetic-m1] user: Please preserve filter state and show clear completion evidence.\n'.repeat(180),
    messageIds: ['synthetic-m1'], endpoint: 'https://example.invalid/v1/chat/completions', model: 'synthetic-only',
    estimatedInputTokens: 3600, maxOutputTokens: 3000, estimatedCostUsd: null, coverage: 'Synthetic preview only; no network request.', sourceUpdatedAt: 1 };
  mount(EvaluationPreviewPanel, { target, props: { preview, config: { ...analysisDefaults, contentEnabled: true, model: 'synthetic-only' }, sending: false,
    ontextchange: () => {}, onsend: () => { throw new Error('Render tests must never send requests'); }, oncancel: () => {}, onstop: () => {} } });
}
