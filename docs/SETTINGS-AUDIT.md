# Settings audit (v0.7)

v0.6 ended with 59 settings across 16 cards in the dashboard. They all work,
but a first-time user faced all of them at once. v0.7 sorts them into tiers and
removes the two that were pure duplicates. Nothing a user could depend on was
dropped without a read path. The decision-support work (routing advice,
commit attribution) then added two advanced keys, `advisorNotifications` and
`gitAttribution`, in a new advanced-only Advisor card.

## Rules used

* **basic**: most people want to look at or change it in the first weeks
  (what the bar shows and where, language and theme, whether to be notified,
  which providers, the privacy switch). Default quality is not a criterion:
  a basic setting still needs a good default.
* **advanced**: tuning, power-user or rarely changed (geometry, colours,
  timings, integrations, webhooks, shortcuts, price list). The default is
  right for nearly everyone, so hiding it costs little.
* **internal**: bookkeeping the app writes itself, or a value edited through
  another card. Never a control of its own.
* A card whose settings are all advanced is not on the page or in the index
  until "Show advanced settings" is on **or the user has changed something in
  it** (a configured extra account, a custom colour, a shortcut), so
  configuration in use never disappears.
* The tier changes only what is **shown**. Every key behaves the same from the
  file, from presets and from the command palette whatever its tier.
* Search always finds advanced controls; a match that is only visible because
  of the search carries a small "advanced" badge. The command palette lists
  every card and every flippable setting and reveals an advanced-only card when
  it jumps there.
* The switch is UI state kept per viewer in `localStorage`
  (`ai-usage-sidebar.showAdvancedSettings`), not a setting: it does not
  roam with `settings.json`, appear in exports or presets, or add a key.
* Per-card "Reset to defaults" resets **all** the card's keys including hidden
  advanced ones, and its confirmation says so.

The tiers live in one typed table, `src/lib/settings-tiers.ts`
(`SETTING_TIERS`); the AGENTS.md section 5 "Tier" column must match it
(`scripts/check-agents-settings.mjs`, run in CI).

## Result

| | before | after |
|---|---|---|
| keys in `Settings` (and in the written file) | 59 | 59 (57 after removing the two duplicates, +2 advisor keys) |
| basic / advanced / internal keys | - | 22 / 32 / 5 |
| cards in the index by default | 16 | 12 of 17 (the Advisor card is advanced-only, so the default count is unchanged) |
| controls visible by default (render suite, `.setting-field` rows) | 80 | 31 (measured before the Advisor card, which adds none) |

## Table

Decisions: **keep** (stays as is), **advanced** (moved behind the switch),
**remove** (no longer part of `Settings`, still read from old files).

| Key | Card | Tier | Decision | Reason |
|---|---|---|---|---|
| `version` | - | internal | keep | schema marker |
| `language` | Appearance | basic | keep | first thing anyone changes; `auto` default |
| `theme` | Appearance | basic | keep | ditto |
| `surfaceStyle` | Appearance | basic | keep | the main visual choice (glass / solid / cyber), also in presets |
| `cyberAccent` | Appearance | basic | keep | only shown while the cyber surface is selected |
| `scale` | Appearance | basic | keep | accessibility: the quickest way to make the bar bigger |
| `opacity` | Appearance | advanced | advanced | overlaps the surface choice; 1 is right for most |
| `percentMode` | Appearance | basic | keep | used vs remaining is a matter of taste that decides how every ring reads |
| `percentPosition` | Appearance | advanced | advanced | only matters when the percent label is on; a layout detail |
| `ringMode` | Appearance | basic | keep | one ring per provider or per window changes what the bar is |
| `ringStyle` | Appearance | advanced | advanced | ring vs mini-bar; secondary look |
| `labelContent` | Appearance | advanced | advanced | percent / countdown / both; secondary look |
| `sidebarAnimations` | Appearance | advanced | advanced | default on is fine; for motion-sensitive users the search finds it |
| `sidebarItems` | Sidebar items | basic | keep (members split) | `fiveHour`, `weekly`, `scoped`, `percentLabel` basic; `other`, `logo`, `moreButton` advanced (rare windows, decoration, the "..." grip) |
| `colors` | Size and colour | advanced | advanced | whole card; theme colours are good defaults, presets cover the common cases |
| `sizes` | Size and colour | advanced | advanced | ditto; `scale` is the basic way to resize |
| `edge` | Position | basic | keep | where the bar docks (also set by dragging) |
| `monitor` | Position | basic | keep | multi-monitor users need it |
| `verticalAlign` | Position | advanced | advanced | position along the edge; set by dragging |
| `verticalOffset` | Position | advanced | advanced | ditto |
| `alwaysOnTop` | Position | advanced | advanced | on is what the bar is for |
| `autoHide` | Behaviour | basic | keep | common preference |
| `autoHideDelayMs` | Behaviour | advanced | advanced | timing detail of `autoHide` |
| `collapsedWidth` | Behaviour | advanced | advanced | ditto |
| `popoverTimeoutSec` | Behaviour | advanced | advanced | timing; 10 s suits nearly everyone |
| `refreshIntervalSec` | Behaviour | basic | keep | trades freshness against API load; the one polling knob people ask about |
| `adaptiveRefresh` | Behaviour | advanced | advanced | an optimisation; the default is right |
| `autostart` | Behaviour | basic | keep | start at login |
| `trayDisplay` | Behaviour | advanced | advanced | icon vs percent in the tray; niche |
| `notifications` | Notifications | basic | keep | master switch, default off (opt-in) |
| `thresholdNotifications` | Notifications | basic | keep | what most people mean by "notify me" |
| `forecastNotifications` | Notifications | basic | keep | the pace warning is the app's most useful alert |
| `budgetNotifications` | Notifications | advanced | advanced | only matters with a monthly budget set |
| `weeklySummary` | Notifications | advanced | advanced | opt-in extra |
| `thresholds` | Notifications | advanced | advanced | 70 / 90 suits most; also colours the rings |
| `webhook` | Notifications | advanced | advanced | three controls (switch, URL, kind) and the test buttons are one group; needs an https endpoint |
| `focusUntil` | Notifications | basic | keep | do-not-disturb timer |
| `focusHidesSidebar` | Notifications | advanced | advanced | extra of focus mode |
| `shortcutToggleSidebar` | Shortcuts | advanced | advanced | off by default; whole card |
| `shortcutOpenDashboard` | Shortcuts | advanced | advanced | ditto |
| `providers` | Providers | basic | keep | which providers are tracked and shown |
| `openrouterKeyEnv` | Providers | advanced | advanced | only for OpenRouter users, and only to rename the variable |
| `accounts` | Accounts | advanced | advanced | whole card; most users have one login. Stays on the page once an extra account exists |
| `ingestEnabled` | Data | basic | keep | decides whether session logs are read at all (privacy relevant) |
| `monthlyBudgetUsd` | Data | basic | keep | the one number that turns History into a budget |
| `subscriptionUsd` | Data | advanced | advanced | ROI input; the app suggests prices |
| `quotaRetentionDays` | Data | advanced | advanced | 365 days is plenty |
| `autoUpdateCheck` | Updates | basic | keep | network behaviour users want to see |
| `autoPricingCheck` | Updates | advanced | advanced | independent of the app check, deliberately; price list is an expert topic |
| `pricingUrl` | Updates | advanced | advanced | custom price source |
| `skippedVersion` | - | internal | keep | the banner's "Skip this version" |
| `exportSnapshot` | Integrations | advanced | advanced | scripts and status lines only; whole card |
| `advisorNotifications` | Advisor | advanced | advanced | new in v0.7; opt-in notification for routing advice (the advice itself is always shown); whole card |
| `gitAttribution` | Advisor | advanced | advanced | new in v0.7; opt-in `git log` for the per-commit cost view; whole card |
| `pollingPaused` | Integrations | advanced | advanced | also reachable from the tray and the palette |
| `hideAccountEmail` | Privacy | basic | keep | the screen-sharing switch |
| `lastSeenVersion` | - | internal | keep | "What's new" bookkeeping |
| `onboarded` | - | internal | keep | first-run wizard bookkeeping |
| `customPresets` | Presets | internal | keep | edited through the Presets card, not a control |
| ~~`showScopedRing`~~ | - | - | **remove** | deprecated duplicate of `sidebarItems.scoped`, written only so older builds could still read it |
| ~~`showPercentLabel`~~ | - | - | **remove** | deprecated duplicate of `sidebarItems.percentLabel` |

## Removed keys: compatibility

`showScopedRing` and `showPercentLabel` are gone from `Settings`, from the
frontend types and defaults, and from the file the app writes. They are still
**read**:

* Rust: `settings::migrate_legacy_keys` rewrites them into `sidebarItems`
  before every merge, so it covers `settings.json` at start-up, an external
  edit while running, a restored backup or history version, an import and
  `update_settings` patches. A flat key of the wrong type is ignored; if the
  same input also has the nested key, the nested one wins.
* TypeScript: `normalizePatch` in `settings-writer.ts` does the same for
  previews of presets (custom presets saved by old builds).
* Tests: `settings.rs` (flat keys migrate, nested wins, wrong type ignored,
  a v0.6 file with both spellings loads and is rewritten without the flat
  keys) and `tests/settings.unit.ts`.
* Downgrade: a v0.6 build reading a v0.7 file finds only the nested keys, which
  it understands; only a hand-edited flat key would be lost.

## Considered and kept apart

| Candidates | Why not merged |
|---|---|
| `notifications` + the per-kind toggles | the master is the opt-in; each kind has an independent default and reason to be off |
| `autoUpdateCheck` + `autoPricingCheck` | different endpoints with different consequences; v0.6 made them independent on purpose |
| `autoHide` + `autoHideDelayMs` + `collapsedWidth` | already one group; two of the three are now advanced, the toggle is not a duplicate |
| `percentMode` + `percentPosition` + `labelContent` + `sidebarItems.percentLabel` | four orthogonal axes (value, place, content, on/off); merging would need a new enum and a migration for a gain the tiers already deliver |
| `focusUntil` + `focusHidesSidebar` | a timer and a behaviour; the second is advanced |
| `pollingPaused` | also in the tray and the palette; one key, three entry points is intended |
