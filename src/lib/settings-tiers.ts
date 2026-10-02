// Basic / advanced tier of every setting. [FRONTEND]
//
// Source of truth for what the Settings tab shows by default: a `basic`
// setting is always visible, an `advanced` one only after "Show advanced
// settings" (or when a search matches it), an `internal` one has no control of
// its own (bookkeeping, or edited through another card). The table is typed
// over `Settings`, so a new setting cannot be added without choosing a tier;
// `scripts/check-agents-settings.mjs` keeps the AGENTS.md table in step, and
// docs/SETTINGS-AUDIT.md explains each decision. Rune-free for unit tests.
import type { Settings, SidebarItems } from './types';

export type Tier = 'basic' | 'advanced' | 'internal';

export const SETTING_TIERS: Record<keyof Settings, Tier> = {
  version: 'internal',
  language: 'basic',
  theme: 'basic',
  surfaceStyle: 'basic',
  cyberAccent: 'basic',
  edge: 'basic',
  verticalAlign: 'advanced',
  verticalOffset: 'advanced',
  monitor: 'basic',
  autoHide: 'basic',
  autoHideDelayMs: 'advanced',
  popoverTimeoutSec: 'advanced',
  collapsedWidth: 'advanced',
  ringMode: 'basic',
  percentMode: 'basic',
  percentPosition: 'advanced',
  labelContent: 'advanced',
  ringStyle: 'advanced',
  sidebarAnimations: 'advanced',
  sidebarItems: 'basic',
  refreshIntervalSec: 'basic',
  adaptiveRefresh: 'advanced',
  providers: 'basic',
  accounts: 'advanced',
  ingestEnabled: 'basic',
  pricingUrl: 'advanced',
  monthlyBudgetUsd: 'basic',
  openrouterKeyEnv: 'advanced',
  subscriptionUsd: 'advanced',
  quotaRetentionDays: 'advanced',
  autostart: 'basic',
  autoUpdateCheck: 'basic',
  autoPricingCheck: 'advanced',
  shortcutToggleSidebar: 'advanced',
  shortcutOpenDashboard: 'advanced',
  opacity: 'advanced',
  scale: 'basic',
  thresholds: 'advanced',
  colors: 'advanced',
  sizes: 'advanced',
  notifications: 'basic',
  forecastNotifications: 'basic',
  advisorNotifications: 'advanced',
  thresholdNotifications: 'basic',
  budgetNotifications: 'advanced',
  weeklySummary: 'advanced',
  webhook: 'advanced',
  focusUntil: 'basic',
  focusHidesSidebar: 'advanced',
  hideAccountEmail: 'basic',
  exportSnapshot: 'advanced',
  gitAttribution: 'advanced',
  pollingPaused: 'advanced',
  trayDisplay: 'advanced',
  alwaysOnTop: 'advanced',
  skippedVersion: 'internal',
  lastSeenVersion: 'internal',
  onboarded: 'internal',
  customPresets: 'internal',
};

export const isAdvanced = (key: keyof Settings): boolean => SETTING_TIERS[key] === 'advanced';

/** `sidebarItems` is one setting; these members of it are advanced, the rest basic. */
export const ADVANCED_SIDEBAR_ITEMS: readonly (keyof SidebarItems)[] = ['other', 'logo', 'moreButton'];

/** localStorage key of the per-viewer "Show advanced settings" switch (UI state, not app behaviour). */
export const ADVANCED_STORAGE_KEY = 'ai-usage-sidebar.showAdvancedSettings';

type Storage = Pick<globalThis.Storage, 'getItem' | 'setItem'>;

/** Stored choice; anything unreadable (private window, blocked storage) means off. */
export function loadShowAdvanced(storage?: Storage): boolean {
  try {
    return (storage ?? window.localStorage).getItem(ADVANCED_STORAGE_KEY) === '1';
  } catch {
    return false;
  }
}

export function saveShowAdvanced(on: boolean, storage?: Storage): void {
  try {
    (storage ?? window.localStorage).setItem(ADVANCED_STORAGE_KEY, on ? '1' : '0');
  } catch {
    /* not persisted; the in-memory choice still holds */
  }
}
