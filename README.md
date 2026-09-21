# AI Usage Sidebar

> An always-on-top edge sidebar that shows your **Claude Code** and **OpenAI Codex**
> rate-limit quotas at a glance — one ring per provider, a detail popover on hover,
> and a dashboard with your local token-usage history.

[中文说明见下 ↓](#ai-使用量侧边栏)

---

## Screenshots

Real captures from CachyOS / GNOME (XWayland), dark theme, liquid-glass surface:

<p align="center">
  <img src="docs/screenshots/sidebar.png" alt="Edge sidebar with concentric rings" height="320">
  <img src="docs/screenshots/popover.png" alt="Claude popover with 5-hour and weekly windows" height="320">
</p>
<p align="center">
  <img src="docs/screenshots/dashboard-history.png" alt="Dashboard: token history and provider comparison" width="48%">
  <img src="docs/screenshots/dashboard-settings.png" alt="Dashboard: settings" width="48%">
</p>

## What it is

`AI Usage Sidebar` is a small desktop widget built with **Tauri 2 (Rust)** and
**SvelteKit / Svelte 5**. It lives on the edge of your screen, above other windows,
and answers one question without interrupting you: *how much of my Claude / Codex
quota is left, and when does it reset?*

* One **progress ring per provider** (or one per window in `ringMode: "all"`).
* Hovering a ring opens a **popover** with every rate-limit window — 5-hour,
  weekly, per-model — plus its reset time. The popover never steals keyboard focus.
* A **dashboard** window holds settings and the **token-usage history** parsed from
  the providers' own local session logs, so you can compare plans and providers
  over time (with an *indicative* API-price estimate).
* A **tray icon** to show/hide the bar, toggle auto-hide, refresh, open the
  dashboard or quit.

## Features

| | |
|---|---|
| Edge docking | Any of the four screen edges, positioned along it (start / centre / end) with a pixel offset, on any monitor. Top and bottom turn the bar into a horizontal strip |
| Drag to move | Drag the bar anywhere: it snaps to the nearest screen edge of the monitor you drop it on and remembers its position along that edge |
| Auto-hide | The bar collapses to a thin handle when you move the pointer away and expands on hover |
| Pinning | Click a ring to pin the popover open while you read it; click again to close it. A pinned popover closes by itself 8 s after the pointer left |
| Always on top | Re-asserted after every map on X11, visible on all workspaces |
| Glass surface | `surfaceStyle: "glass"` uses a real blurred backdrop where the OS has one (macOS vibrancy, Windows acrylic); `"solid"` turns it off |
| Cyber HUD | `surfaceStyle: "cyber"` — a neon sci-fi look: chamfered plate, scanlines, tick-mark rings with glowing arcs, segmented bars |
| HUD accents | `cyberAccent` repaints the HUD: `neon` (cyan/magenta), `matrix` green, `amber` CRT, `ice` white-blue, `synthwave` purple/pink |
| Themes | Dark / light / follow the system |
| i18n | English and 简体中文 (tray menu included) |
| History | Incremental ingestion of session logs into SQLite; per-model, per-day/-week/-month totals; native CSV save and clipboard copy |
| Activity heatmap | 26 weeks of local days (or a weekday × hour punch card); pick a day to narrow the range. Every cell is focusable and labelled |
| Session drill-down | Per-session totals — provider, project, first/last activity, duration, requests, tokens, models — sortable and exportable. Counters and identifiers only, never prompt or response text |
| Cost estimate | Optional API-equivalent price estimate, clearly labelled as a comparison indicator |
| Monthly budget | `monthlyBudgetUsd` draws the month-to-date *estimate* against your budget, with the percentage used and the pace. Estimates only — subscriptions do not bill per token |
| Autostart | Optional login item (`--hidden`) |
| Notifications | Optional warning when a window crosses your editable threshold |
| Usage forecast | "Runs out in ~40 min" / "On pace for 82 % at reset" from the recorded quota samples, a tick on the ring where the projection lands, and an optional notification when a window is on pace to run out early |
| Screen-share safe | `hideAccountEmail` masks account addresses as `h•••@g•••.com` wherever they appear |

## Providers

| Provider | Plan | Rate-limit windows shown |
|---|---|---|
| Claude Code (OAuth) | Pro | 5-hour session **+** weekly (all models) |
| Claude Code (OAuth) | Max 5× / Max 20× | 5-hour session **+** weekly, plus per-model weekly windows (e.g. Opus) when the API reports them |
| OpenAI Codex (ChatGPT login) | Plus | 5-hour **+** weekly |
| OpenAI Codex (ChatGPT login) | Pro / "prolite" | API-reported windows, including weekly-only responses |
| OpenAI Codex | Team / Business / Enterprise / Edu | whatever the API reports, classified by window length |
| OpenAI Codex | API-key mode (`auth_mode: "apikey"`) | no quota windows exist; the ring shows "not signed in" |

Window kinds are **detected from the API**, never assumed: a window is classified
by its `limit_window_seconds` (≤ 6 h → 5-hour, ~7 d → weekly, anything else →
"other"). A weekly window can be *primary*; plan names never decide which
windows are present. The table describes observed payloads, not guaranteed plan entitlements.

## How it reads your data

Everything happens on your machine. The app reads the credentials the official
CLIs already wrote, calls the same two endpoints the CLIs call, and parses the
session logs those CLIs leave on disk.

**Credentials**

| Provider | Location |
|---|---|
| Claude | `~/.claude/.credentials.json` (Linux, Windows) · macOS Keychain item `Claude Code-credentials` · override the directory with `CLAUDE_CONFIG_DIR` |
| Codex | `~/.codex/auth.json` · override the directory with `CODEX_HOME` |

**Endpoints**

| Provider | Request |
|---|---|
| Claude | `GET https://api.anthropic.com/api/oauth/usage` (and `/api/oauth/profile` for the plan label), `Authorization: Bearer …`, `anthropic-beta: oauth-2025-04-20` |
| Codex | `GET https://chatgpt.com/backend-api/wham/usage`, `Authorization: Bearer …`, `ChatGPT-Account-Id: …` |

**Local logs (token history, and a fallback when you are offline)**

| Provider | Location |
|---|---|
| Claude | `~/.claude/projects/**/*.jsonl` |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`, `~/.codex/archived_sessions/**` |

> **Privacy.** Your tokens never leave your machine except in the request to
> Anthropic's and OpenAI's own endpoints — the same ones `claude` and `codex`
> already talk to. There is no telemetry, no analytics and no third-party
> service. Session JSONL records are parsed locally; only usage counters,
> model/session identifiers and project paths are retained. Prompt and response
> text is not stored in the usage database or sent by this app.
>
> The one exception is opt-in and off by default: if *you* put an https URL in
> **Settings → Data → Pricing table URL**, the app downloads that price list
> once a day (see "Keeping prices up to date"). While the field is empty no
> third-party request is ever made.

## Install

> **Let your AI agent do it.** Tell Claude Code, Codex or any coding agent:
> *"Install and set up AI Usage Sidebar by following
> https://github.com/harveyxiacn/AI-usage-monitor-sidebar/blob/main/AGENTS.md"*.
> [`AGENTS.md`](AGENTS.md) covers platform detection, download or source build,
> verification, every setting and uninstalling.

### Releases

Download the installer for your platform from the
[latest release](https://github.com/harveyxiacn/AI-usage-monitor-sidebar/releases/latest):
`.deb` / `.rpm` / `.AppImage` (Linux x86_64), `.dmg` (macOS, Apple Silicon and
Intel), `-setup.exe` / `.msi` (Windows x64). These builds are unsigned: macOS
needs `xattr -dr com.apple.quarantine "/Applications/AI Usage Sidebar.app"`,
Windows shows a SmartScreen warning.

### Staying up to date

The app checks GitHub for a newer release once a day (Settings → Behaviour →
*Check for updates daily*, on by default) and never installs anything on its
own: a new version shows up as a tray item, a one-line banner in the dashboard
and an *Updates* row under Settings → About, where "Install and restart" is a
deliberate click. Copies installed from a `.deb`/`.rpm` or by
`scripts/install-linux.sh` are owned by the package manager, so they get a
link to the release page instead of an in-place install. Maintainers: see
[`docs/RELEASING.md`](docs/RELEASING.md). Draft manifests for AUR, Homebrew
and winget live in [`packaging/`](packaging/README.md).

### Build prerequisites

**CachyOS / Arch**

```sh
sudo pacman -S --needed webkit2gtk-4.1 libayatana-appindicator librsvg base-devel rustup nodejs pnpm
rustup default stable
```

**Debian / Ubuntu**

```sh
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \
  patchelf libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
  build-essential curl wget file libssl-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
corepack enable pnpm
```

**macOS** — Xcode command line tools (`xcode-select --install`), Rust
(`rustup`), Node 22+ and pnpm. macOS 11 or newer.

**Windows** — Visual Studio Build Tools (C++ desktop workload),
[WebView2 runtime](https://developer.microsoft.com/microsoft-edge/webview2/)
(pre-installed on Windows 11), Rust and Node 22+ / pnpm.

GNOME users on Linux also want the
[AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/)
for the tray icon.

**Any Linux, without a package** — `scripts/install-linux.sh` builds a release
binary and installs it for the current user (`~/.local/bin`, launcher entry and
icons, no root), so it can be started and pinned from the dock.
`--uninstall` removes it again; settings and history are kept.

## Build & run

```sh
pnpm install
pnpm tauri dev     # development, with hot reload
pnpm tauri build   # release bundles in src-tauri/target/release/bundle/
# Arch / CachyOS: linuxdeploy's strip step fails on current binutils, so build the
# AppImage with `NO_STRIP=true APPIMAGE_EXTRACT_AND_RUN=1 pnpm tauri build --bundles appimage`
```

Useful environment variables:

| Variable | Effect |
|---|---|
| `AI_USAGE_SIDEBAR_BACKEND=wayland` | Do **not** force XWayland (see below) |
| `AI_USAGE_SIDEBAR_LOG=debug` | Verbose logging, including every window placement |
| `AI_USAGE_SIDEBAR_DEVTOOLS=1` | Open the WebKit inspector in debug builds |
| `CLAUDE_CONFIG_DIR`, `CODEX_HOME` | Point the app at non-default CLI directories |

## Platform notes

* **Linux / GNOME Wayland (primary development target).** Wayland does not let a
  client position its own windows or keep them above others, so the app forces
  `GDK_BACKEND=x11` (XWayland) unless you set `AI_USAGE_SIDEBAR_BACKEND=wayland`.
  With the proprietary NVIDIA driver it also sets
  `WEBKIT_DISABLE_DMABUF_RENDERER=1`, without which WebKitGTK renders black.
* **KDE / wlroots (Hyprland, Sway).** Native Wayland support through the
  `wlr-layer-shell` protocol is planned; until then run it on XWayland too.
  Linux has no client-side blur on GNOME, so the glass surface is the webview's
  own translucent fill; KWin blur-behind is on the roadmap.
* **macOS.** Builds and runs (Intel + Apple Silicon); the overlay windows use
  `macOSPrivateApi` for transparency and a real `NSVisualEffectView` backdrop
  (HUD material) when `surfaceStyle` is `glass`. Not yet notarised — right-click
  → *Open* on first launch.
* **Windows.** Builds and runs, with the DWM acrylic backdrop for the glass
  surface; the tray icon takes the left-click to toggle the dashboard.
  Considered beta.

Details, including the geometry maths and the hover state machine, are in
[`docs/PLATFORM.md`](docs/PLATFORM.md). The module and command contracts are in
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Keeping prices up to date

The cost column is an *estimate* for comparing providers and plans —
subscriptions do not bill per token. Prices come from a built-in table
(checked 2026-09-20). Model names drift fast, so:

* A model the table does not know is priced from its **closest known family**
  (`gpt-5.3-codex-spark` → `gpt-5.3-codex`, `claude-opus-4-99` →
  `claude-opus-4`). Those numbers are approximate and the dashboard says so.
  A model from an unrelated family stays unpriced and shows "—".
* You can edit any row, add your own prefixes and delete rows in
  **Settings → Data**. Your table wins over everything else.
* **Optional, off by default:** put an `https://` URL in **Pricing table URL**
  and the app refreshes the built-in list from it at most once a day (plus a
  "Refresh prices now" button). The download is capped in size, validated
  strictly and cached in the app data directory; anything unexpected is
  rejected and the bundled table keeps being used.

The file must use the same schema as [`pricing.json`](pricing.json) in this
repository, which you can host yourself:

```json
{
  "updatedAt": "2026-09-20T00:00:00Z",
  "entries": [
    { "modelPattern": "gpt-5.3-codex", "inputPerM": 1.75, "outputPerM": 14.0,
      "cacheWritePerM": 1.75, "cacheReadPerM": 0.175 }
  ]
}
```

Rates are USD per 1M tokens. To follow this project's own list:

```
https://raw.githubusercontent.com/harveyxiacn/AI-usage-monitor-sidebar/main/pricing.json
```

## Where your configuration lives

| | Linux | macOS | Windows |
|---|---|---|---|
| Settings | `~/.config/io.github.harveyxiacn.ai-usage-sidebar/settings.json` | `~/Library/Application Support/io.github.harveyxiacn.ai-usage-sidebar/settings.json` | `%APPDATA%\io.github.harveyxiacn.ai-usage-sidebar\settings.json` |
| Database & cache | `~/.local/share/io.github.harveyxiacn.ai-usage-sidebar/usage.db` | `~/Library/Application Support/io.github.harveyxiacn.ai-usage-sidebar/usage.db` | `%APPDATA%\io.github.harveyxiacn.ai-usage-sidebar\usage.db` |
| Logs | `~/.local/share/io.github.harveyxiacn.ai-usage-sidebar/logs/` | `~/Library/Logs/io.github.harveyxiacn.ai-usage-sidebar/` | `%APPDATA%\io.github.harveyxiacn.ai-usage-sidebar\logs\` |

Deleting `settings.json` resets the app to its defaults (right edge, vertically
centred, always visible, dark theme).

## Roadmap

- [ ] `wlr-layer-shell` support for KDE / Hyprland / Sway (true native Wayland)
- [ ] Blur behind the bar on KWin (`_KDE_NET_WM_BLUR_BEHIND_REGION`)
- [ ] More providers (Gemini CLI, GitHub Copilot, Cursor)
- [ ] Menu-bar mode on macOS
- [x] Native CSV export and clipboard copy
- [ ] Per-project token breakdown
- [ ] Notarised macOS builds and a signed Windows installer
- [ ] Optional burn-rate forecast ("at this rate your weekly window runs out on …")

## Contributing

Issues and pull requests are welcome. Please read `docs/ARCHITECTURE.md` first —
it is the contract for module boundaries, shared types, commands and events.
Before opening a PR:

```sh
pnpm check
pnpm test
pnpm exec playwright install chromium
pnpm test:e2e
cd src-tauri && cargo fmt --all -- --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked
```

Never paste real tokens into an issue; redact `accessToken` / `refresh_token`
from any log you attach.

See [validation notes](docs/VALIDATION.md) for regression coverage and native
platform limitations, and [pricing assumptions](docs/ARCHITECTURE.md#9-cost-estimation)
for the sources and limits of cost estimates. Unknown model prices display `—`;
mixed totals do not silently omit those costs. The browser preview is marked
as sample data and never reads desktop credentials.

## License

MIT © Harvey Xia. See [LICENSE](LICENSE).

---

# AI 使用量侧边栏

> 一个常驻屏幕边缘、置顶显示的小挂件，一眼看清 **Claude Code** 与 **OpenAI Codex**
> 的额度用量：每个服务一个进度环，悬停弹出详情，仪表盘里还有本地 token 使用历史。

## 截图

在 CachyOS / GNOME（XWayland）上的真实截图，深色主题、液态玻璃表面：

<p align="center">
  <img src="docs/screenshots/sidebar.png" alt="贴边侧栏与同心环" height="320">
  <img src="docs/screenshots/popover.png" alt="Claude 详情气泡：5 小时与每周额度" height="320">
</p>
<p align="center">
  <img src="docs/screenshots/dashboard-history.png" alt="仪表盘：Token 历史与供应商对比" width="48%">
  <img src="docs/screenshots/dashboard-settings.png" alt="仪表盘：设置" width="48%">
</p>

## 这是什么

`AI Usage Sidebar` 用 **Tauri 2（Rust）** + **SvelteKit / Svelte 5** 编写，
常驻屏幕边缘、置顶于其他窗口之上，只回答一个问题：*我的 Claude / Codex 额度
还剩多少、什么时候重置？*

* 每个服务一个**进度环**（`ringMode: "all"` 时每个限额窗口一个环）。
* 悬停某个环会弹出**详情层**，列出全部限额窗口——5 小时、每周、按模型——以及各自的重置时间。弹层**不会抢走键盘焦点**。
* **仪表盘**窗口提供设置，以及从各 CLI 本地会话日志解析出来的 **token 使用历史**，方便横向比较不同套餐与服务（并给出仅供参考的 API 等价费用估算）。
* **托盘图标**：显示/隐藏侧边栏、切换自动隐藏、立即刷新、打开仪表盘、退出。

## 功能

| | |
|---|---|
| 边缘吸附 | 四条屏幕边缘任选，沿边缘对齐（起始/居中/末端）并可设像素偏移，可指定显示器；贴靠顶部或底部时侧栏会变成横条 |
| 拖拽移动 | 直接拖动侧栏：松手后吸附到所在显示器最近的一条边缘，并记住沿该边缘的位置 |
| 自动隐藏 | 鼠标离开后收起为细条，悬停时自动展开 |
| 固定弹层 | 点击环可固定弹层，方便慢慢看；再次点击立即关闭。固定的弹层在鼠标离开 8 秒后也会自动关闭 |
| 始终置顶 | X11 下每次映射后重新置顶，并在所有工作区可见 |
| 玻璃质感 | `surfaceStyle: "glass"` 在系统支持时使用原生毛玻璃背景（macOS vibrancy、Windows acrylic）；`"solid"` 关闭 |
| 赛博 HUD | `surfaceStyle: "cyber"`：霓虹科幻风——切角面板、扫描线、刻度式圆环与发光用量弧、分段进度条 |
| HUD 霓虹配色 | `cyberAccent` 切换 HUD 配色：`neon`（青/品红）、`matrix` 绿、`amber` 琥珀 CRT、`ice` 冰蓝、`synthwave` 紫/粉 |
| 主题 | 深色 / 浅色 / 跟随系统 |
| 多语言 | English 与简体中文（含托盘菜单） |
| 历史 | 增量解析会话日志入 SQLite，支持按模型、按日/周/月统计，以及原生 CSV 保存与复制 |
| 费用估算 | 可选的 API 等价价格估算，界面明确标注仅作横向参考 |
| 开机自启 | 可选登录项（带 `--hidden` 参数） |
| 通知 | 额度超过可编辑阈值时可选提醒 |
| 用量预测 | 依据已记录的额度样本给出「约 40 分钟后用尽」「重置时将达 82%」，在圆环上标出预计落点，并可在预计提前用尽时通知 |
| 屏幕共享友好 | `hideAccountEmail` 将账号邮箱在所有位置显示为 `h•••@g•••.com` |

## 支持的服务与套餐

| 服务 | 套餐 | 显示的限额窗口 |
|---|---|---|
| Claude Code（OAuth） | Pro | 5 小时会话 **+** 每周（全模型） |
| Claude Code（OAuth） | Max 5× / Max 20× | 5 小时会话 **+** 每周，API 返回时还包括按模型（如 Opus）的每周窗口 |
| OpenAI Codex（ChatGPT 登录） | Plus | 5 小时 **+** 每周 |
| OpenAI Codex（ChatGPT 登录） | Pro / prolite | 以 API 返回为准，兼容仅返回每周窗口的情况 |
| OpenAI Codex | Team / Business / Enterprise / Edu | 以 API 返回为准，按窗口时长归类 |
| OpenAI Codex | API Key 模式（`auth_mode: "apikey"`） | 不存在额度窗口，环显示“未登录” |

窗口类型一律**由 API 返回值判断**，不做假设：按 `limit_window_seconds` 归类
（≤ 6 小时 → 5 小时窗口，约 7 天 → 每周窗口，其余 → 其他）。每周窗口也可能是
*primary*，程序不按套餐名硬编码窗口数量。上表是已观察到的数据形态，不承诺套餐权益。

## 数据从哪里来

全部在本机完成。应用读取官方 CLI 已经写好的凭据，调用 CLI 同样调用的两个接口，
并解析这些 CLI 留在磁盘上的会话日志。

**凭据**

| 服务 | 位置 |
|---|---|
| Claude | `~/.claude/.credentials.json`（Linux、Windows）· macOS 钥匙串条目 `Claude Code-credentials` · 可用 `CLAUDE_CONFIG_DIR` 覆盖目录 |
| Codex | `~/.codex/auth.json` · 可用 `CODEX_HOME` 覆盖目录 |

**接口**

| 服务 | 请求 |
|---|---|
| Claude | `GET https://api.anthropic.com/api/oauth/usage`（套餐名另取 `/api/oauth/profile`），请求头 `Authorization: Bearer …`、`anthropic-beta: oauth-2025-04-20` |
| Codex | `GET https://chatgpt.com/backend-api/wham/usage`，请求头 `Authorization: Bearer …`、`ChatGPT-Account-Id: …` |

**本地日志（token 历史；离线时也作为额度兜底）**

| 服务 | 位置 |
|---|---|
| Claude | `~/.claude/projects/**/*.jsonl` |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`、`~/.codex/archived_sessions/**` |

> **隐私说明**：除了发往 Anthropic 与 OpenAI 自家接口（也就是 `claude`、`codex`
> 本来就会访问的那两个）之外，你的 token 不会离开本机。没有遥测、没有统计上报、
> 没有任何第三方服务。会话 JSONL 记录在本机解析，仅保留用量计数、模型与会话标识、项目路径；
> 提示词和回复正文不会存入用量数据库，也不会由本应用发送出去。
>
> 唯一的例外需要你自己开启，默认关闭：只有当你在**设置 → 数据 → 价格表地址**里
> 填入一个 https 地址时，应用才会每天最多拉取一次该价格表（见“让价格保持最新”）。
> 该字段为空时，不会向任何第三方发起请求。

## 安装

> **让 AI agent 代劳。** 对 Claude Code、Codex 或任意编码 agent 说：
> *“按照 https://github.com/harveyxiacn/AI-usage-monitor-sidebar/blob/main/AGENTS.md
> 安装并设置 AI Usage Sidebar”*。
> [`AGENTS.md`](AGENTS.md) 涵盖平台识别、下载或源码构建、启动验证、全部设置项与卸载。

### 下载安装包

从[最新发布](https://github.com/harveyxiacn/AI-usage-monitor-sidebar/releases/latest)下载对应平台的安装包：
Linux x86_64 的 `.deb` / `.rpm` / `.AppImage`，macOS 的 `.dmg`（Apple Silicon 与 Intel 各一个），
Windows x64 的 `-setup.exe` / `.msi`。这些构建尚未签名：macOS 需执行
`xattr -dr com.apple.quarantine "/Applications/AI Usage Sidebar.app"`，Windows 会出现 SmartScreen 提示。

### 保持更新

应用每天检查一次 GitHub 上是否有新版本（设置 → 行为 → *每天检查更新*，默认开启），
但从不自动安装：有新版本时，托盘菜单、仪表盘顶部的一行提示以及“设置 → 关于 → 更新”
都会显示，安装始终需要你点击“安装并重启”。通过 `.deb` / `.rpm` 或
`scripts/install-linux.sh` 安装的副本由包管理器接管，只会给出发布页链接，不做原地替换。
维护者请看 [`docs/RELEASING.md`](docs/RELEASING.md)；AUR、Homebrew 与 winget 的打包草稿在
[`packaging/`](packaging/README.md)。

### 自行构建所需依赖

**CachyOS / Arch**

```sh
sudo pacman -S --needed webkit2gtk-4.1 libayatana-appindicator librsvg base-devel rustup nodejs pnpm
rustup default stable
```

**Debian / Ubuntu**

```sh
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \
  patchelf libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
  build-essential curl wget file libssl-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
corepack enable pnpm
```

**macOS**：Xcode 命令行工具（`xcode-select --install`）、Rust（`rustup`）、
Node 22+ 与 pnpm，系统需 macOS 11 及以上。

**Windows**：Visual Studio 生成工具（C++ 桌面开发）、
[WebView2 运行时](https://developer.microsoft.com/microsoft-edge/webview2/)（Win11 已内置）、
Rust、Node 22+ 与 pnpm。

Linux 上使用 GNOME 的用户还需要安装
[AppIndicator 扩展](https://extensions.gnome.org/extension/615/appindicator-support/)
才能看到托盘图标。

**任意 Linux、免打包安装**：`scripts/install-linux.sh` 会构建发布版并安装到当前用户
（`~/.local/bin`、启动器条目与图标，无需 root），之后即可从 dock 启动并固定。
`--uninstall` 可卸载，设置与历史数据会保留。

## 构建与运行

```sh
pnpm install
pnpm tauri dev     # 开发模式，带热更新
pnpm tauri build   # 产物在 src-tauri/target/release/bundle/
# Arch / CachyOS：linuxdeploy 的 strip 步骤在新版 binutils 上会失败，打 AppImage 请用
# `NO_STRIP=true APPIMAGE_EXTRACT_AND_RUN=1 pnpm tauri build --bundles appimage`
```

常用环境变量：

| 变量 | 作用 |
|---|---|
| `AI_USAGE_SIDEBAR_BACKEND=wayland` | **不**强制走 XWayland（见下） |
| `AI_USAGE_SIDEBAR_LOG=debug` | 输出详细日志，含每一次窗口定位 |
| `AI_USAGE_SIDEBAR_DEVTOOLS=1` | debug 构建下打开 WebKit 调试器 |
| `CLAUDE_CONFIG_DIR`、`CODEX_HOME` | 指定非默认的 CLI 配置目录 |

## 平台说明

* **Linux / GNOME Wayland（主要开发环境）**：Wayland 不允许客户端自行定位窗口或强制置顶，
  因此程序默认强制 `GDK_BACKEND=x11`（走 XWayland），除非设置
  `AI_USAGE_SIDEBAR_BACKEND=wayland`。在 NVIDIA 闭源驱动下还会设置
  `WEBKIT_DISABLE_DMABUF_RENDERER=1`，否则 WebKitGTK 会渲染成黑屏。
* **KDE / wlroots（Hyprland、Sway）**：计划通过 `wlr-layer-shell` 协议支持原生 Wayland；
  在此之前同样建议走 XWayland。GNOME 下没有客户端模糊接口，玻璃质感只能由网页层自行半透明绘制；
  KWin 的 blur-behind 已列入路线图。
* **macOS**：可构建可运行（Intel 与 Apple Silicon），透明窗口依赖 `macOSPrivateApi`；
  `surfaceStyle` 为 `glass` 时使用原生 `NSVisualEffectView`（HUD 材质）。
  尚未公证，首次启动请右键 → *打开*。
* **Windows**：可构建可运行，玻璃质感使用 DWM acrylic；托盘左键点击用于切换仪表盘。目前视为 beta。

更多细节（几何计算、悬停状态机）见 [`docs/PLATFORM.md`](docs/PLATFORM.md)；
模块与命令契约见 [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)。

## 让价格保持最新

费用一栏只是用于横向比较服务与套餐的**估算值**——订阅制并不按 token 计费。价格来自
内置价目表（2026-09-20 核对）。模型名字更新很快，所以：

* 表里没有的模型会按**最接近的同系列模型**计价（`gpt-5.3-codex-spark` →
  `gpt-5.3-codex`，`claude-opus-4-99` → `claude-opus-4`）。这类数字是近似值，
  仪表盘会明确标注；完全不沾边的模型仍然显示“—”。
* 在**设置 → 数据**里可以改价、添加自己的前缀、删除行；你保存的表优先级最高。
* **可选，默认关闭**：在**价格表地址**里填入 `https://` 地址后，应用每天最多从该地址
  更新一次内置价目表（也可以点“立即更新价格”）。下载有体积上限、经过严格校验，
  并缓存在应用数据目录；只要有任何异常就整份丢弃，继续用内置表。

文件格式与本仓库的 [`pricing.json`](pricing.json) 一致，你也可以自己托管一份：

```json
{
  "updatedAt": "2026-09-20T00:00:00Z",
  "entries": [
    { "modelPattern": "gpt-5.3-codex", "inputPerM": 1.75, "outputPerM": 14.0,
      "cacheWritePerM": 1.75, "cacheReadPerM": 0.175 }
  ]
}
```

价格单位是每百万 token 的美元数。想直接跟随本项目维护的价目表：

```
https://raw.githubusercontent.com/harveyxiacn/AI-usage-monitor-sidebar/main/pricing.json
```

## 配置文件位置

| | Linux | macOS | Windows |
|---|---|---|---|
| 设置 | `~/.config/io.github.harveyxiacn.ai-usage-sidebar/settings.json` | `~/Library/Application Support/io.github.harveyxiacn.ai-usage-sidebar/settings.json` | `%APPDATA%\io.github.harveyxiacn.ai-usage-sidebar\settings.json` |
| 数据库与缓存 | `~/.local/share/io.github.harveyxiacn.ai-usage-sidebar/usage.db` | `~/Library/Application Support/io.github.harveyxiacn.ai-usage-sidebar/usage.db` | `%APPDATA%\io.github.harveyxiacn.ai-usage-sidebar\usage.db` |
| 日志 | `~/.local/share/io.github.harveyxiacn.ai-usage-sidebar/logs/` | `~/Library/Logs/io.github.harveyxiacn.ai-usage-sidebar/` | `%APPDATA%\io.github.harveyxiacn.ai-usage-sidebar\logs\` |

删除 `settings.json` 即可恢复默认（右边缘、垂直居中、常驻显示、深色主题）。

## 路线图

- [ ] 支持 `wlr-layer-shell`（KDE / Hyprland / Sway 的原生 Wayland）
- [ ] KWin 下的背景模糊（`_KDE_NET_WM_BLUR_BEHIND_REGION`）
- [ ] 接入更多服务（Gemini CLI、GitHub Copilot、Cursor）
- [ ] macOS 菜单栏模式
- [x] 原生 CSV 导出与剪贴板复制
- [ ] 按项目统计 token
- [ ] 已公证的 macOS 包与已签名的 Windows 安装器
- [ ] 可选的消耗速率预测（“按此速度，你的每周额度将在 …… 用尽”）

## 参与贡献

欢迎提 Issue 和 PR。动手前请先读 `docs/ARCHITECTURE.md`——那是模块边界、共享类型、
命令与事件的契约。提交 PR 前请跑：

```sh
pnpm check
pnpm test
pnpm exec playwright install chromium
pnpm test:e2e
cd src-tauri && cargo fmt --all -- --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked
```

请勿在 Issue 中粘贴真实 token；附日志前请把 `accessToken` / `refresh_token` 打码。

回归测试覆盖范围和原生平台限制见[验证说明](docs/VALIDATION.md)；费用来源与估算限制见
[架构文档](docs/ARCHITECTURE.md#9-cost-estimation)。未知模型价格显示 `—`，汇总不会悄悄忽略未知费用。
浏览器预览明确标注示例数据，不读取桌面登录凭据。

## 许可证

MIT © Harvey Xia，详见 [LICENSE](LICENSE)。
