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
  linux.rs      Wayland layer-shell docking + the KDE X11 blur hint
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
absolute coordinates all work.

### Backend selection (`main.rs::linux_backend`)

XWayland is the default whenever an X display exists, on every desktop.
Native Wayland is **strictly opt-in**, in this order:

| Condition | `GDK_BACKEND` |
|---|---|
| `AI_USAGE_SIDEBAR_BACKEND=wayland` | `wayland` |
| `AI_USAGE_SIDEBAR_BACKEND=x11` | `x11` |
| the user already set `GDK_BACKEND` themselves | left untouched |
| no `DISPLAY`, but a `WAYLAND_DISPLAY` | `wayland` |
| anything else (including GNOME Wayland) | `x11` |

The pure decision function is unit tested (`cargo test --bin ai-usage-sidebar`).

### Native layer-shell docking (`window/linux.rs`)

Only when the step above left us on Wayland, `linux::prepare_backend()` runs —
still in `main`, before anything initialises GTK. It `dlopen`s
`libgtk-layer-shell.so.0` (so X11 and GNOME users never need the library
installed), initialises GTK and asks `gtk_layer_is_supported()`. If the library
is missing, too old or the compositor has no `zwlr_layer_shell_v1`, it
**falls back to XWayland**: by setting `GDK_BACKEND=x11` when GTK is not yet
initialised, otherwise by `exec()`ing the same binary again with
`GDK_BACKEND=x11` (GTK cannot change display after `gtk_init`;
`AI_USAGE_SIDEBAR_BACKEND_REEXEC=1` guards against a restart loop). A pure
Wayland session without `DISPLAY` has nothing to fall back to: the failure is
logged and the app starts anyway as an ordinary Wayland window that the
compositor places, exactly as before layer-shell existed.

`window::setup()` then calls `linux::initialize()` **before any overlay is
shown or realised** — `gtk_layer_init_for_window()` only works on an unrealised
window, which is why the overlays are `visible: false` in `tauri.conf.json`.
Any failure is logged at `info` and leaves the app on the ordinary
move/resize path; start-up is never aborted.

Once active (`window::layer_shell_active()`):

* `window::place_overlay()` — the single place a computed rectangle reaches the
  OS — routes to `linux::place()`, which sets the layer (`TOP` when
  `alwaysOnTop`, else `BOTTOM`), anchors to the configured edge plus the start
  of the axis the bar runs along (left/right → also top, top/bottom → also
  left), and translates the rectangle into **output-local** margins, then
  applies the documented `set_size_request()` + `resize(1, 1)` sequence.
  Margins are monitor-local logical px, so negative-origin and mixed-DPI
  layouts work. The top/bottom mapping is a straight extension of the
  left/right one and is covered by unit tests, but it has **not** been run on
  a layer-shell compositor.
* The exclusive zone is 0: the bar never reserves desktop space.
* Keyboard interactivity is `NONE` where the symbol exists
  (`gtk_layer_set_keyboard_mode` is only in gtk-layer-shell ≥ 0.6, so it is
  looked up optionally and a missing symbol does not fail the load).
* The target output is matched by `GdkMonitor::model()`, which is exactly what
  tao reports as `Monitor::name()` and therefore what `settings.monitor` holds.
* **The geometry watchdog is disabled.** A layer surface has no position of its
  own, so `outer_position()` is meaningless and a drift check could only
  re-place the bar in a loop. Settings changes and relayouts still place it.

Env vars: `AI_USAGE_SIDEBAR_BACKEND` (`wayland` | `x11`), `GDK_BACKEND`,
`AI_USAGE_SIDEBAR_BACKEND_REEXEC` (internal marker).

`window::backend_name()` reports what is actually in use and it ends up in
`AppInfo.backend` in the dashboard's *About* section.

The `native-smoke` cargo feature enables one extra test that `dlopen`s
`libgtk-layer-shell.so.0` and asserts every symbol we bind resolves, including
the ≥ 0.6 keyboard-mode one. It only makes sense on a machine that has the
library installed: `cargo test --features native-smoke window::linux`.

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
| Linux (X11/XWayland) | `linux::apply_blur()` sets `_KDE_NET_WM_BLUR_BEHIND_REGION` to an empty `CARDINAL` region ("blur the whole window"), which KWin honours and every other X11 compositor — mutter included — silently ignores. `solid` deletes the property. On a window that is still hidden the property is written from a one-shot `realize` handler |
| Linux (Wayland) | nothing: there is no client-side blur protocol, so the webview's own translucent fill is all you get |

The crate is only a dependency on macOS and Windows
(`[target.'cfg(any(target_os = "macos", target_os = "windows"))'.dependencies]`),
so Linux builds do not pull it in at all. Linux instead uses `gtk 0.18` (the
same version tauri/wry already link, verified with `cargo tree -i gtk`, so no
second GTK crate is compiled in) and `libloading 0.8`.

## 2. Geometry maths

Everything is computed in **logical pixels** (CSS px). The OS reports monitor
geometry in physical pixels; `MonitorRect::from_monitor` divides position and
size by the destination monitor's scale factor. Calculations use its usable
work area, avoiding reserved taskbars and docks. Before calling the OS, the
result is multiplied by that same destination scale and submitted as
`PhysicalPosition` / `PhysicalSize`, rounding only at this final boundary.
This avoids Tauri converting logical coordinates with the window's previous
monitor scale when moving between monitors. The pure functions live in
`monitors.rs` and are covered by unit tests (`cargo test window::`).

**Target monitor** — `settings.monitor` by name, else the primary monitor, else
the first monitor the OS reports.

**Edges and axes** — `edge` is one of `left | right | top | bottom`. Left and
right make the bar a vertical pill, top and bottom a horizontal strip. Every
formula below has an *across* axis (flush with the edge) and an *along* axis
(the one the bar runs along); `verticalAlign` and `verticalOffset` always
describe the position **along** the edge, whichever axis that is:
`top` = start of the span, `bottom` = end, offset positive towards the end.
The wire names are historical and deliberately unchanged, so settings files
written by older versions keep their exact meaning.

**Sidebar** — `sidebar_rect(mon, settings, content_w, content_h, expanded)`,
for a left/right edge:

```
width  = expanded ? content_w : settings.collapsedWidth      (clamped to mon.w)
height = content_h                                           (clamped to mon.h)
x      = edge == right ? mon.x + mon.w - width : mon.x
y      = align_start(verticalAlign, mon.y, mon.h, height) + settings.verticalOffset
y      = clamp(y, mon.y, mon.y + mon.h - height)
```

and, for a top/bottom edge, with the axes swapped:

```
height = expanded ? content_h : settings.collapsedWidth      (clamped to mon.h)
width  = content_w                                           (clamped to mon.w)
y      = edge == bottom ? mon.y + mon.h - height : mon.y
x      = align_start(verticalAlign, mon.x, mon.w, width) + settings.verticalOffset
x      = clamp(x, mon.x, mon.x + mon.w - width)
```

where `align_start` is `{ top: min, center: min + (span - len) / 2,
bottom: min + span - len }`. `collapsedWidth` is the *thickness* of the
collapsed handle, so on a horizontal edge it is its height.

`content_w/h` is what the webview measured and sent through `sidebar_relayout`;
until then the defaults `76 × 160` apply. Collapsing only changes the thickness,
so the bar keeps its position along the edge and the rings stay where they were.

**Popover** — `popover_rect(mon, sidebar, w, h, anchor, gap = 10, edge)`:

```
// left/right bar — open towards the screen centre, centred on the ring
open_left = sidebar.center_x >= mon.center_x
x = open_left ? sidebar.x - gap - w : sidebar.right + gap
y = sidebar.y + anchor - h / 2

// top/bottom bar — same, one quarter turn
open_up = sidebar.center_y >= mon.center_y
y = open_up ? sidebar.y - gap - h : sidebar.bottom + gap
x = sidebar.x + anchor - w / 2

x, y are then clamped inside the monitor
```

`anchor` is the ring centre in CSS px **relative to the sidebar window**, along
the bar's own axis — which is what the frontend can measure without knowing
anything about screen coordinates. It arrives as `PopoverRequest.anchorY` for a
vertical bar and `PopoverRequest.anchorX` for a horizontal one; the frontend
sends both, and `anchorX` is optional so requests from older frontends (which
only ever had a vertical bar) still deserialize. The bubble's tail is drawn by
the frontend at that anchor; because the popover is centred on the ring and
clamped afterwards, the frontend is also told the request it is rendering
(`popover-target`) and can offset the tail when clamping moved the window.

**When placement runs**

* on `apply_window_settings` (settings changed, from the dashboard or the tray),
* on `sidebar_relayout` / `popover_relayout` (content resized),
* on `sidebar_set_expanded` and on every popover show,
* and every 5 s from a watchdog that compares the window's real geometry with
  the computed one and re-places it when it drifted more than 2 px (monitor
  hot-plug, resolution change, a window manager that "helpfully" moved us) —
  skipped under Wayland layer-shell, see §1.

GTK on X11 sometimes keeps the pre-resize origin when a window is moved and
resized in the same frame, so the origin is written **before and after** the
resize. The watchdog catches whatever still slips through.

GTK also forces a non-resizable WebKit window to its natural minimum (200 CSS
px). On Linux, overlays therefore enable GTK resizing with equal minimum and
maximum size constraints matching the requested physical size. This permits
the thin collapsed handle while keeping user resizing disabled. The frontend
retains the last expanded dimensions when measuring the collapsed handle, so
collapse/expand does not change the bar's vertical center or restore width.

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
Changing auto-hide settings also invalidates existing timers and schedules
the current behavior immediately; disabling auto-hide expands the bar.
Hiding from the tray clears hover/pin state in both Rust and the frontend.
Native vibrancy updates are dispatched on the main thread for macOS AppKit.

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
| Check for updates / Update x.y.z available… | One item with two faces (`tray::update_label`): a manual `updater::check()` while nothing is on offer, and `open_dashboard(Some("settings"))` once there is |
| Quit | `app.exit(0)` |

Labels follow `settings.language` (English / 简体中文, `auto` reads `LANG`).
On macOS and Windows a left-click on the icon toggles the dashboard; on Linux
the AppIndicator protocol has no click event, so the menu is all there is.

## 4b. Global shortcuts (`window/shortcuts.rs`)

Because the overlays are dock windows (see the last section) they never take
keyboard focus, so nothing about the widget can be reached from the keyboard.
Two optional, desktop-wide shortcuts fill that gap:

| Setting | Action |
|---|---|
| `shortcutToggleSidebar` | `tray::toggle_sidebar` — shows/hides the bar window |
| `shortcutOpenDashboard` | `dashboard::open(None)` |

Both default to the **empty string**, which registers nothing: a widget has no
business taking a key combination the user did not ask it to take. They are
(re-)registered in `window::setup` and again from `apply_settings`, i.e. on
every `settings-updated`. `shortcuts::parse` refuses a combination without a
modifier — a bare `U` would be swallowed in every application on the desktop —
and whatever fails to parse or to register is reported through the
`get_shortcut_status` command and shown under the field in Settings.

**Where this works.** The plugin uses `global-hotkey`, which grabs keys with
`XGrabKey` on X11, `RegisterHotKey` on Windows and a Carbon event handler on
macOS. Linux runs on XWayland by default, so the X11 path applies and the
shortcut works. On a **native Wayland session** (`AI_USAGE_SIDEBAR_BACKEND=wayland`)
there is no protocol that lets an application grab a global key: some
compositors expose `org.freedesktop.portal.GlobalShortcuts`, several do not,
and the plugin does not use the portal. Treat global shortcuts as an X11
feature and bind the equivalent in the compositor's own keybinding
configuration on native Wayland.

macOS additionally requires Accessibility permission for some combinations,
and a shortcut another application already owns simply fails to register —
which is exactly what the settings field then says.

## 5. Windows and their lifecycle

| Label | Route | Notes |
|---|---|---|
| `sidebar` | `/` | frameless, transparent, no shadow, skip taskbar, sticky, `focus: false`, created hidden |
| `popover` | `/popover` | same, plus `set_focusable(false)`; hidden unless a ring is hovered or pinned |
| `dashboard` | `/dashboard` | normal decorated window, created hidden, min 880×600 |

`CloseRequested` is prevented on all three: the overlays cannot be closed at all
and the dashboard hides, so the app keeps living in the tray. A second launch
(single-instance plugin) restores the sidebar and focuses the dashboard instead
of starting a second bar. An already visible sidebar is hidden and shown once
to recover a stalled native surface, with placement and stacking restored.
Recovery is skipped while dragging. The display watchdog also remaps a visible
sidebar after monitor changes or a long scheduling pause; it leaves a bar
explicitly hidden from the tray hidden.

## 6. Known issues / field notes

* **Sidebar numbers and hover can freeze while the dashboard stays current.**
  Observed on Linux/XWayland: unmapping and remapping the same sidebar window,
  without restarting or requesting new quotas, immediately restored its current
  numbers. This identifies a native presentation/lifecycle stall; the underlying
  WebKit/compositor cause is not established. Reopening the application now
  performs that recovery. Cache reconciliation in the frontend separately
  repairs missed quota events; it cannot repair a frozen native surface.
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
* **Wayland mode (`AI_USAGE_SIDEBAR_BACKEND=wayland`)** docks natively through
  layer-shell where `zwlr_layer_shell_v1` and `libgtk-layer-shell.so.0` are
  both present (wlroots: Sway/Hyprland; KWin). Everywhere else it falls back
  to XWayland, or — with no X display at all — to a floating window at a
  compositor-chosen position, which is only useful for debugging rendering.
* **The layer-shell path has not been exercised on real hardware yet.** It was
  written and reviewed against `gtk-layer-shell.h`, but no wlroots or KWin
  session has run it. Unverified in particular: whether Tauri's overlays are
  still unrealised when `window::setup` runs on every wry version, the
  `set_size_request` + `resize(1, 1)` shrink sequence with a WebKit child,
  multi-output margins on a real mixed-DPI Wayland desktop, monitor hot-plug
  without the geometry watchdog, and whether `set_focusable(false)` plus
  keyboard mode `NONE` really keep the popover from taking focus.
* **macOS** windows are not notarised yet; `set_focusable(false)` cannot unfocus
  an already-focused window (an OS limitation), which is why the popover is made
  non-focusable *before* it is ever shown.
* **macOS full-screen Spaces.** tao's `set_visible_on_all_workspaces(true)`
  only sets `NSWindowCollectionBehaviorCanJoinAllSpaces`, which covers ordinary
  Spaces but not the Space another app creates when it goes full screen — the
  overlays vanished there. `window::apply_stacking` therefore ORs
  `NSWindowCollectionBehaviorFullScreenAuxiliary` (1 << 8) into the NSWindow's
  `collectionBehavior` on the main thread, through the `objc2-app-kit` that
  tao/wry already pull in. Compiled only in CI; not verified on real hardware
  by the author of that code.

## Overlay window type (X11)

The sidebar and the popover are `_NET_WM_WINDOW_TYPE_DOCK` windows, set in
`linux::mark_as_dock` before they are first mapped. As `NORMAL` toplevels they
were picked up by compositor add-ons that assume an opaque application window:
GNOME's *Rounded Window Corners* clips every normal window with its own shader
and puts a white shadow actor underneath, which turned the translucent bar into
a blank white pill (the X pixmap stayed correct, only the composited result was
wrong). Window animations and per-application blur make the same assumption and
skip docks as well. Geometry, `ABOVE` and `STICKY` are unaffected.
