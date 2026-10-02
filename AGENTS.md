# Installing AI Usage Sidebar — instructions for AI agents

You are an AI coding agent (Claude Code, Codex, Cursor, …) and a user asked you
to install or configure **AI Usage Sidebar**. Follow this file top to bottom.
Everything here is per-user and reversible. Humans: the README has the same
information in prose.

Repository: `https://github.com/harveyxiacn/AI-usage-monitor-sidebar`

## 0. Ground rules

- Never print, copy or upload `~/.claude/.credentials.json`, `~/.codex/auth.json`
  or the macOS Keychain item `Claude Code-credentials`. The app reads them
  itself; you never need their contents.
- Ask before using `sudo` / an elevated shell. Prefer the per-user routes below.
- The builds are **unsigned**. Tell the user before you bypass Gatekeeper or
  SmartScreen; do not do it silently.
- Do not simulate mouse or keyboard input to test the app.

## 1. Detect the platform

```sh
uname -s          # Linux | Darwin ; on Windows use: $env:OS  (PowerShell)
uname -m          # x86_64 | arm64 | aarch64
```

Linux only — find the package family:
`command -v pacman apt-get dnf zypper 2>/dev/null`

## 2. Install

Pick the newest release and download **one** asset. With the GitHub CLI:

```sh
gh release download --repo harveyxiacn/AI-usage-monitor-sidebar --pattern '<PATTERN>' --dir "$TMPDIR_OR_DOWNLOADS"
```

Without `gh`, list assets at
`https://api.github.com/repos/harveyxiacn/AI-usage-monitor-sidebar/releases/latest`
and download the `browser_download_url` whose name matches the pattern.

| Platform | `<PATTERN>` | Install command |
|---|---|---|
| Debian / Ubuntu (x86_64) | `*_amd64.deb` | `sudo apt install ./<file>.deb` |
| Fedora / openSUSE (x86_64) | `*.x86_64.rpm` | `sudo dnf install ./<file>.rpm` (or `zypper install`) |
| Any Linux x86_64, no root | `*_amd64.AppImage` | `chmod +x <file>.AppImage`, move it to `~/.local/bin/`, run it |
| Arch / CachyOS / Manjaro, other arches | – | build from source, §2.1 |
| macOS Apple Silicon | `*_aarch64.dmg` | §2.2 |
| macOS Intel | `*_x64.dmg` | §2.2 |
| Windows x64 | `*_x64-setup.exe` | run it; per-user, no admin needed. `*.msi` is the alternative for managed machines |

If there is no release yet, or no asset fits, build from source (§2.1).

### 2.1 Linux from source (per-user, no root for the install itself)

Build dependencies (this step may need `sudo`; ask first):

```sh
# Arch / CachyOS
sudo pacman -S --needed webkit2gtk-4.1 libayatana-appindicator librsvg base-devel rustup nodejs pnpm && rustup default stable
# Debian / Ubuntu
sudo apt install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev build-essential curl file libssl-dev
# + rustup (https://rustup.rs) and Node 22 with pnpm (`corepack enable pnpm`)
```

Then:

```sh
git clone https://github.com/harveyxiacn/AI-usage-monitor-sidebar.git
cd AI-usage-monitor-sidebar
scripts/install-linux.sh        # release build (~3 min) + install
```

This installs `~/.local/bin/ai-usage-sidebar`, a launcher entry
(`~/.local/share/applications/ai-usage-sidebar.desktop`) and icons. Re-run it
to update. `scripts/install-linux.sh --uninstall` removes it and keeps the
user's settings and history.

### 2.2 macOS

```sh
hdiutil attach <file>.dmg -nobrowse
cp -R "/Volumes/AI Usage Sidebar/AI Usage Sidebar.app" /Applications/
hdiutil detach "/Volumes/AI Usage Sidebar"
```

The app is not notarised. After telling the user, clear the quarantine flag:
`xattr -dr com.apple.quarantine "/Applications/AI Usage Sidebar.app"`.
On first refresh macOS asks for access to the Keychain item
`Claude Code-credentials`; the user should choose **Always Allow**.

## 3. Start it and verify

| Platform | Start | Log directory |
|---|---|---|
| Linux | `gtk-launch ai-usage-sidebar` or `~/.local/bin/ai-usage-sidebar &` (packages: `ai-usage-sidebar &`) | `~/.local/share/io.github.harveyxiacn.ai-usage-sidebar/logs/` |
| macOS | `open -a "AI Usage Sidebar"` | `~/Library/Logs/io.github.harveyxiacn.ai-usage-sidebar/` |
| Windows | Start menu → AI Usage Sidebar | `%LOCALAPPDATA%\io.github.harveyxiacn.ai-usage-sidebar\logs\` |

Healthy start-up writes `platform setup: …`, `tray menu ready` and
`sidebar revealed` to the newest log file within a few seconds. Starting the
app a second time does not create a second instance; it opens the dashboard.

What the user should see: a thin bar on the right screen edge with one ring per
provider. Hover a ring for details, click to pin, click again to close, drag
the bar to move it, double-click it (or use the tray icon) for the dashboard.
On Linux the tray icon has a menu only: a left click does nothing there.

A check that needs no UI: with `"exportSnapshot": true` (see §5) the app writes
`snapshot.json` after every refresh, and `ai-usage-sidebar --print` reads it
back and exits (exit code 2 = missing or stale data, `--format json` shows
everything). A signed-in provider reports status `ok` there.

## 4. Providers

The app shows whatever the official CLIs are already signed in to. It has no
login of its own (OpenRouter is the one exception: it reads an API key from an
environment variable, below).

- Claude Code: the user runs `claude` and signs in (`/login`).
- Codex: the user runs `codex login` (ChatGPT sign-in). API-key mode has no
  quota windows, so the ring shows "not signed in" — that is expected.
- GitHub Copilot: **experimental and unverified** (see `docs/PROVIDERS.md`). It
  reads the token the Copilot editor plugins leave in
  `~/.config/github-copilot/apps.json` and is switched off unless that file
  already holds a github.com token. Do not enable it for a user without saying
  it was never tested against a live account.
- OpenRouter: **experimental and unverified**, always off until the user
  enables it (`providers.openrouter.enabled`, or Settings → Providers). It has
  no CLI login: the API key is read from the environment variable named by
  `openrouterKeyEnv` (default `OPENROUTER_API_KEY`). Never write a key into
  `settings.json`; the setting holds the variable's *name* and a pasted key is
  rejected. A variable exported after the app started needs an app restart.
- Multiple accounts: Claude Code and Codex can track extra logins through the
  `accounts` setting (at most 6, each `{id, provider, label, configDir,
  enabled}`; `configDir` must be absolute). The user signs the extra login in
  themself (`CLAUDE_CONFIG_DIR=<dir> claude` then `/login`, or
  `CODEX_HOME=<dir> codex login`); you never read what those write. Extra
  accounts get quota only (no token history), their keys read `claude@work`,
  and **on macOS an extra Claude account is read from
  `<configDir>/.credentials.json` only** (non-default Keychain items are not
  supported). `accounts` is replaced as a whole by a patch; invalid entries
  are dropped.
- Non-default locations of the primary account are honoured through
  `CLAUDE_CONFIG_DIR` / `CODEX_HOME`.

A ring in an error state has its message in the popover and in the log.
`HTTP 429` from Anthropic means "polled too often"; it clears by itself.

## 5. Configure

Settings live in one JSON file. The app **watches** it: save your edit and it
applies within about a second — no restart, no quitting. (It also rewrites the
file when the user changes something in the dashboard; a half-written file is
ignored until it parses, so write it atomically or in one go.)

| Platform | Path |
|---|---|
| Linux | `~/.config/io.github.harveyxiacn.ai-usage-sidebar/settings.json` |
| macOS | `~/Library/Application Support/io.github.harveyxiacn.ai-usage-sidebar/settings.json` |
| Windows | `%APPDATA%\io.github.harveyxiacn.ai-usage-sidebar\settings.json` |

The file may be partial: missing keys take their defaults, unknown or invalid
values are ignored and numbers are clamped. Write only what the user asked for.
Nested objects (`providers`, `sidebarItems`, `thresholds`, `colors`, `sizes`,
`webhook`, `subscriptionUsd`) are merged member by member; `accounts` and
`customPresets` are replaced as a whole.

Safety net: before each change the previous version is kept in
`settings.history.json` next to `settings.json` (newest first, 5 versions;
edits less than 3 s apart count as one). The user restores one under Settings →
Backup & history, and an external edit of `settings.json` is recorded too.
The same card makes a full backup (settings + a copy of the usage database,
restored on the next start) and exports / imports settings as a file; an
import goes through the same merge, so unknown or invalid keys are ignored.

Defaults are those of `src/lib/settings-defaults.ts` and `impl Default for
Settings` in `src-tauri/src/model.rs`; the latter is authoritative. The tables
below list every key exactly once; `pnpm check:agents`
(`scripts/check-agents-settings.mjs`) fails CI when they and the code disagree.

### Language, theme, look

| Key | Values (default first) | Meaning |
|---|---|---|
| `version` | `1` | schema marker; leave it alone (never imported) |
| `language` | `"auto"`, `"en"`, `"zh-CN"` | UI and tray language; `auto` follows the OS |
| `theme` | `"dark"`, `"light"`, `"auto"` | `auto` follows the OS |
| `surfaceStyle` | `"glass"`, `"solid"`, `"cyber"` | liquid glass (blurred backdrop where the OS has one), opaque, or a neon sci-fi HUD |
| `cyberAccent` | `"neon"`, `"matrix"`, `"amber"`, `"ice"`, `"synthwave"` | neon pair the `cyber` HUD is painted with; ignored by the other surfaces |
| `opacity` | `1` (0.3 – 1) | window opacity |
| `scale` | `1` (0.75 – 1.5) | overall size multiplier |
| `colors` | `{"claude":"#ff5c1a","codex":"#10a37f","copilot":"#8250df","openrouter":"#6467f2","warn":"#f5c542","critical":"#ff3b30","surface":"","text":""}` | CSS hex (`#rgb`, `#rrggbb`, `#rrggbbaa`); an invalid value falls back to the default; empty `surface` / `text` = the theme's own |
| `sizes` | `{"ringSize":56,"ringStroke":4.5,"barGap":18,"barPadding":10,"cornerRadius":26,"labelSize":13}` | px at scale 1, clamped to 40–96, 3–8, 6–40, 4–24, 8–40, 9–18 |

### The bar: position and behaviour

| Key | Values (default first) | Meaning |
|---|---|---|
| `edge` | `"right"`, `"left"`, `"top"`, `"bottom"` | screen edge (also set by dragging). `top`/`bottom` make the bar a horizontal strip |
| `verticalAlign` | `"center"`, `"top"`, `"bottom"` | position **along** the edge: `top` = its start (left end of a top/bottom edge), `bottom` = its end |
| `verticalOffset` | `0` (px, positive = towards the end of the edge: down on left/right, right on top/bottom) | also set by dragging |
| `monitor` | `null` = primary, or a monitor name | |
| `alwaysOnTop` | `true` | keep the bar above other windows |
| `autoHide` | `false`, `true` | collapse to a thin handle when idle |
| `autoHideDelayMs` | `800` (0 – 600000) | idle time before the bar collapses |
| `collapsedWidth` | `6` (2 – 24) | thickness in px of the collapsed handle |
| `popoverTimeoutSec` | `10` (0 – 600, 0 = never) | the detail popover closes after this many seconds without pointer activity; a pinned one gets 6× |
| `shortcutToggleSidebar` | `""` (off), e.g. `"Ctrl+Alt+U"` | global shortcut showing/hiding the bar (X11/XWayland, Windows, macOS; not native Wayland). A combination needs a modifier |
| `shortcutOpenDashboard` | `""` (off), e.g. `"Ctrl+Alt+D"` | global shortcut opening the dashboard |
| `autostart` | `false`, `true` | start at login |
| `trayDisplay` | `"icon"`, `"percent"` | `percent` shows the busiest window's percentage in the tray (menu-bar title on macOS, drawn into the icon on Windows/Linux) |

### The bar: what it draws

| Key | Values (default first) | Meaning |
|---|---|---|
| `ringMode` | `"concentric"`, `"primary"`, `"all"` | one ring per provider, only the primary window, or one per window |
| `ringStyle` | `"ring"`, `"bar"` | rings, or compact mini-bars (a logo dot plus one slim bar per window) |
| `percentMode` | `"used"`, `"remaining"` | |
| `percentPosition` | `"below"`, `"center"` | show the percentage below each ring, or in its centre in place of the provider logo; used only when `sidebarItems.percentLabel` is on |
| `labelContent` | `"percent"`, `"reset"`, `"both"` | what the label says: percentage, reset countdown or both |
| `sidebarAnimations` | `true`, `false` | one-shot pulse when a ring crosses a threshold and a flash on a reset; reduced motion turns them off |
| `sidebarItems` | `{"fiveHour":true,"weekly":true,"scoped":true,"other":true,"logo":true,"percentLabel":true,"moreButton":true}` | what the bar draws; hidden items are still polled and still shown in the dashboard |
| `showScopedRing` | `true` | deprecated alias of `sidebarItems.scoped` |
| `showPercentLabel` | `true` | deprecated alias of `sidebarItems.percentLabel` |
| `thresholds` | `{"warn":70,"critical":90}` | ring colour and notifications; each 1 – 100 and `warn` < `critical` |

### Providers, accounts and polling

| Key | Values (default first) | Meaning |
|---|---|---|
| `providers` | `{"claude":{"enabled":true,"showInSidebar":true,"order":0},"codex":{"enabled":true,"showInSidebar":true,"order":1},"copilot":{"enabled":false,"showInSidebar":true,"order":2},"openrouter":{"enabled":false,"showInSidebar":true,"order":3}}` | `enabled` off = not polled at all; `showInSidebar` off = hidden from the bar only. `copilot` is **experimental** and only turns itself on when its credentials are found; `openrouter` is **experimental** and always starts off |
| `accounts` | `[]` | extra Claude Code / Codex accounts, at most 6: `{"id":"work","provider":"claude","label":"Work","configDir":"/abs/path","enabled":true}`; see §4 |
| `openrouterKeyEnv` | `"OPENROUTER_API_KEY"` | *name* of the environment variable holding the OpenRouter key (upper-case letters, digits, `_`); never the key |
| `refreshIntervalSec` | `60` (min 15) | quota polling period; Claude is never polled faster than every 120 s, per account |
| `adaptiveRefresh` | `true`, `false` | poll a provider less often while its session logs are quiet |
| `pollingPaused` | `false`, `true` | skip the *automatic* polling (log ingestion keeps running; an explicit refresh still polls). Also in the tray and the command palette |
| `ingestEnabled` | `true`, `false` | parse the CLIs' local session logs into the usage database (token history, sessions, forecasts); off = quota rings only |
| `quotaRetentionDays` | `365` (0 – 3650, 0 = keep forever) | delete quota samples older than this; samples older than 14 days are thinned to one peak per hour. Token usage events are never deleted |

### Cost, notifications and integrations

| Key | Values (default first) | Meaning |
|---|---|---|
| `pricingUrl` | `""` | price source URL. Empty = the project's GitHub raw `pricing.json`; non-empty must be an `https://` custom source |
| `autoPricingCheck` | `true`, `false` | check the selected price source about 60 s after startup and daily; checks only and reminds, never applies prices automatically |
| `monthlyBudgetUsd` | `0` (0 – 1000000, 0 = off) | monthly *estimated* cost budget shown in History; never billing |
| `subscriptionUsd` | `{"claude":0,"codex":0}` (each 0 – 10000, 0 = unknown) | what the user pays per month per provider, to compare with the API-equivalent estimate in History |
| `notifications` | `false`, `true` | master switch for every notification (native and webhook); the switches below only act while it is on |
| `thresholdNotifications` | `true`, `false` | warn when a window crosses `thresholds.warn` / `thresholds.critical` (once per window, level and cycle) |
| `forecastNotifications` | `true`, `false` | warn when a window is on pace to run out before it resets |
| `advisorNotifications` | `false`, `true` | suggest another provider ("Claude runs out in ~25 min, Codex has 70 % of its week left") when one is on pace to run out and another has room; confident forecasts only, once per reset period. The suggestion is always shown in the Overview and the popover; this only adds a notification. Needs `notifications` |
| `budgetNotifications` | `true`, `false` | warn at 80 % and 100 % of `monthlyBudgetUsd` (needs a budget > 0), once a month each |
| `weeklySummary` | `false`, `true` | Monday ~09:00: one notification summarising last week |
| `webhook` | `{"enabled":false,"url":"","kind":"generic"}` | optional second channel: `kind` is `"generic"` (JSON), `"ntfy"` or `"slack"`; `https://` only (enabling with another URL turns it off); the URL is never logged |
| `focusUntil` | `0` (off), `-1` (until turned off), or an epoch-ms deadline | focus mode silences every notification channel; normally set from the tray, the palette or Settings |
| `focusHidesSidebar` | `false`, `true` | also hide the bar while focus mode is on |
| `exportSnapshot` | `false`, `true` | write `snapshot.json` after every refresh for `ai-usage-sidebar --print` and status bars (`docs/STATUSLINE.md`) |
| `gitAttribution` | `false`, `true` | task-level cost: for project folders already in the usage log, run a read-only `git log` (hash, time, subject; no diff, no file contents, 10 s timeout) to show tokens and estimated cost per commit in History → Commits. Off = no process is started |
| `hideAccountEmail` | `false`, `true` | mask account e-mails as `h•••@g•••.com` (screenshots, screen sharing) |

### Updates, presets and first-run bookkeeping

| Key | Values (default first) | Meaning |
|---|---|---|
| `autoUpdateCheck` | `true`, `false` | check GitHub for a newer application release once a day; never installs on its own; independent from `autoPricingCheck` |
| `skippedVersion` | `""` | update version the user chose to skip; leave it to the app |
| `lastSeenVersion` | `""` | version whose "What's new" was last shown; leave it to the app |
| `onboarded` | `false`, `true` | the first-run wizard was done or skipped; a settings file that already exists counts as onboarded |
| `customPresets` | `{}` | the user's saved presets, name → partial settings object (at most 10, names up to 40 characters; a preset cannot carry `customPresets` or `focusUntil`). Built-in presets (minimal, power, screenShare, cyber) live in `src/lib/builtin-presets.json` |

Example — Chinese UI, left edge, auto-hide, start at login:

```json
{ "language": "zh-CN", "edge": "left", "autoHide": true, "autostart": true }
```

### Reading the numbers from a script

`ai-usage-sidebar --print [--format json|line|statusline] [--provider ID]`
prints the numbers from `snapshot.json` and exits: no window, no network, exit
code 2 when the file is missing or older than 15 minutes. It needs
`exportSnapshot` on and the app running (`docs/STATUSLINE.md`). It is also a
safe way to check an install without touching the UI.

## 6. Platform notes

- **GNOME on Wayland**: the app runs through XWayland on purpose (GNOME cannot
  position or keep-above native Wayland windows). The tray icon needs the
  AppIndicator extension:
  `https://extensions.gnome.org/extension/615/appindicator-support/`.
- **KDE / Hyprland / Sway**: native docking is opt-in with
  `AI_USAGE_SIDEBAR_BACKEND=wayland` and needs `gtk-layer-shell` installed; it
  falls back to XWayland on its own. See `docs/PLATFORM.md`. Native Wayland
  cannot register global shortcuts (`shortcutToggleSidebar` /
  `shortcutOpenDashboard`); bind a compositor key to the app instead.
- **macOS**: an extra Claude account (`accounts`) needs a `.credentials.json`
  in its `configDir`; the Keychain is only consulted for the primary account.
- **NVIDIA on Linux**: handled automatically (`WEBKIT_DISABLE_DMABUF_RENDERER=1`).
- **Blank or missing bar**: run with `AI_USAGE_SIDEBAR_LOG=debug` and read the log.

## 7. Uninstall

| Installed with | Remove with |
|---|---|
| `scripts/install-linux.sh` | `scripts/install-linux.sh --uninstall` |
| `.deb` / `.rpm` | `sudo apt remove ai-usage-sidebar` / `sudo dnf remove ai-usage-sidebar` |
| AppImage | delete the file |
| macOS | quit, delete `/Applications/AI Usage Sidebar.app` |
| Windows | Settings → Apps → AI Usage Sidebar → Uninstall |

Settings and the local usage database stay behind in the directories from §3
and §5. Delete them only if the user asks for a clean removal.
