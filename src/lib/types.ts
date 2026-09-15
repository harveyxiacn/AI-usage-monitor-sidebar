// Canonical shared types. Mirror of src-tauri/src/model.rs — keep in sync.
// See docs/ARCHITECTURE.md §4.

export type ProviderId = 'claude' | 'codex';

export type WindowKind = 'five_hour' | 'seven_day' | 'other';

export interface QuotaWindow {
  kind: WindowKind;
  /** Human label, e.g. "5-hour", "Weekly", "Weekly · Fable", "GPT-5.3-Codex-Spark · 5-hour" */
  label: string;
  windowSeconds: number | null;
  /** Used percentage 0..100 */
  usedPercent: number;
  /** RFC 3339 UTC or null */
  resetsAt: string | null;
  /** Model / feature scope, null for the account-wide window */
  scope: string | null;
  /** Exactly one non-scoped window per provider is primary */
  isPrimary: boolean;
}

export type ProviderStatus = 'ok' | 'not_logged_in' | 'token_expired' | 'error' | 'disabled';
export type DataSource = 'api' | 'local_log' | 'cache';

export interface AccountInfo {
  email: string | null;
  name: string | null;
}

export interface CreditsInfo {
  hasCredits: boolean;
  unlimited: boolean;
  balance: string | null;
}

export interface ProviderQuota {
  provider: ProviderId;
  displayName: string;
  /** raw plan id: "max" | "pro" | "plus" | "prolite" | ... */
  plan: string | null;
  /** Human label: "Claude Max 5x", "ChatGPT Plus" */
  planLabel: string | null;
  account: AccountInfo | null;
  windows: QuotaWindow[];
  /** RFC 3339 UTC */
  fetchedAt: string;
  source: DataSource;
  status: ProviderStatus;
  error: string | null;
  credits: CreditsInfo | null;
}

export interface AppSnapshot {
  providers: ProviderQuota[];
  generatedAt: string;
}

// ---------- settings ----------

export type Edge = 'left' | 'right';
export type VerticalAlign = 'top' | 'center' | 'bottom';
export type RingMode = 'primary' | 'all';
export type PercentMode = 'used' | 'remaining';
export type Theme = 'dark' | 'light' | 'auto';
export type Language = 'auto' | 'en' | 'zh-CN';

export interface ProviderSettings {
  enabled: boolean;
  order: number;
}

export interface Thresholds {
  warn: number;
  critical: number;
}

export interface Settings {
  version: number;
  language: Language;
  theme: Theme;
  edge: Edge;
  verticalAlign: VerticalAlign;
  /** px offset applied after alignment (positive moves down) */
  verticalOffset: number;
  /** monitor name, null = primary */
  monitor: string | null;
  autoHide: boolean;
  autoHideDelayMs: number;
  /** width of the visible handle when collapsed (px) */
  collapsedWidth: number;
  ringMode: RingMode;
  percentMode: PercentMode;
  showPercentLabel: boolean;
  refreshIntervalSec: number;
  providers: Record<string, ProviderSettings>;
  ingestEnabled: boolean;
  autostart: boolean;
  /** 0.3 .. 1 */
  opacity: number;
  /** UI scale 0.75 .. 1.5 */
  scale: number;
  thresholds: Thresholds;
  notifications: boolean;
  alwaysOnTop: boolean;
}

// ---------- history ----------

export type Bucket = 'hour' | 'day' | 'week' | 'month';

export interface HistoryQuery {
  /** RFC 3339 */
  from: string;
  to: string;
  bucket: Bucket;
  groupByModel: boolean;
  provider: ProviderId | null;
}

export interface TokenTotals {
  inputTokens: number;
  cacheWriteTokens: number;
  cacheReadTokens: number;
  outputTokens: number;
  reasoningTokens: number;
  totalTokens: number;
  requests: number;
  estimatedCostUsd: number | null;
}

export interface HistoryRow extends TokenTotals {
  /** RFC 3339, start of bucket (local time) */
  bucketStart: string;
  provider: ProviderId;
  model: string | null;
}

export interface HistoryResult {
  rows: HistoryRow[];
  totals: TokenTotals;
  /** totals per provider */
  byProvider: Record<string, TokenTotals>;
}

export interface QuotaHistoryQuery {
  from: string;
  to: string;
  provider: ProviderId | null;
}

export interface QuotaSample {
  provider: ProviderId;
  kind: WindowKind;
  scope: string | null;
  usedPercent: number;
  resetsAt: string | null;
  plan: string | null;
  ts: string;
}

export interface IngestStats {
  filesScanned: number;
  filesUpdated: number;
  eventsAdded: number;
  durationMs: number;
  errors: string[];
  /** true while a scan is running (progress events) */
  running: boolean;
}

export interface ProviderInfo {
  id: ProviderId;
  displayName: string;
  loggedIn: boolean;
  credentialPath: string | null;
  logPath: string | null;
  planLabel: string | null;
}

export interface PricingEntry {
  /** model name prefix, longest match wins */
  modelPattern: string;
  inputPerM: number;
  outputPerM: number;
  cacheWritePerM: number;
  cacheReadPerM: number;
}

export interface PricingTable {
  entries: PricingEntry[];
  updatedAt: string | null;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  configDir: string;
  platform: 'linux' | 'macos' | 'windows' | 'unknown';
  /** "x11" | "wayland" | "cocoa" | "win32" */
  backend: string;
}

// ---------- platform / window ----------

export interface PopoverRequest {
  provider: ProviderId;
  /** which ring (index within the sidebar list) */
  ringIndex: number;
  /** y of the ring centre in CSS px relative to the sidebar window */
  anchorY: number;
  /** optional window to highlight */
  windowKind: WindowKind | null;
}

export interface SidebarState {
  expanded: boolean;
  pinned: boolean;
}

export interface MonitorInfo {
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  scaleFactor: number;
  isPrimary: boolean;
}

export type DashboardTab = 'overview' | 'history' | 'settings';
