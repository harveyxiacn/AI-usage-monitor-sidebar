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
  src/providers/          claude.rs, codex.rs, mod.rs (trait)     [BACKEND]
  src/ingest/             jsonl parsers, incremental ingestion     [BACKEND]
  src/store/              sqlite schema + queries                  [BACKEND]
  src/pricing.rs          default pricing table + cost estimation [BACKEND]
  src/scheduler.rs        periodic refresh + ingest, emits events  [BACKEND]
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
* `QuotaWindow.isPrimary`: exactly one window per provider is primary (the
  5‑hour window when the plan has one, else the weekly window).
  `ringMode = "concentric"` (default): one ring *group* per provider — outer
  ring = non-scoped weekly window, inner ring = 5-hour window (omitted when the
  plan has none, e.g. Codex Pro), optional innermost third ring = first scoped
  window (e.g. Claude "Weekly · Fable") when `showScopedRing` is on; Codex
  never gets a third ring (its scoped windows are additional per-feature
  limits). Rings of one group share the provider hue in decreasing intensity;
  threshold colours override per ring. The percent label shows the primary
  window. `ringMode = "primary"`: one plain ring per provider (primary window).
  `ringMode = "all"`: one ring per non-scoped window.
* `ProviderQuota.status`: `ok | not_logged_in | token_expired | error |
  disabled`. Any status other than `ok` still returns the last known windows
  (from cache or local logs) if available, with `source` telling where they
  came from.
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
| `update_settings` | partial settings JSON; nested `providers`, `colors`, `sizes`, `thresholds` preserve untouched members | `Settings` (also emits `settings-updated` after persistence succeeds) |
| `get_usage_history` | `query: HistoryQuery` | `HistoryResult` |
| `get_quota_history` | `query: QuotaHistoryQuery` | `QuotaSample[]` |
| `get_pricing` | – | `PricingTable` |
| `set_pricing` | `table: PricingTable` | `PricingTable` |
| `reingest_logs` | – | `IngestStats` (full rescan) |
| `get_providers` | – | `ProviderInfo[]` |
| `get_app_info` | – | `AppInfo` |
| `export_usage_csv` | `csv: string, suggestedName: string` | `string \| null` (native save dialog, UTF-8 CSV path on success; null on cancel) |

### Platform (window) commands — `src-tauri/src/window/`
| command | args | effect |
|---|---|---|
| `sidebar_set_expanded` | `expanded: boolean` | expand to full width / collapse to the thin handle (window resize + reposition). Emits `sidebar-state`. |
| `sidebar_relayout` | `width: number, height: number` (CSS px the bar content needs) | resize sidebar window to fit content and re-anchor to the edge |
| `sidebar_drag` | `phase: 'start'\|'move'\|'end'\|'cancel', dx: number, dy: number` (CSS px the pointer travelled since `start`, screen space) | the bar follows the pointer; on `end` it snaps to the nearer edge of the monitor it was dropped on and the result is persisted as `edge` / `monitor` / `verticalOffset` (emits `settings-updated`). `cancel` puts it back. |
| `popover_show` | `req: PopoverRequest` | position popover next to the ring and show it; emits `popover-target` to the popover window |
| `popover_relayout` | `width: number, height: number` (CSS px the popover content needs) | resize the popover window to fit content and re-anchor it next to the ring |
| `popover_hide` | – | hide popover (unless pinned) |
| `popover_set_pinned` | `pinned: boolean` | pinned popovers ignore hover-out |
| `hover_report` | `source: "bar" \| "popover", hovered: boolean` | Rust keeps a hover state machine. `bar/true` expands a collapsed bar and cancels timers. When neither bar nor popover is hovered: the popover hides after ~250 ms (unless pinned) and, if `autoHide`, the bar collapses after `autoHideDelayMs`. |
| `open_dashboard` | `tab?: "overview" \| "history" \| "settings"` | show/focus dashboard window, emits `dashboard-navigate` |
| `apply_window_settings` | – | re-read settings (edge, monitor, vertical position, opacity, autoHide, always-on-top) and reposition windows |
| `get_monitors` | – | `MonitorInfo[]` |
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

See `Settings` in `types.ts`. Defaults: right edge, vertically centred, always
shown (`autoHide=false`), `ringMode="concentric"`, `showScopedRing=true`, `percentMode="used"`,
`refreshIntervalSec=60`, dark theme, `surfaceStyle="glass"` (translucent liquid-glass pill/popover with specular highlight; `solid` = opaque), language `auto`, ingestion enabled,
autostart off, thresholds warn 70 / critical 90.

### Colours and sizes

`Settings.colors` (hex strings; `surface`/`text` empty = theme default) and
`Settings.sizes` (px at scale 1: `ringSize` 40–96, `ringStroke` 3–8, `barGap`
6–40, `barPadding` 4–24, `cornerRadius` 8–40, `labelSize` 9–18) are applied by
the frontend as CSS custom properties; the backend only clamps and persists
them. `update_settings` merges `colors`, `sizes`, `thresholds` and each
provider's fields individually. Invalid nested members do not discard valid
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

## 9. Cost estimation

`pricing.rs` ships an API-equivalent price list (USD per 1M tokens: input,
output, cache write, cache read). Built-in model names match exactly after
case/date normalization; new custom entries support longest-prefix matching.
Unknown variants must not inherit a similarly named model's built-in price.
Any group containing an unknown model has `estimatedCostUsd = null`, rather
than a misleading partial total. Subscription users do not pay per token;
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
