# AI Usage Sidebar — Architecture & Contracts

This document is the **single source of truth** for module boundaries, data
types, Tauri commands, events and window labels. Several people/agents work on
the code base in parallel; every one of them must respect the contracts below.
If you need to change a contract, change it *here first* and keep the change
minimal and backwards compatible.

## 1. Product summary

A tiny always-on-top **edge sidebar widget** (see `docs/reference/*.jpg`) that
shows one progress ring per AI-coding-agent provider (Claude Code, OpenAI
Codex, …). Hovering / clicking a ring opens a **popover** with the detailed
rate-limit windows (5-hour, weekly, per-model, …) and their reset time. A
separate **dashboard** window holds settings and the **token usage history**
(parsed from the providers' local session logs) so users can compare plans and
providers over time.

Targets: Linux first (developed on CachyOS / GNOME Wayland), then macOS,
other Linux distros, Windows. Stack: **Tauri 2 (Rust)** + **SvelteKit / Svelte 5
/ TypeScript** (adapter-static, SPA mode) + **SQLite (rusqlite, bundled)**.

## 2. Providers — verified facts (2026-09)

### Claude (Claude Code OAuth)
* Credentials: `~/.claude/.credentials.json` (Linux/Windows). Override dir with
  `CLAUDE_CONFIG_DIR`. On macOS the same JSON lives in the Keychain, item
  service name `Claude Code-credentials` (`security find-generic-password -s
  "Claude Code-credentials" -w`). Shape:
  `{"claudeAiOauth":{"accessToken","refreshToken","expiresAt"(ms),"scopes":[],
  "subscriptionType":"max"|"pro"|…,"rateLimitTier":"default_claude_max_5x"}}`
* Usage endpoint: `GET https://api.anthropic.com/api/oauth/usage` with headers
  `Authorization: Bearer <accessToken>`, `anthropic-beta: oauth-2025-04-20`.
  Response (verified):
  ```json
  {"five_hour":{"utilization":3.0,"resets_at":"2026-09-15T06:19:59.7+00:00"},
   "seven_day":{"utilization":31.0,"resets_at":"2026-09-18T19:59:59.7+00:00"},
   "seven_day_opus":null,"seven_day_sonnet":null,
   "limits":[{"kind":"session","group":"session","percent":3,"severity":"normal","resets_at":"…","scope":null,"is_active":false},
             {"kind":"weekly_all","group":"weekly","percent":31,"resets_at":"…","scope":null},
             {"kind":"weekly_scoped","group":"weekly","percent":24,"resets_at":"…","scope":{"model":{"id":null,"display_name":"Fable"}}}],
   "extra_usage":{"is_enabled":false,…},"spend":{…},
   "seven_day_breakdown":{"rows":[{"key":"claude_code","display_name":"Claude Code","percent":100},…]}}
  ```
  Prefer the generic `limits[]` array (kind `session` → five_hour, `weekly_all`
  → seven_day, `weekly_scoped` → seven_day with `scope`), fall back to the
  legacy top-level keys. `utilization`/`percent` are already 0‑100.
* Profile endpoint: `GET https://api.anthropic.com/api/oauth/profile` (same
  headers) → `account.{display_name,email,has_claude_max,has_claude_pro}`,
  `organization.{organization_type,rate_limit_tier}`.
* All Claude plans (Pro, Max 5x, Max 20x) have a 5‑hour *and* a 7‑day window.
* Session logs (token usage): `~/.claude/projects/**/*.jsonl`. Lines with
  `"type":"assistant"` carry `message.model`, `message.id`, `requestId`,
  `timestamp` (ISO), `sessionId`, `cwd`, and
  `message.usage.{input_tokens,cache_creation_input_tokens,cache_read_input_tokens,output_tokens,output_tokens_details.thinking_tokens?}`.
  Streaming writes several lines for the same `message.id`; **dedupe on
  `message.id` + `requestId`** (keep the last/most complete usage). Skip
  `model == "<synthetic>"`.

### Codex (OpenAI Codex CLI, ChatGPT login)
* Credentials: `~/.codex/auth.json` (override dir with `CODEX_HOME`). Shape:
  `{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token","access_token","refresh_token","account_id"},"last_refresh":"…Z"}`.
  `access_token` is a JWT; `exp` claim gives expiry; claim
  `https://api.openai.com/auth`.`chatgpt_plan_type` gives the plan
  (`free|plus|pro|prolite|team|business|enterprise|edu|…`). If `auth_mode` is
  `apikey` there are no quota windows to show (status `not_logged_in` with a
  hint).
* Usage endpoint: `GET https://chatgpt.com/backend-api/wham/usage` with
  headers `Authorization: Bearer <access_token>`, `ChatGPT-Account-Id:
  <account_id>`. Response (verified):
  ```json
  {"plan_type":"prolite","email":"…",
   "rate_limit":{"allowed":true,"limit_reached":false,
     "primary_window":{"used_percent":75,"limit_window_seconds":604800,"reset_after_seconds":370836,"reset_at":1789807722},
     "secondary_window":null},
   "additional_rate_limits":[{"limit_name":"GPT-5.3-Codex-Spark","metered_feature":"codex_bengalfox",
     "rate_limit":{"primary_window":{"used_percent":0,"limit_window_seconds":18000,…},"secondary_window":{"used_percent":0,"limit_window_seconds":604800,…}}}],
   "credits":{"has_credits":false,"unlimited":false,"balance":"0"},
   "rate_limit_reached_type":null}
  ```
  **Do not assume primary = 5h / secondary = 7d.** Plus has a 5‑hour window
  (primary, 18000 s) + weekly (secondary, 604800 s); Pro/prolite only has the
  weekly window (it is then the *primary* window and secondary is `null`).
  Classify by `limit_window_seconds`: ≤ 6 h → `five_hour`, ~7 d → `seven_day`,
  else `other`. `additional_rate_limits[]` become extra windows with
  `scope = limit_name`.
* Fallback when the token is expired / offline: the newest
  `~/.codex/sessions/**/*.jsonl` line of `type:"event_msg"` with
  `payload.type:"token_count"` carries `payload.rate_limits` =
  `{"primary":{"used_percent","window_minutes","resets_at"(unix s)},"secondary":null|{…},"plan_type":"prolite","credits":{…}}`.
* Session logs (token usage): `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`
  (also `~/.codex/archived_sessions/`). Prefer lines `type:"token_usage_record"`
  with `payload.{thread_id,turn_id,response_id,usage:{input_tokens,cached_input_tokens,cache_write_input_tokens,output_tokens,reasoning_output_tokens,total_tokens}}`
  (**dedupe on `response_id`**). Older CLI versions only have
  `event_msg/token_count` with `info.last_token_usage` / `info.total_token_usage`
  (cumulative) — derive per-turn deltas from `total_token_usage` in file order.
  Model name: `turn_context.payload.model` (latest one before the record), else
  `session_meta.payload.model`, else `"unknown"`. `session_meta.payload` also
  has `cwd`, `cli_version`, `originator`, `source` (sub-agent marker).
  Note Codex `input_tokens` **includes** `cached_input_tokens`; Claude
  `input_tokens` **excludes** cache tokens. Normalise: store
  `input_tokens` = non-cached input for both providers.

### GitHub Copilot (experimental, unverified — 2026-09)

Added from published source only: **nothing below was checked against a live
Copilot account**, which is why `ProviderInfo.experimental` is `true` for it,
the settings row carries an "Experimental" badge, and the provider starts
switched off unless its credentials are already on disk
(`providers::enabled_by_default`). Full citations: **docs/PROVIDERS.md §1**.

* Credentials: the OAuth token the Copilot editor plugins write to
  `<config>/github-copilot/apps.json` (older `hosts.json`), where `<config>` is
  `$XDG_CONFIG_HOME`, `%LOCALAPPDATA%` on Windows, else `~/.config` (also on
  macOS — *not* `~/Library/Application Support`). The file is an object keyed
  by host, `"github.com"` or `"github.com:<appId>"`, each value carrying
  `oauth_token`. Only github.com keys are used: an Enterprise entry's token
  must never be sent to api.github.com. The GitHub CLI's own token is
  deliberately **not** used as a fallback.
* Usage endpoint: `GET https://api.github.com/copilot_internal/user` with
  `Authorization: token <oauth_token>` (the `token` scheme, **not** `Bearer`),
  `Accept: application/json`, `Editor-Version`, `Editor-Plugin-Version`,
  `User-Agent`, `X-Github-Api-Version: 2025-04-01`. Response (from source):
  ```json
  {"copilot_plan":"pro","access_type_sku":"…","quota_reset_date":"2099-01-15T00:00:00Z",
   "token_based_billing":false,
   "quota_snapshots":{"premium_interactions":{"entitlement":300,"remaining":123,
       "percent_remaining":41,"unlimited":false,"overage_permitted":false,"overage_count":0,
       "quota_id":"premium","quota_remaining":123},
     "chat":{…},"completions":{…}}}
  ```
* Buckets report percent **remaining**; `usedPercent = 100 - percent_remaining`.
  Suppress a bucket that is `unlimited`, that uses the `-1` sentinel, or whose
  `entitlement` is `0` (an org-managed placeholder, or `premium_interactions`
  on a free seat) — a suppressed bucket is not "0 % used". Older free-tier
  responses predate `quota_snapshots` and carry `limited_user_quotas`
  (remaining) against `monthly_quotas` (total), read only when
  `quota_snapshots` produced nothing.
* The quota period is a **calendar month**: every window is `kind: "other"`
  with `windowSeconds: null`. Copilot has no 5-hour or weekly window.
* `quota_reset_date` (or `limited_user_reset_date`) is sometimes a bare date
  (`"2099-07-01"`), sometimes an ISO-8601 datetime; both are accepted.
* No Copilot client is known to write local session logs with token counts, so
  there is no ingestion and no history for this provider (`logPath: null`).

Researched and **rejected**: Google Gemini CLI (its credentials moved into the
OS keychain, with an AES-256-GCM-obfuscated file as the fallback) and Cursor
(ToS §1.5(viii), Enterprise-only official API, bot-protected endpoints). Both
are written up with sources in **docs/PROVIDERS.md** §2 and §3 so the question
is not re-opened without new information.

## 3. Repository layout & ownership

```
src/                      SvelteKit frontend                     [FRONTEND]
  routes/+layout.ts       ssr=false, prerender=true
  routes/+page.svelte     window "sidebar"  (route "/")
  routes/popover/         window "popover"  (route "/popover")
  routes/dashboard/       window "dashboard"(route "/dashboard")
  lib/types.ts            canonical TS types (mirror of model.rs)  [SHARED - edit only via contract]
  lib/api.ts              invoke() wrapper + browser mock         [FRONTEND owns, keep command names]
  lib/i18n/               en.json, zh-CN.json, t()
  lib/components/         Ring.svelte, Popover.svelte, ...
src-tauri/
  src/lib.rs, main.rs     builder wiring, plugins, handler list   [PLATFORM]
  src/window/             sidebar/popover/dashboard window logic, tray, autostart, monitors  [PLATFORM]
  src/model.rs            canonical Rust types (serde camelCase)  [SHARED - edit only via contract]
  src/commands.rs         backend commands (thin wrappers)        [BACKEND]
  src/state.rs            AppState (settings, snapshot cache, db) [BACKEND]
  src/settings.rs         load/save settings.json                 [BACKEND]
  src/providers/          claude.rs, codex.rs, copilot.rs, mod.rs (trait, registry) [BACKEND]
  lib/providers.ts        provider-agnostic frontend helpers      [FRONTEND]
  src/ingest/             jsonl parsers, incremental ingestion     [BACKEND]
  src/store/              sqlite schema + queries                  [BACKEND]
  src/pricing.rs          default pricing table + cost estimation [BACKEND]
  src/scheduler.rs        periodic refresh + ingest, emits events  [BACKEND]
  src/updater.rs          in-app update check, never auto-installs [PLATFORM]
  tauri.conf.json, capabilities/, icons/                          [PLATFORM]
docs/                     this file, PLATFORM.md, reference images
.github/workflows/        CI builds for linux/macos/windows        [PLATFORM]
```

Rules for parallel work:
* Only touch files you own. If you must touch a shared file, make the
  smallest possible additive change.
* `Cargo.toml` already lists every dependency that is expected to be needed.
  Add a dependency only if truly required, with `cargo add`, and mention it in
  your final report.
* The backend agent runs cargo with `CARGO_TARGET_DIR=src-tauri/target-backend`
  (already git-ignored) so it does not fight the platform agent's
  `pnpm tauri dev` build for the lock on `src-tauri/target`.
* Never print or commit real tokens. Redact when logging.

## 4. Canonical types

TypeScript: `src/lib/types.ts`. Rust: `src-tauri/src/model.rs` (serde
`rename_all = "camelCase"` for structs, `snake_case` for enums). They are
field-for-field mirrors; keep them in sync.

Key semantics:
* `QuotaWindow.usedPercent` is **used** percent, 0‑100 (clamp). The UI derives
  "remaining" when the user prefers it.
* `QuotaWindow.resetsAt` is RFC 3339 UTC or `null`.
* `QuotaWindow.forecast` is optional (absent/`null` = no forecast) and is
  filled when the snapshot is built, from the `quota_samples` history of that
  exact provider/kind/scope since the window's last reset — see
  `src-tauri/src/forecast.rs`. `projectedPercentAtReset` is clamped at the
  current value and may exceed 100; `exhaustsAt` is set only when 100 % is
  reached *before* `resetsAt`; `ratePercentPerHour` is always > 0;
  `confidence` (`low|medium|high`) comes from the sample count and the share of
  the estimation horizon they cover. Too few samples, too short or stale a
  history, a flat/negative slope, an already-full window or a projection within
  one percentage point of the current value all mean "no forecast". The UI only
  marks the ring for `medium`/`high`.
* `QuotaWindow.isPrimary`: exactly one window per provider is primary (the
  5‑hour window when the plan has one, else the weekly window).
  `ringMode = "concentric"` (default): one ring *group* per provider — outer
  ring = non-scoped weekly window, inner ring = 5-hour window (omitted when the
  plan has none, e.g. Codex Pro), optional innermost third ring = first scoped
  window (e.g. Claude "Weekly · Fable") when `sidebarItems.scoped` is on; Codex
  never gets a third ring (its scoped windows are additional per-feature
  limits). Rings of one group share the provider hue in decreasing intensity;
  threshold colours override per ring. The percent label shows the primary
  window. `ringMode = "primary"`: one plain ring per provider (primary window).
  `ringMode = "all"`: one ring per non-scoped window.
* `ProviderQuota.status`: `ok | not_logged_in | token_expired | rate_limited |
  error | disabled`. Any status other than `ok` still returns the last known
  windows (from cache or local logs) if available, with `source` telling where
  they came from. `rate_limited` (`HTTP 429`) is **not** an error: the numbers
  are merely ageing, the UI presents them as stale, and
  `ProviderQuota.nextAttemptAt` (RFC 3339 UTC, else null) says when the
  scheduler will try again.
* `ProviderQuota.extras` is an optional, provider-neutral list of facts that
  are not rate-limit windows (credits, a spend limit that was hit, models the
  plan cannot use right now). Each item is
  `{kind, value: string|null, detail: string|null, severity: info|warn|critical}`.
  `kind` is a stable machine id (`reset_credits`, `spend_limit_reached`,
  `model_unavailable`, `rate_limit_reached`, `overage_limit_reached`,
  `approx_local_messages`, `approx_cloud_messages`, `extra_usage`, …); the
  frontend renders `extras.<kind>` as the label and falls back to the raw id,
  so a new kind needs no frontend change. The backend never puts English prose
  in `value`/`detail`, and never repeats what `credits` already says. Absent in
  older cache files, so it deserializes as an empty list.
* Times/timestamps in the DB are unix **milliseconds** (INTEGER).
* `HistoryQuery.project` is optional: null/absent selects all projects, an
  empty string selects events whose `cwd` is null or empty, and any other
  string matches the original working-directory path exactly. Paths are
  not trimmed, canonicalized or case-folded across platforms.
* `HistoryQuery.groupByProject` defaults to false and combines with
  `groupByModel`. `HistoryRow.project` is the original path (or an empty
  string for unassigned events) when grouped or filtered by project; null
  means an unfiltered aggregate across projects.
* `HistoryResult.projects` lists distinct project paths within the selected
  time/provider range, ignoring the current project filter. It includes an
  empty string when unassigned events exist. UI labels may shorten paths,
  but selection, series identity and CSV preserve the complete value.
* `CalendarResult.days` has one entry per **local** calendar day that has
  activity; days without events are omitted, so the UI can tell an empty day
  (inside the window, zero usage) from a missing one (outside it).
  `CalendarResult.slots` folds the same range into weekday × hour-of-day cells
  with Monday = 0 and local hours, for the punch-card view.
* A session is `(provider, sessionId)`; `SessionRow.sessionId` is an empty
  string for events that carry none, mirroring the unassigned-project rule.
  `firstTs`/`lastTs`/`durationMs` only cover events **inside** the query range,
  and `project` is the cwd of the session's latest event in range (exact path).
  `SessionsResult.rows` is capped server-side (`limit`, default 200, clamped to
  1..1000) to the largest sessions by total tokens, while `totalSessions` and
  `totals` always describe the whole range. Session rows carry counters and
  identifiers only — never prompt or response text.

## 5. Tauri commands

All commands are `async`-safe, return `Result<T, String>` on the Rust side and
are called only through `src/lib/api.ts`. Argument names are camelCase on the
JS side (Tauri converts to snake_case Rust parameters).

### Backend (data) commands — `src-tauri/src/commands.rs`
| command | args | returns |
|---|---|---|
| `get_snapshot` | – | `AppSnapshot` (cached, never blocks on network) |
| `refresh_now` | `provider?: ProviderId` | `AppSnapshot` (forces network fetch) |
| `get_settings` | – | `Settings` |
| `update_settings` | partial settings JSON; nested `providers`, `colors`, `sizes`, `thresholds`, `sidebarItems` preserve untouched members | `Settings` (also emits `settings-updated` after persistence succeeds) |
| `get_usage_history` | `query: HistoryQuery` | `HistoryResult` |
| `get_usage_calendar` | `query: CalendarQuery` | `CalendarResult` (local-day calendar **and** weekday × hour punch card from one scan) |
| `get_usage_sessions` | `query: SessionQuery` | `SessionsResult` (top `limit` sessions by tokens + the full-range count/totals) |
| `get_quota_history` | `query: QuotaHistoryQuery` | `QuotaSample[]` |
| `get_pricing` | – | `PricingTable` |
| `set_pricing` | `table: PricingTable` | `PricingTable` |
| `refresh_pricing` | – | `PricingTable` (downloads `settings.pricingUrl` now; errors when it is empty — no network call is ever made without it) |
| `reingest_logs` | – | `IngestStats` (full rescan) |
| `get_providers` | – | `ProviderInfo[]` |
| `get_app_info` | – | `AppInfo` |
| `export_usage_csv` | `csv: string, suggestedName: string` | `string \| null` (native save dialog, UTF-8 CSV path on success; null on cancel) |

### Updater commands — `src-tauri/src/updater.rs`
| command | args | returns |
|---|---|---|
| `get_update_status` | – | `UpdateStatus` (cached; never touches the network) |
| `check_for_updates` | – | `UpdateStatus` (reads `latest.json`; never downloads a bundle) |
| `install_update` | – | `()` — downloads, installs and restarts. Only valid while `UpdateStatus.canInstall`; errors otherwise |

### Platform (window) commands — `src-tauri/src/window/`
| command | args | effect |
|---|---|---|
| `sidebar_set_expanded` | `expanded: boolean` | expand to full width / collapse to the thin handle (window resize + reposition). Emits `sidebar-state`. |
| `sidebar_relayout` | `width: number, height: number` (CSS px the bar content needs) | resize sidebar window to fit content and re-anchor to the edge |
| `sidebar_drag` | `phase: 'start'\|'move'\|'end'\|'cancel', dx: number, dy: number` (CSS px the pointer travelled since `start`, screen space) | the bar follows the pointer; on `end` it snaps to the nearest of the four edges of the monitor it was dropped on (distances normalised by the monitor's half extent, so the zones meet at its diagonals, with a small hysteresis in favour of the current edge for corner drops) and the result is persisted as `edge` / `monitor` / `verticalOffset` (emits `settings-updated`). `cancel` puts it back. |
| `popover_show` | `req: PopoverRequest` | position popover next to the ring and show it; emits `popover-target` to the popover window |
| `popover_relayout` | `width: number, height: number` (CSS px the popover content needs) | resize the popover window to fit content and re-anchor it next to the ring |
| `popover_hide` | – | hide the popover now and unpin it (second click on the pinned ring). Hover-out hides an unpinned popover after 250 ms and a pinned one after 8 s. |
| `popover_set_pinned` | `pinned: boolean` | pinned popovers ignore hover-out |
| `hover_report` | `source: "bar" \| "popover", hovered: boolean` | Rust keeps a hover state machine. `bar/true` expands a collapsed bar and cancels timers. When neither bar nor popover is hovered: the popover hides after ~250 ms (unless pinned) and, if `autoHide`, the bar collapses after `autoHideDelayMs`. |
| `open_dashboard` | `tab?: "overview" \| "history" \| "settings"` | show/focus dashboard window, emits `dashboard-navigate` |
| `apply_window_settings` | – | re-read settings (edge, monitor, vertical position, opacity, autoHide, always-on-top) and reposition windows |
| `get_monitors` | – | `MonitorInfo[]` |
| `get_shortcut_status` | – | `ShortcutStatus` — why a configured global shortcut is not active (`null` = registered, or disabled because the setting is empty) |
| `quit_app` | – | exit |

### Events (Rust → JS, `listen()`)
| event | payload | emitted by |
|---|---|---|
| `snapshot-updated` | `AppSnapshot` | backend scheduler after every refresh |
| `settings-updated` | `Settings` | backend `update_settings` |
| `ingest-progress` | `IngestStats` | backend ingestion |
| `popover-target` | `PopoverRequest` | platform, tells the popover window what to render |
| `sidebar-state` | `SidebarState` | platform |
| `dashboard-navigate` | `{ tab: string }` | platform |
| `update-status` | `UpdateStatus` | updater, after every state change |

## 6. Windows

| label | route | style |
|---|---|---|
| `sidebar` | `/` | frameless, transparent, always on top, skip taskbar, not resizable, visible on all workspaces, no shadow |
| `popover` | `/popover` | same style, hidden by default, focus does not steal from the active app |
| `dashboard` | `/dashboard` | normal decorated window, hidden by default, min 880×600 |

Linux notes: GNOME Wayland does not allow clients to position windows or stay
on top, so on Linux the app forces `GDK_BACKEND=x11` (XWayland) unless
`AI_USAGE_SIDEBAR_BACKEND=wayland` is set. With NVIDIA drivers WebKitGTK needs
`WEBKIT_DISABLE_DMABUF_RENDERER=1`. Layer-shell support for wlroots/KDE is a
follow-up.

## 7. Settings (`settings.json` in the app config dir)

`edge` is `left | right | top | bottom`: left/right dock the bar as a vertical
pill, top/bottom as a horizontal strip. `verticalAlign` and `verticalOffset`
describe the position **along** that edge whatever its orientation — `top`
means the start of the edge (its left end on a top/bottom edge), `bottom` the
end, and a positive offset moves towards the end. The two field names are kept
as they are so existing `settings.json` files stay valid; the settings tab
relabels the controls ("Horizontal align / offset") on a horizontal edge.

See `Settings` in `types.ts`. Defaults: right edge, vertically centred, always
shown (`autoHide=false`), `ringMode="concentric"`, every `sidebarItems` member on, `percentMode="used"`,
`refreshIntervalSec=60`, `adaptiveRefresh=true`, dark theme, `surfaceStyle="glass"` (translucent liquid-glass pill/popover with specular highlight; `solid` = opaque, `cyber` = a neon sci-fi HUD painted by the frontend with no native backdrop), language `auto`, ingestion enabled,
autostart off, thresholds warn 70 / critical 90, `notifications=false` with
`forecastNotifications=true` (the predictive warning is on by default but only
fires while `notifications` is on, at most once per window per reset period and
only for a `medium`/`high` confidence forecast).

### Sidebar items (what the bar shows)

`Settings.sidebarItems` (`fiveHour`, `weekly`, `scoped`, `other`, `logo`,
`percentLabel`, `moreButton`, all default `true`) and
`ProviderSettings.showInSidebar` (default `true`) decide what the **bar**
draws. They are presentation only: a provider hidden from the bar is still
polled and still appears in the dashboard, the history and the popover, unlike
`ProviderSettings.enabled`, which switches the provider off entirely.

Rules, implemented in `src/lib/sidebar-items.ts` and unit-tested in
`tests/sidebar-items.unit.ts`:
* windows are filtered **before** the ring groups are built, so the same rules
  apply in all three `ringMode`s. A window with a `scope` counts as `scoped`
  whatever its `kind`; `other` means an account-wide window that is neither
  5-hour nor weekly;
* the percent label of a group describes its primary window, falling back to
  the first *visible* window;
* a provider whose windows are all hidden disappears from the bar (only from
  the bar). A provider that reports no windows at all keeps its dimmed
  placeholder ring, because that is status, not a hidden item;
* `barSeverity()` — the collapsed handle colour, and anything that warns the
  user — reads **every** window of **every** polled provider, so hiding a ring
  can never hide a warning;
* if the configuration would leave the pill empty, the "⋯" grip is rendered
  anyway (whatever `moreButton` says), so the bar stays hoverable, draggable
  and never measures zero.

`showScopedRing` and `showPercentLabel` are **deprecated** aliases of
`sidebarItems.scoped` / `sidebarItems.percentLabel`. `settings.rs` migrates an
old file into the nested object on load and keeps writing both (the nested
value wins when a patch carries both spellings). Writing both was chosen over
dropping the flat keys because it costs two lines and keeps hand-written
settings files, older builds and downgrades working; nothing in the UI reads
the flat fields any more.

`cyberAccent="neon"`, `hideAccountEmail=false` and `monthlyBudgetUsd=0` (no budget
line on the History tab) complete the defaults, together with `autoUpdateCheck=true` and empty
`shortcutToggleSidebar` / `shortcutOpenDashboard` (= no global shortcut registered).

`monthlyBudgetUsd` (0 – 1 000 000, 0 = off) is an **estimated** monthly cost
budget. When it is set and the History tab shows cost, the tab draws the
month-to-date cumulative estimate against it and states the percentage used
and the linear pace. It never affects quotas, notifications or billing.

`cyberAccent` (`neon` | `matrix` | `amber` | `ice` | `synthwave`) picks the
neon pair the `cyber` surface is painted with. `applyTheme` stamps it on
`<html>` as `data-cyber`; `src/lib/styles/cyber.css` keys the pair, the plate
tint and the text tokens off it, and everything else in that file is written
in terms of `--cy-a` / `--cy-b`. The other surface styles ignore it.

`hideAccountEmail` (default false) is a privacy switch for screenshots and
screen sharing: every place that renders an account address — card text,
`title` attributes, exports — goes through `accountEmail()` in
`src/lib/privacy.ts`, which masks it as `h•••@g•••.com`. The backend keeps
sending the real address; only the rendering changes.

### Polling schedule (`scheduler.rs`)

`refreshIntervalSec` is the *steady-state* interval of a provider that is
being used. On top of it the scheduler applies, per provider, the **longest**
of these waits — the decision functions (`poll_interval_secs`,
`next_poll_due_ms`, `should_poll`, `should_force_poll`) are pure and unit
tested:

* **floor** — Claude is never polled more often than every 120 s
  (`CLAUDE_MIN_INTERVAL_SEC`). `/api/oauth/usage` shares its budget with
  Claude Code's own calls and a 5-hour window only moves ~1 % per 3 min, so a
  shorter interval buys nothing and earns `HTTP 429`. Codex uses the
  configured value (minimum 15 s).
* **idle stretch** (`adaptiveRefresh`) — the log watcher records when each
  provider's session logs last changed; after ~10 min of quiet the interval
  doubles, after ~30 min it is ×5, capped at 10 min. Fresh log activity, the
  tray's "Refresh now", `refresh_now` and opening the dashboard end the
  stretch at once.
* **learned stretch (AIMD)** — every `429` doubles a per-provider multiplier
  (cap ×8 and 15 min) which halves again only after five consecutive good
  polls, so one success cannot put the app back on the cadence that caused
  the limit.
* **gates** — the exponential error backoff (cap 5 min) and, for a `429`, the
  server's `Retry-After` (delta-seconds or HTTP-date, clamped to 30 s–1 h,
  default 5 min). An explicit refresh ignores the schedule and the error
  backoff but still honours `Retry-After`.

The learned multiplier and both gates are persisted in
`<data_dir>/cache/poll-state.json`, and the cached snapshot's `fetchedAt`
counts as the last poll, so restarting the app does not produce a burst of
requests.
### Updates

`autoUpdateCheck` only governs the *automatic* check (30 s after start-up,
then daily); the manual "Check for updates" action always exists. A check
never downloads anything, and installing is always an explicit click.
`UpdateStatus.canInstall` is false for `.deb`/`.rpm` and
`scripts/install-linux.sh` installs (detected through the `APPIMAGE`
environment variable), where the UI links to the release page instead. The
signing key, `latest.json` and what a release looks like without either are
documented in `docs/RELEASING.md`.

### Colours and sizes

There is no fixed provider list in the frontend. `providers::DEFAULT_PROVIDER_ORDER`
is the single registry: `Settings::default()` and `settings::clamp` seed one
`ProviderSettings` entry per id (enabled per `providers::enabled_by_default`,
so an experimental provider stays off until its credentials exist), and the UI
renders whatever `get_providers` / the snapshot report. A provider whose
`Provider::experimental()` is true and which is switched off is left out of
`AppSnapshot` entirely (no ring, no "disabled" card) while still being listed
by `get_providers`, so it can be switched on; a *verified* provider that is
switched off keeps its `disabled` entry exactly as before. A provider accent is
addressed as `var(--accent-<id>-N, var(--accent-fallback-N))`, so an id with no
hand-tuned tokens in `theme.css` still gets a ring; `ProviderLogo.svelte` draws
a monogram when it has no mark for the id.

`Settings.colors` (hex strings; one per provider that has a user-tunable
accent, plus the fixed `warn`/`critical`/`surface`/`text`; `surface`/`text`
empty = theme default) and
`Settings.sizes` (px at scale 1: `ringSize` 40–96, `ringStroke` 3–8, `barGap`
6–40, `barPadding` 4–24, `cornerRadius` 8–40, `labelSize` 9–18) are applied by
the frontend as CSS custom properties; the backend only clamps and persists
them. `settings.json` is watched (`settings::watch`, `notify` on the config
*directory*, 300 ms debounce): an external edit — by the user or by an AI
agent — is re-read, merged and clamped exactly like start-up, swapped into
`AppState.settings` and published as `settings-updated`, so no restart is
needed. The decision is the pure `settings::reload_action`: our own atomic
save is recognised by its bytes and ignored, a file that is missing or does
not parse yet is waited out rather than treated as empty, and the comparison
always uses the file and the memory *as they are now*, so a late event cannot
clobber a newer in-memory change. `update_settings` merges `colors`, `sizes`,
`thresholds` and each provider's fields individually. Invalid nested members do not discard valid
siblings. Backend read/merge/write/event emission is serialized, and memory
changes only after a successful disk write. Each frontend window serializes
its own saves and keeps newer optimistic edits visible while earlier saves finish.

## 8. Storage (`usage.db` in the app data dir, SQLite)

```sql
CREATE TABLE usage_events (
  id INTEGER PRIMARY KEY, provider TEXT NOT NULL, model TEXT NOT NULL,
  ts INTEGER NOT NULL, input_tokens INTEGER NOT NULL DEFAULT 0,
  cache_write_tokens INTEGER NOT NULL DEFAULT 0, cache_read_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0, reasoning_tokens INTEGER NOT NULL DEFAULT 0,
  total_tokens INTEGER NOT NULL DEFAULT 0, session_id TEXT, request_id TEXT NOT NULL,
  cwd TEXT, source_file TEXT, UNIQUE(provider, request_id));
CREATE INDEX idx_usage_ts ON usage_events(ts);
CREATE TABLE quota_samples (
  id INTEGER PRIMARY KEY, provider TEXT NOT NULL, kind TEXT NOT NULL, scope TEXT,
  used_percent REAL NOT NULL, resets_at INTEGER, plan TEXT, ts INTEGER NOT NULL);
CREATE INDEX idx_quota_ts ON quota_samples(provider, ts);
CREATE TABLE ingest_files (
  path TEXT PRIMARY KEY, provider TEXT NOT NULL, size INTEGER NOT NULL,
  mtime INTEGER NOT NULL, byte_offset INTEGER NOT NULL, last_ingested_at INTEGER NOT NULL);
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);
```
Ingestion is incremental (remember byte offset per file; if the file shrank or
was rewritten without growth, re-parse from 0). File modification times are
recorded at nanosecond precision; upgrading older bookkeeping causes one rescan.
An unfinished JSONL line, including an incomplete UTF-8 character, is retried
on the next append. A quota sample is stored only for a successful live API
refresh when the percent changed or ≥ 5 min passed; cached/offline values
never acquire a new sample timestamp. History ranges use `[from, to)` and
local calendar buckets.

`get_usage_history`, `get_usage_calendar` and `get_usage_sessions` each run a
single range-scan over `idx_usage_ts` and aggregate in Rust — one query per
panel, never one per day/session, and only the aggregates cross into the
webview. Bucketing stays in Rust (not in SQL `localtime`) so all three views
agree on the same DST-aware local calendar. No extra index is warranted: the
`(?N IS NULL OR col = ?N)` filters cannot be used as index prefixes, and the
cost of these queries is reading the rows in range, which no index removes.

## 9. Cost estimation

`pricing.rs` ships an API-equivalent price list (USD per 1M tokens: input,
output, cache write, cache read). Built-in model names match exactly after
case/date normalization; new custom entries support longest-prefix matching
(`find_entry`).

A model that matches nothing exactly falls back to the **closest known
family** (`find_family_entry`): among the entries sharing the longest run of
leading `-`/`.` components — at least two — the most generic one wins, so
`gpt-5.3-codex-spark` is priced as `gpt-5.3-codex` and `gpt-5.9` as `gpt-5`,
while `llama-9` stays unpriced. Such a price is an approximation, never an
exact match: `find_match` / `estimate_cost_kind` report it as
`MatchKind::Family` and `HistoryResult.costApproximate` tells the UI to say
so. A model with no family at all still yields `estimatedCostUsd = null` for
every group it touches, rather than a misleading partial total.

The effective table is layered: the user's saved `pricing.json` wins; below
it sits the cached remote list, and below that the bundled defaults. The
remote layer is **opt-in** — nothing is downloaded unless the user sets
`settings.pricingUrl` to an `https` URL. It is then fetched at most once a
day (or on `refresh_pricing`), limited to 256 KiB and 2000 entries,
validated for plausible names and rates, and cached as
`<data_dir>/pricing-remote.json`; any failure keeps the previous table.
`pricing.json` at the repository root is the same schema, so the project can
host its own list over raw.githubusercontent. Subscription users do not pay per token;
the estimate is a *comparison indicator* and is labelled as such in the UI.
The saved user table is authoritative (including removed rows or an empty
table); defaults apply only when no valid saved table exists.

Defaults were checked on 2026-09-20 against
[OpenAI pricing](https://developers.openai.com/api/docs/pricing),
[GPT-5.5](https://developers.openai.com/api/docs/models/gpt-5.5),
[GPT-5.3-Codex](https://developers.openai.com/api/docs/models/gpt-5.3-codex), and
[Claude pricing](https://platform.claude.com/docs/en/about-claude/pricing).
These are standard short-context estimates, not invoices: fast/batch tiers,
long-context multipliers, region fees and cache TTL differences are not
tracked in the normalized counters. Cache writes use the published rate
where available, otherwise the base input rate.

## 10. History export and verification

The History tab also carries a 26-week activity heatmap (local days, or the
same range as a weekday × hour punch card) whose cells are keyboard-focusable
and labelled with their date and value; picking a day narrows the range below
to that single local day. The session drill-down and the bucket table share
the copy/export buttons, so the CSV always matches the visible view.

History presets include today and count local calendar days. Custom ranges
end at midnight after the final selected day, including DST transitions;
invalid dates stop the query and show a validation message. Later queries
win over earlier requests. CSV uses CRLF, quotes embedded delimiters and
escapes spreadsheet formula prefixes in text fields. Clipboard failure is
reported. Desktop export opens a native save dialog and writes UTF-8 with
a BOM (cancel returns null); the browser preview downloads a Blob.

`pnpm test` covers date, CSV and save-queue regressions. `pnpm test:e2e`
exercises the three browser routes using synthetic data. Native geometry,
parsing, persistence and aggregation are tested with `cargo test --locked`.
CI additionally builds bundles on Linux, macOS and Windows. Browser tests
do not prove native window-manager behavior; see `docs/VALIDATION.md`.
