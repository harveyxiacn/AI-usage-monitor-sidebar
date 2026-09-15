# Platform layer — windows, geometry, hover

This document explains what `src-tauri/src/window/` does and why. It is the
companion of `docs/ARCHITECTURE.md` (which is the contract); this one is the
*rationale* and the field notes.

```
src-tauri/src/window/
  mod.rs        PlatformState, setup(), the #[tauri::command] entry points,
                stacking helpers, native translucency, autostart
  monitors.rs   monitor enumeration + the pure geometry maths (unit tested)
  sidebar.rs    placement, expand/collapse, first reveal, geometry watchdog
  popover.rs    anchoring, show without focus, relayout, pinning
  hover.rs      the hover state machine and its cancellable timers
  dashboard.rs  show / focus / navigate, close-to-hide
  tray.rs       tray icon menu (localised), autoHide toggle
```

## 1. X11 vs Wayland

An edge widget needs three things that Wayland deliberately does not give a
regular client:

1. **Absolute positioning.** `xdg_toplevel` has no "move my window to x,y".
2. **Stay on top.** There is no keep-above for ordinary toplevels.
3. **Visible on all workspaces.**

On wlroots compositors (Sway, Hyprland) and KDE those are available through the
`wlr-layer-shell` protocol, but GNOME/mutter does not implement it and has said
it will not. So on Linux `main.rs` sets `GDK_BACKEND=x11` **before GTK
initialises**, which makes the app an XWayland client where
`_NET_WM_STATE_ABOVE`, `_NET_WM_STATE_STICKY`, `_NET_WM_STATE_SKIP_TASKBAR` and
absolute coordinates all work. `AI_USAGE_SIDEBAR_BACKEND=wayland` opts out (and
the bar will then appear wherever the compositor feels like).

`window::backend_name()` reports what is actually in use and it ends up in
`AppInfo.backend` in the dashboard's *About* section.

Two more Linux quirks handled in `main.rs` / `window/mod.rs`:

* **NVIDIA proprietary driver + WebKitGTK** renders black unless
  `WEBKIT_DISABLE_DMABUF_RENDERER=1`. `main.rs` sets it when
  `/proc/driver/nvidia/version` exists.
* **mutter drops keep-above on map.** `window::after_show()` re-asserts
  `always_on_top` + `visible_on_all_workspaces` immediately after `show()` and
  once more 120 ms later, when the map has settled.

### Native translucency

`Settings.surfaceStyle` (`glass` | `solid`) asks for a real blurred backdrop
behind the overlay windows. `window::apply_surface_style()` implements it with
the `window-vibrancy` crate, cfg-gated:

| OS | Effect |
|---|---|
| macOS | `NSVisualEffectMaterial::HudWindow`, corner radius 16 |
| Windows | DWM acrylic, tint `(0, 0, 0, 10)` |
| Linux | nothing — mutter has no client-side blur; the webview's own translucent fill is all you get. `_KDE_NET_WM_BLUR_BEHIND_REGION` for KWin is on the roadmap |

The crate is only a dependency on macOS and Windows
(`[target.'cfg(any(target_os = "macos", target_os = "windows"))'.dependencies]`),
so Linux builds do not pull it in at all.

## 2. Geometry maths

Everything is computed in **logical pixels** (CSS px). The OS reports monitor
geometry in physical pixels; `MonitorRect::from_monitor` divides position and
size by the monitor's scale factor exactly once, and from then on the numbers
can be handed to `LogicalPosition` / `LogicalSize` verbatim. The pure functions
live in `monitors.rs` and are covered by unit tests (`cargo test window::`).

**Target monitor** — `settings.monitor` by name, else the primary monitor, else
the first monitor the OS reports.

**Sidebar** — `sidebar_rect(mon, settings, content_w, content_h, expanded)`:

```
width  = expanded ? content_w : settings.collapsedWidth      (clamped to mon.w)
height = content_h                                           (clamped to mon.h)
x      = edge == right ? mon.x + mon.w - width : mon.x
y      = { top:    mon.y
           center: mon.y + (mon.h - height) / 2
           bottom: mon.y + mon.h - height } + settings.verticalOffset
y      = clamp(y, mon.y, mon.y + mon.h - height)
```

`content_w/h` is what the webview measured and sent through `sidebar_relayout`;
until then the defaults `76 × 160` apply. Collapsing only changes the width, so
the bar keeps its vertical position and the rings stay where they were.

**Popover** — `popover_rect(mon, sidebar, w, h, anchor_y, gap = 10)`:

```
open_left = sidebar.center_x >= mon.center_x        // open towards screen centre
x = open_left ? sidebar.x - gap - w : sidebar.right + gap
y = sidebar.y + anchor_y - h / 2                    // centred on the hovered ring
x, y are then clamped inside the monitor
```

`anchor_y` is the ring centre in CSS px **relative to the sidebar window**, which
is what the frontend can measure without knowing anything about screen
coordinates. The bubble's tail is drawn by the frontend at `anchor_y`; because
the popover is vertically centred on the ring and clamped afterwards, the
frontend is also told the request it is rendering (`popover-target`) and can
offset the tail when clamping moved the window.

**When placement runs**

* on `apply_window_settings` (settings changed, from the dashboard or the tray),
* on `sidebar_relayout` / `popover_relayout` (content resized),
* on `sidebar_set_expanded` and on every popover show,
* and every 5 s from a watchdog that compares the window's real geometry with
  the computed one and re-places it when it drifted more than 2 px (monitor
  hot-plug, resolution change, a window manager that "helpfully" moved us).

GTK on X11 sometimes keeps the pre-resize origin when a window is moved and
resized in the same frame, so the origin is written **before and after** the
resize. The watchdog catches whatever still slips through.

**First reveal** — the sidebar window is created hidden. It is shown when the
frontend reports its first layout, or after 1.5 s, whichever comes first, so a
slow frontend can never leave an unpainted rectangle on screen.

## 3. Hover state machine

State lives in `PlatformState.inner`:
`{bar_hovered, popover_hovered, pinned, expanded, …}`.

```
hover_report("bar", true)      → cancel timers; expand if collapsed
hover_report(*, false)         → if neither window is hovered: start idle timers
popover_set_pinned(true)       → cancel timers
popover_set_pinned(false)      → start idle timers if the pointer is away

idle timers (one task):
  +250 ms                      → popover_hide()          unless pinned
  +autoHideDelayMs             → sidebar collapse        unless pinned / re-hovered,
                                                          and only if autoHide is on
```

Timers are `tauri::async_runtime::spawn` + `tokio::time::sleep`. There are no
`JoinHandle`s to keep: every hover transition bumps a **generation counter**, and
a task that wakes up with a stale generation returns without doing anything.
Before acting, a timer re-checks the whole condition (generation, pinned, both
hover flags), so a pointer that comes back during the delay always wins.

The popover is shown with `set_focusable(false)` and **never** `set_focus()` — a
status widget that steals focus from the editor is worse than no widget.

## 4. Tray

The icon comes from `tauri.conf.json` (`app.trayIcon`, id `main`,
`icons/tray.png` + `tray@2x.png`, a white ring glyph on transparency with
`iconAsTemplate: true` so macOS tints it). `tray::build()` only attaches the
menu:

| Item | Effect |
|---|---|
| Show/Hide sidebar | Hides or shows the whole bar window (different from collapse) |
| Always show sidebar | Check item; checked == `autoHide: false`. Writes through `commands::settings::set_auto_hide` |
| Refresh now | Emits `refresh-requested`, which the scheduler consumes |
| Open dashboard | `open_dashboard(None)` |
| Settings… | `open_dashboard(Some("settings"))` |
| Quit | `app.exit(0)` |

Labels follow `settings.language` (English / 简体中文, `auto` reads `LANG`).
On macOS and Windows a left-click on the icon toggles the dashboard; on Linux
the AppIndicator protocol has no click event, so the menu is all there is.

## 5. Windows and their lifecycle

| Label | Route | Notes |
|---|---|---|
| `sidebar` | `/` | frameless, transparent, no shadow, skip taskbar, sticky, `focus: false`, created hidden |
| `popover` | `/popover` | same, plus `set_focusable(false)`; hidden unless a ring is hovered or pinned |
| `dashboard` | `/dashboard` | normal decorated window, created hidden, min 880×600 |

`CloseRequested` is prevented on all three: the overlays cannot be closed at all
and the dashboard hides, so the app keeps living in the tray. A second launch
(single-instance plugin) focuses the dashboard instead of starting a second bar.

## 6. Known issues / field notes

* **A full-desktop screenshot is impossible under rootless XWayland.** The X root
  window is not viewable, so `import -window root` and `XGetImage` on the root
  both fail. Individual app windows can still be captured
  (`import -window <id>`), and `xprop` / `_NET_CLIENT_LIST` show the real
  geometry and state.
* **The XWayland scale factor can change between runs** (GNOME display settings,
  `GDK_SCALE`, fractional scaling). The maths adapts — the bar stays flush and
  centred — but the same content is physically smaller at scale 1. If the bar
  looks half-size, that is a DPI question for the frontend's `settings.scale`,
  not a placement bug.
* **Transparency needs a compositor.** Without one (bare X11, `picom` off) the
  rounded corners render black.
* **GNOME needs the AppIndicator extension** for the tray icon to appear at all.
* **`alwaysOnTop` fights full-screen windows.** Most WMs put full-screen windows
  above everything; that is intentional and not worked around.
* **Wayland mode (`AI_USAGE_SIDEBAR_BACKEND=wayland`)** currently gives a
  floating window at a compositor-chosen position. Useful for debugging
  rendering, not for daily use, until layer-shell lands.
* **macOS** windows are not notarised yet; `set_focusable(false)` cannot unfocus
  an already-focused window (an OS limitation), which is why the popover is made
  non-focusable *before* it is ever shown.
