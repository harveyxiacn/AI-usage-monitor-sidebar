// Canonical shared types. Mirror of src-tauri/src/model.rs — keep in sync.
// See docs/ARCHITECTURE.md §4.

/**
 * A backend provider id. The two built-ins are spelled out so editors still
 * autocomplete them, but the registry lives in Rust (`get_providers`) and the
 * UI must render any id it is handed — see `$lib/providers`.
 */
export type ProviderId = 'claude' | 'codex' | (string & {});

export type WindowKind = 'five_hour' | 'seven_day' | 'other';

/** How much the burn-rate estimate can be trusted (sample count + time span). */
export type ForecastConfidence = 'low' | 'medium' | 'high';

/** "At this pace" projection for one quota window — see src-tauri/src/forecast.rs. */
export interface QuotaForecast {
  /** used percent expected at the reset; never below the current value, may exceed 100 */
  projectedPercentAtReset: number;
  /** RFC 3339 UTC, set only when 100 % is reached *before* the reset */
  exhaustsAt: string | null;
  /** current burn rate in percentage points per hour (always > 0) */
  ratePercentPerHour: number;
  confidence: ForecastConfidence;
}

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
  /** Burn-rate projection; absent/null when there is not enough usable history */
  forecast?: QuotaForecast | null;
}

/** `rate_limited` is not an error: the last known windows are only getting stale. */
export type ProviderStatus =
  | 'ok'
  | 'not_logged_in'
  | 'token_expired'
  | 'rate_limited'
  | 'error'
  | 'disabled';
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

export type ExtraSeverity = 'info' | 'warn' | 'critical';

/**
 * One provider-neutral fact that is not a rate-limit window (a credit balance,
 * a spend limit that was hit, models the plan cannot use right now, …).
 * `kind` is a stable machine id; the UI renders `extras.<kind>` as the label,
 * so the backend never ships English prose.
 */
export interface QuotaExtra {
  /** credits_balance | reset_credits | spend_limit_reached | model_unavailable | … */
  kind: string;
  /** Already-formatted display value; null when the label alone says it. */
  value: string | null;
  /** Optional secondary text (a model list, the user's own cap, …). */
  detail: string | null;
  severity: ExtraSeverity;
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
  /** Extras beyond the windows; empty when the provider reported none. */
  extras: QuotaExtra[];
  /** Only for `rate_limited`: RFC 3339 UTC of the scheduler's next attempt */
  nextAttemptAt: string | null;
}

export interface AppSnapshot {
  providers: ProviderQuota[];
  generatedAt: string;
}

// ---------- settings ----------

/** left/right dock the bar as a vertical pill, top/bottom as a horizontal strip */
export type Edge = 'left' | 'right' | 'top' | 'bottom';
/**
 * Position *along* the docked edge. The wire values are historical (the bar
 * used to be vertical only): `top` = start (top of a left/right edge, left of
 * a top/bottom edge), `bottom` = end.
 */
export type VerticalAlign = 'top' | 'center' | 'bottom';
/** concentric: one ring group per provider (outer weekly, inner 5-hour, optional 3rd scoped ring); primary: single ring; all: one ring per window */
export type RingMode = 'concentric' | 'primary' | 'all';
export type PercentMode = 'used' | 'remaining';
export type Theme = 'dark' | 'light' | 'auto';
export type Language = 'auto' | 'en' | 'zh-CN';
/** glass: translucent "liquid glass" surface with specular highlights; solid: opaque dark/light pill */
export type SurfaceStyle = 'glass' | 'solid' | 'cyber';
/** Neon pair the cyber surface is painted with; ignored by the other styles. */
export type CyberAccent = 'neon' | 'matrix' | 'amber' | 'ice' | 'synthwave';

export interface ProviderSettings {
  /** off = not polled at all, and gone from the dashboard too */
  enabled: boolean;
  /**
   * off = hidden from the floating bar only. The provider is still polled and
   * still shown in the dashboard, the history and the threshold warnings.
   */
  showInSidebar: boolean;
  order: number;
}

/**
 * What the floating bar may draw. Everything switched off here stays polled,
 * stays in the dashboard and can still raise a warning — see
 * `src/lib/sidebar-items.ts`.
 */
export interface SidebarItems {
  /** account-wide 5-hour windows */
  fiveHour: boolean;
  /** account-wide weekly windows */
  weekly: boolean;
  /** per-model / per-feature windows (any window with a `scope`) */
  scoped: boolean;
  /** account-wide windows that are neither 5-hour nor weekly */
  other: boolean;
  /** provider mark in the middle of a ring group */
  logo: boolean;
  /** percent under a ring group */
  percentLabel: boolean;
  /** the "⋯" button (a grip is still drawn when nothing else is left) */
  moreButton: boolean;
}

export interface Thresholds {
  warn: number;
  critical: number;
}

/** User-tunable colours (CSS hex). Empty string = theme default. */
export interface ColorSettings {
  /** provider accents; the concentric ramp (-1/-2/-3) is derived by lightening */
  claude: string;
  codex: string;
  copilot: string;
  warn: string;
  critical: string;
  /** tint of the glass / solid surface, alpha comes from `opacity` */
  surface: string;
  /** percent label + popover text */
  text: string;
}

/** User-tunable geometry (CSS px at scale 1). */
export interface SizeSettings {
  /** ring group diameter, 40..96 (default 56) */
  ringSize: number;
  /** ring stroke width, 3..8 (default 4.5) */
  ringStroke: number;
  /** vertical gap between ring groups in the bar, 6..40 (default 18) */
  barGap: number;
  /** bar horizontal padding, 4..24 (default 10) */
  barPadding: number;
  /** corner radius of the pill/bubble, 8..40 (default 26) */
  cornerRadius: number;
  /** percent label font size, 9..18 (default 13) */
  labelSize: number;
}

export interface Settings {
  version: number;
  language: Language;
  theme: Theme;
  surfaceStyle: SurfaceStyle;
  /** Only meaningful while `surfaceStyle` is 'cyber'. */
  cyberAccent: CyberAccent;
  edge: Edge;
  /** position along the docked edge, see {@link VerticalAlign} */
  verticalAlign: VerticalAlign;
  /** px offset applied after alignment, positive towards the end of the edge (down on left/right, right on top/bottom) */
  verticalOffset: number;
  /** monitor name, null = primary */
  monitor: string | null;
  autoHide: boolean;
  autoHideDelayMs: number;
  /** close the popover after this many seconds without pointer activity; 0 = never */
  popoverTimeoutSec: number;
  /** width of the visible handle when collapsed (px) */
  collapsedWidth: number;
  ringMode: RingMode;
  /** @deprecated mirror of `sidebarItems.scoped`, kept so old settings files load */
  showScopedRing: boolean;
  percentMode: PercentMode;
  /** @deprecated mirror of `sidebarItems.percentLabel` */
  showPercentLabel: boolean;
  /** what the floating bar may draw; hidden items are still tracked */
  sidebarItems: SidebarItems;
  refreshIntervalSec: number;
  /** Poll a provider less often while its session logs are quiet (10 min → ×2, 30 min → ×5, capped at 10 min) */
  adaptiveRefresh: boolean;
  providers: Record<string, ProviderSettings>;
  ingestEnabled: boolean;
  /** Opt-in https URL of a pricing table; empty = no third-party request is ever made */
  pricingUrl: string;
  /** Monthly *estimated* cost budget in USD; 0 = no budget line. */
  monthlyBudgetUsd: number;
  autostart: boolean;
  /** ask GitHub once a day for a newer release; never installs on its own */
  autoUpdateCheck: boolean;
  /** global shortcut showing/hiding the bar, e.g. "Ctrl+Alt+U"; empty = off */
  shortcutToggleSidebar: string;
  /** global shortcut opening the dashboard; empty = off */
  shortcutOpenDashboard: string;
  /** 0.3 .. 1 */
  opacity: number;
  /** UI scale 0.75 .. 1.5 */
  scale: number;
  thresholds: Thresholds;
  colors: ColorSettings;
  sizes: SizeSettings;
  notifications: boolean;
  /** warn when a window is on pace to run out before it resets */
  forecastNotifications: boolean;
  /** Mask account e-mails everywhere they render (screenshots, screen sharing). */
  hideAccountEmail: boolean;
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
  /** Exact cwd; null/omitted = all projects, empty string = unassigned. */
  project?: string | null;
  groupByProject?: boolean;
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
  /** Sum of priced records; null when no record has a known price. */
  knownCostUsd?: number | null;
  /** Records excluded from knownCostUsd because their price is unknown. */
  unpricedRequests?: number;
}

export interface HistoryRow extends TokenTotals {
  /** RFC 3339, start of bucket (local time) */
  bucketStart: string;
  provider: ProviderId;
  model: string | null;
  /** Raw reasoning effort from session logs; absent on older backends. */
  reasoningEffort?: string | null;
  /** Exact cwd or "" for unassigned; null for aggregation across projects. */
  project: string | null;
}

export interface HistoryResult {
  rows: HistoryRow[];
  totals: TokenTotals;
  /** totals per provider */
  byProvider: Record<string, TokenTotals>;
  /** Projects in the time/provider range, independent of the project filter. */
  projects: string[];
  /** At least one cost came from an approximate family match (§9). */
  costApproximate: boolean;
}

export interface CalendarQuery {
  /** RFC 3339 */
  from: string;
  to: string;
  provider: ProviderId | null;
  /** Same semantics as `HistoryQuery.project`. */
  project?: string | null;
}

/** One local calendar day with activity; days without events are omitted. */
export interface CalendarDay extends TokenTotals {
  /** Local `YYYY-MM-DD` */
  date: string;
}

/** One weekday × hour-of-day cell of the punch card, in local time. */
export interface CalendarSlot extends TokenTotals {
  /** 0 = Monday … 6 = Sunday */
  weekday: number;
  /** 0..23 local hour */
  hour: number;
}

export interface CalendarResult {
  days: CalendarDay[];
  slots: CalendarSlot[];
  totals: TokenTotals;
}

export interface SessionQuery {
  /** RFC 3339 */
  from: string;
  to: string;
  provider: ProviderId | null;
  /** Same semantics as `HistoryQuery.project`. */
  project?: string | null;
  /** Server-side cap on the returned rows (default 200, clamped to 1..1000). */
  limit?: number | null;
}

/** Exact model and effort metadata observed in session logs. */
export interface ModelVariant {
  model: string;
  reasoningEffort: string | null;
}

/** Counters and identifiers only — never prompt or response text. */
export interface SessionRow extends TokenTotals {
  /** Provider session id; "" groups the events that carry none. */
  sessionId: string;
  provider: ProviderId;
  /** Exact cwd of the session's last event in range; "" = unassigned. */
  project: string;
  /** RFC 3339 local, first/last event *inside* the range */
  firstTs: string;
  lastTs: string;
  durationMs: number;
  /** Distinct model names used, sorted */
  models: string[];
  /** Every distinct model/effort combination observed inside this range. */
  modelVariants?: ModelVariant[];
}

export interface SessionsResult {
  /** Top sessions by total tokens, at most `limit` of them. */
  rows: SessionRow[];
  /** Sessions in range before the cap was applied. */
  totalSessions: number;
  /** Totals over every session in range, not only the returned ones. */
  totals: TokenTotals;
  truncated: boolean;
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
  /** quota source implemented from published source, never verified live */
  experimental: boolean;
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

/** What the in-app updater knows right now. Mirror of `model.rs`. */
export interface UpdateStatus {
  /** version offered by the release feed, null when up to date */
  available: string | null;
  currentVersion: string;
  /** release notes of the offered version */
  notes: string | null;
  /** release page, for builds we must not replace in place */
  releaseUrl: string;
  /** false for .deb/.rpm and script installs — link to the release instead */
  canInstall: boolean;
  checking: boolean;
  installing: boolean;
  error: string | null;
  /** RFC 3339 UTC of the last completed check */
  checkedAt: string | null;
}

/** Why a configured global shortcut is not active; null = fine (or disabled). */
export interface ShortcutStatus {
  toggleSidebar: string | null;
  openDashboard: string | null;
}

// ---------- platform / window ----------

export interface PopoverRequest {
  provider: ProviderId;
  /** which ring (index within the sidebar list) */
  ringIndex: number;
  /** y of the ring centre in CSS px relative to the sidebar window (left/right edges) */
  anchorY: number;
  /** x of the ring centre in CSS px relative to the sidebar window (top/bottom edges) */
  anchorX?: number | null;
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
