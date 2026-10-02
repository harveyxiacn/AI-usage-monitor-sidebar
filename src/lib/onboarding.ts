// Pure logic for the getting-started card and the first-run wizard. [FRONTEND]
import type { ProviderQuota, ProviderSetup } from './types';

export type ProviderHealth =
  /** quota is flowing (or served from cache while rate limited) */
  | 'ok'
  /** the CLI's config directory does not exist: not installed / never run */
  | 'cli-missing'
  /** the CLI is there but has no valid sign-in */
  | 'not-signed-in'
  /** signed in, but the last fetch failed */
  | 'error'
  | 'disabled';

/**
 * Combine the cheap on-disk check with the last snapshot status. The snapshot
 * wins when it says `ok`; the directory check explains the rest.
 */
export function providerHealth(setup: ProviderSetup | undefined, quota: ProviderQuota | undefined): ProviderHealth {
  const status = quota?.status;
  if (status === 'ok' || status === 'rate_limited') return 'ok';
  if (status === 'disabled') return 'disabled';
  if (setup && !setup.configDirFound) return 'cli-missing';
  if (status === 'not_logged_in' || status === 'token_expired') return 'not-signed-in';
  if (setup && !setup.credentialsFound) return 'not-signed-in';
  if (status === 'error') return 'error';
  return 'not-signed-in';
}

/**
 * The getting-started card appears while there is something to set up: the
 * snapshot is in, and no provider delivers quota data.
 */
export function needsGettingStarted(providers: ProviderQuota[] | undefined, loading: boolean): boolean {
  if (loading || !providers || providers.length === 0) return false;
  return !providers.some((p) => p.status === 'ok' || p.status === 'rate_limited');
}

export const WIZARD_STEPS = ['basics', 'edge', 'providers'] as const;
export type WizardStep = (typeof WIZARD_STEPS)[number];

export function nextStep(step: WizardStep): WizardStep | null {
  return WIZARD_STEPS[WIZARD_STEPS.indexOf(step) + 1] ?? null;
}

export function previousStep(step: WizardStep): WizardStep | null {
  return WIZARD_STEPS[WIZARD_STEPS.indexOf(step) - 1] ?? null;
}

/** The wizard is for first runs only, and only until it was finished or skipped. */
export function shouldShowWizard(loaded: boolean, onboarded: boolean): boolean {
  return loaded && !onboarded;
}
