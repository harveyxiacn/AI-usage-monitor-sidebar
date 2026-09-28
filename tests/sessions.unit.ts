import { test, expect } from '@playwright/test';
import { analysisDefaults, pageBounds, previewBudget, mergeMessagePages, evaluationReady, sessionIdentity } from '../src/lib/sessions';
import type { SessionMessage } from '../src/lib/session-types';

test('session identities remain distinct across providers', () => {
  expect(sessionIdentity('claude', 'same')).not.toBe(sessionIdentity('codex', 'same'));
});
test('pagination bounds preserve empty and partial pages', () => {
  expect(pageBounds(0, 0, 25)).toEqual({ first: 0, last: 0, previous: null, next: null });
  expect(pageBounds(26, 25, 25)).toEqual({ first: 26, last: 26, previous: 0, next: null });
  expect(pageBounds(51, 0, 25).next).toBe(25);
});
test('message paging deduplicates replayed identities without hiding newer contents', () => {
  const message = (id: string, text: string): SessionMessage => ({ id, text, role: 'user', turnId: null, timestamp: null, toolName: null, isError: false, truncated: false });
  expect(mergeMessagePages([message('a', 'old')], [message('a', 'new'), message('b', 'next')]).map(m => m.text)).toEqual(['new', 'next']);
});
test('preview budget tracks edits and includes maximum output cost', () => {
  expect(previewBudget('x'.repeat(100), { ...analysisDefaults, inputUsdPerMillion: 2, outputUsdPerMillion: 8, maxOutputTokens: 1000 })).toEqual({ inputTokens: 25, costUsd: 0.00805 });
  expect(previewBudget('你好', analysisDefaults).costUsd).toBeNull();
});
test('evaluation requires content opt-in, a configured model and nonempty bounded text', () => {
  expect(evaluationReady(analysisDefaults, 'hello')).toBe(false);
  const settings = { ...analysisDefaults, contentEnabled: true, model: 'example', maxInputChars: 5 };
  expect(evaluationReady(settings, 'hello')).toBe(true);
  expect(evaluationReady(settings, '      ')).toBe(false);
  expect(evaluationReady(settings, 'too long')).toBe(false);
});
