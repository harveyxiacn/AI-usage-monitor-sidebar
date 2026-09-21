// The provider set is decided by the backend, so none of these helpers may
// assume it is exactly claude + codex. See src/lib/providers.ts.
import { test, expect } from '@playwright/test';
import {
  accentVar,
  altVar,
  providerColor,
  providerColorKeys,
  providerDisplayName,
  rampVar,
} from '../src/lib/providers';
import { resolveColor } from '../src/lib/colors';
import { mockSettings } from '../src/lib/mock';

// `rings.svelte.ts` only forwards to rampVar/altVar (it cannot be imported
// here: its store chain pulls in the i18n JSON, which this runner will not
// load). The rendered colours are covered end-to-end in dashboard.e2e.ts.

test('the built-in providers keep exactly the accents they had before', () => {
  // Regression guard for the ramp refactor: the variable that actually
  // resolves must still be the hand-tuned one from theme.css, so the added
  // fallback is inert for claude and codex.
  expect(rampVar('claude', 0)).toBe('var(--accent-claude-1, var(--accent-fallback-1))');
  expect(rampVar('claude', 1)).toBe('var(--accent-claude-2, var(--accent-fallback-2))');
  expect(rampVar('claude', 2)).toBe('var(--accent-claude-3, var(--accent-fallback-3))');
  expect(rampVar('codex', 0)).toBe('var(--accent-codex-1, var(--accent-fallback-1))');
  expect(altVar('claude', 0)).toBe('var(--accent-claude, var(--accent-fallback))');
  expect(altVar('codex', 1)).toBe('var(--accent-codex-alt, var(--accent-fallback-alt))');
});

test('a provider with no hand-tuned tokens falls back to the neutral ramp', () => {
  expect(rampVar('copilot', 0)).toBe('var(--accent-copilot-1, var(--accent-fallback-1))');
  expect(altVar('copilot', 1)).toBe('var(--accent-copilot-alt, var(--accent-fallback-alt))');
  expect(accentVar('copilot')).toBe('var(--accent-copilot, var(--accent-fallback))');
  // depth is clamped: a fourth concentric arc reuses the palest step
  expect(rampVar('copilot', 9)).toBe('var(--accent-copilot-3, var(--accent-fallback-3))');
});

test('provider ids are sanitised before they reach a CSS variable name', () => {
  expect(accentVar('Foo Bar_1')).toBe('var(--accent-foo-bar-1, var(--accent-fallback))');
  // a hostile id cannot escape the variable name and close the var() early
  expect(accentVar('--evil: red; x')).toBe('var(--accent-evil-red-x, var(--accent-fallback))');
  expect(rampVar('a/b*c', 1)).toBe('var(--accent-a-b-c-2, var(--accent-fallback-2))');
  expect(accentVar('')).toBe('var(--accent-unknown, var(--accent-fallback))');
  expect(accentVar('!!!')).toBe('var(--accent-unknown, var(--accent-fallback))');
});

test('an unresolved accent resolves through its fallback instead of reaching a canvas', () => {
  // No document in this runner, so every custom property is "undefined" and
  // resolveColor must walk to the literal default rather than hand chart.js a
  // `var(...)` string it cannot paint.
  expect(resolveColor('var(--accent-copilot-1, var(--accent-fallback-1))', '#123456')).toBe('#123456');
  expect(resolveColor('#ff5c1a')).toBe('#ff5c1a');
});

test('display names fall back to a readable label for an unknown provider', () => {
  expect(providerDisplayName('claude')).toBe('Claude');
  expect(providerDisplayName('codex')).toBe('Codex');
  expect(providerDisplayName('copilot')).toBe('Copilot');
  expect(providerDisplayName('github-copilot')).toBe('Github Copilot');
  expect(providerDisplayName('gemini_cli')).toBe('Gemini Cli');
});

test('the colour UI derives its provider rows from Settings.colors', () => {
  // Adding a provider accent in the backend must give it a picker for free,
  // and must never turn a threshold/surface key into a "provider".
  const colors = mockSettings.colors;
  expect(providerColorKeys(colors)).toEqual(['claude', 'codex', 'copilot']);
  expect(providerColor(colors, 'claude')).toBe('#ff5c1a');
  expect(providerColor(colors, 'copilot')).toBe('#8250df');
  expect(providerColor(colors, 'warn')).toBeNull();
  expect(providerColor(colors, 'surface')).toBeNull();
  expect(providerColor(colors, 'gemini')).toBeNull();
});
