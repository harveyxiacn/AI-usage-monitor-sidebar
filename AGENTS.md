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

## 4. Providers

The app shows whatever the official CLIs are already signed in to. It has no
login of its own.

- Claude Code: the user runs `claude` and signs in (`/login`).
- Codex: the user runs `codex login` (ChatGPT sign-in). API-key mode has no
  quota windows, so the ring shows "not signed in" — that is expected.
- Non-default locations are honoured through `CLAUDE_CONFIG_DIR` / `CODEX_HOME`.

A ring in an error state has its message in the popover and in the log.
`HTTP 429` from Anthropic means "polled too often"; it clears by itself.

## 5. Configure

Settings live in one JSON file. The app reads it **at start-up** and rewrites
it when the user changes something in the dashboard, so: quit the app (tray →
Quit, or kill the process), edit, start it again.

| Platform | Path |
|---|---|
| Linux | `~/.config/io.github.harveyxiacn.ai-usage-sidebar/settings.json` |
| macOS | `~/Library/Application Support/io.github.harveyxiacn.ai-usage-sidebar/settings.json` |
| Windows | `%APPDATA%\io.github.harveyxiacn.ai-usage-sidebar\settings.json` |

The file may be partial: missing keys take their defaults, unknown or invalid
values are ignored and numbers are clamped. Write only what the user asked for.

| Key | Values (default first) | Meaning |
|---|---|---|
| `language` | `"auto"`, `"en"`, `"zh-CN"` | UI and tray language |
| `theme` | `"dark"`, `"light"`, `"auto"` | |
| `surfaceStyle` | `"glass"`, `"solid"`, `"cyber"` | liquid glass (blurred backdrop where the OS has one), opaque, or a neon sci-fi HUD |
| `cyberAccent` | `"neon"`, `"matrix"`, `"amber"`, `"ice"`, `"synthwave"` | neon pair the `cyber` HUD is painted with; ignored by the other surfaces |
| `edge` | `"right"`, `"left"` | screen edge (also set by dragging) |
| `verticalAlign` | `"center"`, `"top"`, `"bottom"` | |
| `verticalOffset` | `0` (px, positive = down) | also set by dragging |
| `monitor` | `null` = primary, or a monitor name | |
| `autoHide` | `false`, `true` | collapse to a thin handle when idle |
| `autoHideDelayMs` | `800` | |
| `ringMode` | `"concentric"`, `"primary"`, `"all"` | one ring per provider, or one per window |
| `percentMode` | `"used"`, `"remaining"` | |
| `showPercentLabel` | `true` | |
| `refreshIntervalSec` | `60` | quota polling period |
| `providers` | `{"claude":{"enabled":true,"order":0},"codex":{"enabled":true,"order":1}}` | hide or reorder a provider |
| `thresholds` | `{"warn":70,"critical":90}` | ring colour and notifications |
| `notifications` | `false`, `true` | warn when a window crosses a threshold |
| `hideAccountEmail` | `false`, `true` | mask account e-mails as `h•••@g•••.com` (screenshots, screen sharing) |
| `autostart` | `false`, `true` | start at login |
| `alwaysOnTop` | `true` | |
| `opacity` | `1` (0.3 – 1) | |
| `scale` | `1` (0.75 – 1.5) | |

Example — Chinese UI, left edge, auto-hide, start at login:

```json
{ "language": "zh-CN", "edge": "left", "autoHide": true, "autostart": true }
```

Check the defaults above against `src-tauri/src/model.rs` (`impl Default for
Settings`) if the user needs an exact value; that file is authoritative.

## 6. Platform notes

- **GNOME on Wayland**: the app runs through XWayland on purpose (GNOME cannot
  position or keep-above native Wayland windows). The tray icon needs the
  AppIndicator extension:
  `https://extensions.gnome.org/extension/615/appindicator-support/`.
- **KDE / Hyprland / Sway**: native docking is opt-in with
  `AI_USAGE_SIDEBAR_BACKEND=wayland` and needs `gtk-layer-shell` installed; it
  falls back to XWayland on its own. See `docs/PLATFORM.md`.
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
