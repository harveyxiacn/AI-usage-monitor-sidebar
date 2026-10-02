import { test, expect } from '@playwright/test';
import { needsGettingStarted, nextStep, previousStep, providerHealth, shouldShowWizard } from '../src/lib/onboarding';
import type { ProviderQuota, ProviderSetup } from '../src/lib/types';

const setup = (over: Partial<ProviderSetup> = {}): ProviderSetup => ({
  provider: 'claude',
  configDir: '~/.claude',
  configDirFound: true,
  credentialsFound: true,
  loginSteps: ['claude', '/login'],
  ...over,
});

const quota = (status: ProviderQuota['status'], provider = 'claude'): ProviderQuota => ({
  provider,
  displayName: provider,
  plan: null,
  planLabel: null,
  account: null,
  windows: [],
  fetchedAt: '2026-09-15T00:00:00Z',
  source: 'api',
  status,
  error: null,
  credits: null,
  extras: [],
  nextAttemptAt: null,
});

test('a flowing provider is ok whatever the disk says', () => {
  expect(providerHealth(setup({ configDirFound: false }), quota('ok'))).toBe('ok');
  expect(providerHealth(setup(), quota('rate_limited'))).toBe('ok');
});

test('a missing config directory means the CLI was never run', () => {
  expect(providerHealth(setup({ configDirFound: false, credentialsFound: false }), quota('not_logged_in'))).toBe('cli-missing');
});

test('a present directory without a sign-in asks for the login', () => {
  expect(providerHealth(setup({ credentialsFound: false }), quota('not_logged_in'))).toBe('not-signed-in');
  expect(providerHealth(setup(), quota('token_expired'))).toBe('not-signed-in');
  expect(providerHealth(setup({ credentialsFound: false }), undefined)).toBe('not-signed-in');
});

test('a signed-in provider whose fetch failed is an error, a disabled one stays disabled', () => {
  expect(providerHealth(setup(), quota('error'))).toBe('error');
  expect(providerHealth(setup(), quota('disabled'))).toBe('disabled');
});

test('the getting-started card shows only while no provider delivers data', () => {
  expect(needsGettingStarted([quota('not_logged_in'), quota('not_logged_in', 'codex')], false)).toBe(true);
  expect(needsGettingStarted([quota('not_logged_in'), quota('ok', 'codex')], false)).toBe(false);
  expect(needsGettingStarted([quota('rate_limited')], false)).toBe(false);
  expect(needsGettingStarted([quota('error')], false)).toBe(true);
  expect(needsGettingStarted([], false)).toBe(false);
  expect(needsGettingStarted(undefined, false)).toBe(false);
  expect(needsGettingStarted([quota('not_logged_in')], true)).toBe(false);
});

test('the wizard walks basics, edge, providers and stops at both ends', () => {
  expect(nextStep('basics')).toBe('edge');
  expect(nextStep('edge')).toBe('providers');
  expect(nextStep('providers')).toBeNull();
  expect(previousStep('providers')).toBe('edge');
  expect(previousStep('basics')).toBeNull();
});

test('the wizard waits for the settings and never reappears once finished', () => {
  expect(shouldShowWizard(false, false)).toBe(false);
  expect(shouldShowWizard(true, false)).toBe(true);
  expect(shouldShowWizard(true, true)).toBe(false);
});
