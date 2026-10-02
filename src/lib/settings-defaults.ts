// Contract defaults for every setting. [FRONTEND]
//
// Rune-free so unit tests (and the per-card "reset to defaults" helper) can
// import them; the Settings store re-exports everything from here. The Rust
// twin is `impl Default for Settings` in src-tauri/src/model.rs.
import { defaultSidebarItems } from './sidebar-items';
import type { ColorSettings, Settings, SizeSettings } from './types';

/** Contract defaults for the user-tunable palette (empty = keep the theme's). */
export const defaultColors: ColorSettings = {
  claude: '#ff5c1a',
  codex: '#10a37f',
  copilot: '#8250df',
  warn: '#f5c542',
  critical: '#ff3b30',
  surface: '',
  text: '',
};

/** Contract defaults for the user-tunable geometry, in CSS px at scale 1. */
export const defaultSizes: SizeSettings = {
  ringSize: 56,
  ringStroke: 4.5,
  barGap: 18,
  barPadding: 10,
  cornerRadius: 26,
  labelSize: 13,
};

/** Frontend-side fallback so the first paint never has to null-check. */
export const defaultSettings: Settings = {
  version: 1,
  language: 'auto',
  theme: 'dark',
  surfaceStyle: 'glass',
  cyberAccent: 'neon',
  edge: 'right',
  verticalAlign: 'center',
  verticalOffset: 0,
  monitor: null,
  autoHide: false,
  autoHideDelayMs: 800,
  popoverTimeoutSec: 10,
  collapsedWidth: 6,
  ringMode: 'concentric',
  showScopedRing: true,
  percentMode: 'used',
  percentPosition: 'below',
  labelContent: 'percent',
  ringStyle: 'ring',
  sidebarAnimations: true,
  showPercentLabel: true,
  sidebarItems: structuredClone(defaultSidebarItems),
  refreshIntervalSec: 60,
  adaptiveRefresh: true,
  providers: {
    claude: { enabled: true, showInSidebar: true, order: 0 },
    codex: { enabled: true, showInSidebar: true, order: 1 },
    // experimental: off until its credentials are found (providers::enabled_by_default)
    copilot: { enabled: false, showInSidebar: true, order: 2 },
  },
  ingestEnabled: true,
  pricingUrl: '',
  monthlyBudgetUsd: 0,
  subscriptionUsd: { claude: 0, codex: 0 },
  quotaRetentionDays: 365,
  autostart: false,
  autoUpdateCheck: true,
  autoPricingCheck: true,
  shortcutToggleSidebar: '',
  shortcutOpenDashboard: '',
  opacity: 1,
  scale: 1,
  thresholds: { warn: 70, critical: 90 },
  colors: structuredClone(defaultColors),
  sizes: structuredClone(defaultSizes),
  notifications: false,
  forecastNotifications: true,
  focusUntil: 0,
  focusHidesSidebar: false,
  hideAccountEmail: false,
  exportSnapshot: false,
  pollingPaused: false,
  trayDisplay: 'icon',
  alwaysOnTop: true,
  customPresets: {},
};
