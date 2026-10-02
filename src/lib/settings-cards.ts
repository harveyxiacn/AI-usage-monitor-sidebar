// Which settings each Settings-tab card owns, and what "reset to defaults"
// means for it. [FRONTEND]
//
// Rune-free so `tests/settings-cards.unit.ts` can exercise it. Defaults are a
// parameter (callers pass `defaultSettings` from `settings-defaults.ts`), so a
// card can never reset to something other than the contract defaults.
import type { SettingsPatch } from './settings-writer';
import type { Settings } from './types';

export type CardId =
  | 'appearance'
  | 'sidebarItems'
  | 'sizeColour'
  | 'position'
  | 'behaviour'
  | 'notifications'
  | 'privacy'
  | 'shortcuts'
  | 'providers'
  | 'accounts'
  | 'data'
  | 'updates'
  | 'integrations';

type Key = keyof Settings;

/**
 * Keys per card. A key appears in exactly one card (tested), so "Reset all" is
 * the union of the cards and nothing is reset twice or forgotten.
 */
export const CARD_KEYS: Record<CardId, readonly Key[]> = {
  appearance: ['theme', 'language', 'scale', 'opacity', 'percentMode', 'percentPosition', 'ringMode', 'surfaceStyle', 'cyberAccent', 'labelContent', 'ringStyle', 'sidebarAnimations'],
  sidebarItems: ['sidebarItems'],
  sizeColour: ['colors', 'sizes'],
  position: ['edge', 'verticalAlign', 'verticalOffset', 'monitor', 'alwaysOnTop'],
  behaviour: ['autoHide', 'autoHideDelayMs', 'popoverTimeoutSec', 'collapsedWidth', 'refreshIntervalSec', 'adaptiveRefresh', 'autostart', 'trayDisplay'],
  notifications: ['notifications', 'forecastNotifications', 'thresholdNotifications', 'budgetNotifications', 'weeklySummary', 'webhook', 'thresholds', 'focusUntil', 'focusHidesSidebar'],
  privacy: ['hideAccountEmail'],
  shortcuts: ['shortcutToggleSidebar', 'shortcutOpenDashboard'],
  providers: ['providers', 'openrouterKeyEnv'],
  accounts: ['accounts'],
  data: ['ingestEnabled', 'monthlyBudgetUsd', 'subscriptionUsd', 'quotaRetentionDays'],
  updates: ['autoUpdateCheck', 'autoPricingCheck', 'pricingUrl', 'skippedVersion'],
  integrations: ['exportSnapshot', 'pollingPaused'],
};

export const CARD_IDS = Object.keys(CARD_KEYS) as CardId[];

/** Keys no card resets: schema version, the user's presets, first-run bookkeeping. */
export const UNRESET_KEYS: readonly Key[] = ['version', 'customPresets', 'lastSeenVersion', 'onboarded'];

/** Patch that sets each of `keys` to its default (nested groups in full). */
export function defaultsPatch(keys: readonly Key[], defaults: Settings): SettingsPatch {
  const patch: Record<string, unknown> = {};
  for (const key of keys) patch[key] = structuredClone(defaults[key]);
  return patch as SettingsPatch;
}

export const resetPatch = (card: CardId, defaults: Settings): SettingsPatch =>
  defaultsPatch(CARD_KEYS[card], defaults);

export const resetAllPatch = (defaults: Settings): SettingsPatch =>
  defaultsPatch(CARD_IDS.flatMap((id) => CARD_KEYS[id]), defaults);

/**
 * True when `current` already holds `expected`. Objects compare by the keys of
 * `expected` only, so a provider this build does not know about (or one added
 * later) cannot make a card look modified.
 */
function holds(expected: unknown, current: unknown): boolean {
  if (Array.isArray(expected)) return Array.isArray(current) && JSON.stringify(expected) === JSON.stringify(current);
  if (expected !== null && typeof expected === 'object' && !Array.isArray(expected)) {
    if (current === null || typeof current !== 'object') return false;
    return Object.entries(expected).every(([k, v]) => holds(v, (current as Record<string, unknown>)[k]));
  }
  return expected === current;
}

/** Does the card differ from its defaults? (Enables its reset button.) */
export function isCardModified(card: CardId, current: Settings, defaults: Settings): boolean {
  return CARD_KEYS[card].some((key) => !holds(defaults[key], current[key]));
}
