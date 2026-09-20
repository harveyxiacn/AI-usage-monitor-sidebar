# Validation and remaining platform limits

The September 2026 stability pass covers the existing Claude/Codex sidebar,
popover and dashboard, with native CSV export. All automated data fixtures are
synthetic; tests do not require or print personal credentials or session logs.

## Repeatable checks

```sh
pnpm install --frozen-lockfile
pnpm check
pnpm test
pnpm exec playwright install chromium
pnpm test:e2e
pnpm build
cd src-tauri
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

To use an already installed Chromium for browser tests, set
`PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH` to its executable path. Unit tests do not
launch a server or browser. E2E uses an isolated localhost Vite server; on Linux
CI, `playwright install --with-deps chromium` supplies browser dependencies.
Failures retain screenshots and traces in `test-results/`.

Browser captures below use sample data, not real accounts:

![History with provider/model filtering and CSV export](screenshots/dashboard-history-preview.png)
![Chinese light-theme settings at the minimum dashboard size](screenshots/dashboard-settings-preview.png)

| Area | Evidence |
|---|---|
| UI and accessibility | Svelte/TypeScript checks; rendered dark English history and light Chinese settings reviewed at 1280×720 and 880×600 |
| History interactions | Browser tests filter by provider/model, validate dates, download and inspect CSV, exercise failed clipboard copy and missing-price feedback |
| Request ordering | Browser tests hold earlier successful/failed history requests until a newer request completes; final rows retain the newest filter |
| Settings | Unit tests cover rapid edits, failed writes, external window events, nested partial updates, persistence and notification order |
| Log ingestion | Rust fixtures cover append/rewrite/truncation, submillisecond timestamps, partial JSON/UTF-8, streaming duplicates and cumulative counters |
| Cost and quota integrity | Rust tests cover unknown-model aggregates, exact built-in prices, custom prefixes, editable table deletion, and excluding stale quota samples |
| Native geometry | Rust tests cover negative monitor origins, fractional/mixed DPI, work areas, clamping and hover timer cancellation |
| CSV file writing | Rust tests cover portable suggestions, Unicode/BOM, replacing an existing report, and preserving it when a write fails |
| Packaged apps | GitHub CI runs checks and builds installer artifacts on Ubuntu, macOS and Windows using locked dependencies |

## Linux native smoke check

The application was launched on the local XWayland desktop at 2× scale with
temporary XDG data/config directories and empty CLI credential directories.
The smoke check verified actual rendering, screen-edge placement, keep-above
and skip-taskbar flags, automatic collapse, hover expansion and popover,
second-instance dashboard activation, and close-to-hide behavior.

This caught GTK's 200 CSS-pixel natural minimum for non-resizable webviews.
The Linux implementation now uses equal minimum/maximum physical size hints:
the six CSS-pixel handle occupies twelve physical pixels at 2× scale, rather
than an invisible 400-pixel-wide window. Expanded dimensions are retained
when the frontend reports the collapsed handle's layout.

## What these checks do not establish

- Browser tests use mocks and do not validate live provider credentials or
  undocumented quota endpoints. Parser fixtures prove supported payload
  handling; providers can change those endpoints independently.
- macOS/Windows CI establishes native compilation, tests and installer
  generation, not hands-on behavior with every window manager, DPI setup,
  accessibility tool, permission prompt or graphics driver. Native save-dialog
  interaction still needs manual confirmation on each OS.
- Native Wayland positioning still requires the planned layer-shell work;
  XWayland is the supported docking path. Pure Wayland fallback opens the app
  but lets the compositor place its windows.
- macOS notarization and Windows signing require maintainer certificates and
  are not configured. CI artifacts are unsigned; no release is automatically
  published by a push to `main`.

The roadmap in the README tracks future additions separately from the current
Claude/Codex feature set.
