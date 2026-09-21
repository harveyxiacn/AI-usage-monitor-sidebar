//! Native Linux overlay support. [PLATFORM]
//!
//! Two independent pieces live here:
//!
//! * **gtk-layer-shell docking** for compositors that implement
//!   `zwlr_layer_shell_v1` (wlroots: Sway/Hyprland, and KWin). GNOME/mutter
//!   does not, which is why `main.rs` keeps XWayland as the default; see
//!   [`prepare_backend`].
//! * **`_KDE_NET_WM_BLUR_BEHIND_REGION`** for KWin on X11, see [`apply_blur`].
//!
//! `libgtk-layer-shell.so.0` is opened with `dlopen` at runtime so X11 and
//! GNOME users never need the shared library installed at all, and a missing
//! or too old library can only ever downgrade us to the XWayland path.

use super::monitors::LogicalRect;
use crate::model::{windows, Edge, SurfaceStyle};
use gtk::{gdk, glib::translate::ToGlibPtr, prelude::*};
use libloading::Library;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};
use tauri::{AppHandle, Manager, WebviewWindow};

type GtkWindowPtr = *mut gtk::ffi::GtkWindow;
type GdkMonitorPtr = *mut gdk::ffi::GdkMonitor;
/// `gtk_layer_set_keyboard_mode(GtkWindow*, GtkLayerShellKeyboardMode)`.
type KeyboardModeFn = unsafe extern "C" fn(GtkWindowPtr, i32);

/// `GtkLayerShellEdge`.
const EDGE_LEFT: i32 = 0;
const EDGE_RIGHT: i32 = 1;
const EDGE_TOP: i32 = 2;
const EDGE_BOTTOM: i32 = 3;
/// `GtkLayerShellLayer`: BACKGROUND = 0, BOTTOM = 1, TOP = 2, OVERLAY = 3.
/// TOP sits above normal windows but below fullscreen ones, which matches the
/// X11 behaviour of `_NET_WM_STATE_ABOVE` that the widget already has.
const LAYER_BOTTOM: i32 = 1;
const LAYER_TOP: i32 = 2;
/// `GTK_LAYER_SHELL_KEYBOARD_MODE_NONE` — a status widget never takes input.
const KEYBOARD_MODE_NONE: i32 = 0;

const SONAME: &str = "libgtk-layer-shell.so.0";

/// Guards against an unbounded re-exec chain if the environment ever makes
/// [`prepare_backend`] want to restart twice.
const REEXEC_MARKER: &str = "AI_USAGE_SIDEBAR_BACKEND_REEXEC";

/// The subset of the gtk-layer-shell C ABI we use. Signatures follow
/// `gtk-layer-shell.h`; `gboolean` and every enum are plain `gint` (`i32`).
struct LayerApi {
    // Keep the library mapped for the entire lifetime of every layer surface.
    _library: Library,
    /// `gboolean gtk_layer_is_supported(void)` — since 0.5.
    supported: unsafe extern "C" fn() -> i32,
    init: unsafe extern "C" fn(GtkWindowPtr),
    is_layer: unsafe extern "C" fn(GtkWindowPtr) -> i32,
    set_namespace: unsafe extern "C" fn(GtkWindowPtr, *const std::ffi::c_char),
    set_layer: unsafe extern "C" fn(GtkWindowPtr, i32),
    set_monitor: unsafe extern "C" fn(GtkWindowPtr, GdkMonitorPtr),
    get_monitor: unsafe extern "C" fn(GtkWindowPtr) -> GdkMonitorPtr,
    /// `void gtk_layer_set_anchor(GtkWindow*, GtkLayerShellEdge, gboolean)`.
    set_anchor: unsafe extern "C" fn(GtkWindowPtr, i32, i32),
    /// `void gtk_layer_set_margin(GtkWindow*, GtkLayerShellEdge, int)`.
    set_margin: unsafe extern "C" fn(GtkWindowPtr, i32, i32),
    set_exclusive_zone: unsafe extern "C" fn(GtkWindowPtr, i32),
    /// Only exists since gtk-layer-shell 0.6. Older libraries simply never
    /// focus a layer surface that did not ask for keyboard interactivity, so
    /// the symbol is optional rather than fatal.
    set_keyboard: Option<KeyboardModeFn>,
}

static API: OnceLock<Result<LayerApi, String>> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);

fn load_api() -> Result<LayerApi, String> {
    // The system loader resolves the standard soname; no path ever comes from
    // the frontend or from settings.
    unsafe {
        let library = Library::new(SONAME).map_err(|e| format!("{SONAME}: {e}"))?;
        macro_rules! required {
            ($name:literal) => {{
                let symbol: libloading::Symbol<_> = library
                    .get(concat!($name, "\0").as_bytes())
                    .map_err(|e| format!("{}: {e}", $name))?;
                *symbol
            }};
        }
        Ok(LayerApi {
            supported: required!("gtk_layer_is_supported"),
            init: required!("gtk_layer_init_for_window"),
            is_layer: required!("gtk_layer_is_layer_window"),
            set_namespace: required!("gtk_layer_set_namespace"),
            set_layer: required!("gtk_layer_set_layer"),
            set_monitor: required!("gtk_layer_set_monitor"),
            get_monitor: required!("gtk_layer_get_monitor"),
            set_anchor: required!("gtk_layer_set_anchor"),
            set_margin: required!("gtk_layer_set_margin"),
            set_exclusive_zone: required!("gtk_layer_set_exclusive_zone"),
            set_keyboard: library
                .get::<KeyboardModeFn>(b"gtk_layer_set_keyboard_mode\0")
                .ok()
                .map(|symbol| *symbol),
            _library: library,
        })
    }
}

fn layer_api() -> Result<&'static LayerApi, &'static str> {
    API.get_or_init(load_api).as_ref().map_err(String::as_str)
}

/// True once [`initialize`] has really turned the overlays into layer
/// surfaces. While this is false every caller must use the ordinary
/// move/resize path.
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Called on the main thread from `main.rs`, **before** anything initialises
/// GTK. A no-op unless `GDK_BACKEND=wayland`, i.e. unless the user opted into
/// the native path (see `linux_backend` in `main.rs`).
///
/// When the library or the compositor cannot provide layer-shell we prefer
/// XWayland, because an edge widget that silently becomes a floating window
/// the user cannot place is worse than one running through XWayland. If there
/// is no X display to fall back to, this reports an error; the caller logs it
/// and continues, which leaves the historical "floating Wayland window"
/// behaviour intact for pure-Wayland sessions.
pub fn prepare_backend() -> Result<(), String> {
    if std::env::var("GDK_BACKEND").as_deref() != Ok("wayland") {
        return Ok(());
    }
    let reason = match layer_api() {
        Err(error) => Some(format!("GTK layer-shell is unavailable: {error}")),
        Ok(api) => match gtk::init() {
            Err(e) => Some(format!("could not open the Wayland display: {e}")),
            // SAFETY: GTK is initialised and this runs on the main thread.
            Ok(()) if unsafe { (api.supported)() } != 0 => None,
            Ok(()) => Some(
                "the compositor does not implement zwlr_layer_shell_v1 \
                 (GNOME/mutter needs XWayland)"
                    .to_string(),
            ),
        },
    };
    let Some(reason) = reason else { return Ok(()) };

    if std::env::var_os("DISPLAY").is_none() {
        return Err(format!(
            "native edge docking is unavailable: {reason}. Without an X display \
             the bar becomes an ordinary window wherever the compositor puts it; \
             install gtk-layer-shell or enable XWayland."
        ));
    }
    eprintln!("AI Usage Sidebar: {reason}; using XWayland for edge docking.");
    if !gtk::is_initialized() {
        std::env::set_var("GDK_BACKEND", "x11");
        return Ok(());
    }
    if std::env::var_os(REEXEC_MARKER).is_some() {
        return Err(format!(
            "native edge docking is unavailable: {reason}, and restarting with \
             XWayland already happened once."
        ));
    }
    // GTK cannot switch displays once it is initialised, so replace this
    // process — before any plugin, window or state exists — with the same
    // executable and arguments on the X11 backend.
    use std::os::unix::process::CommandExt;
    let error = std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .args(std::env::args_os().skip(1))
        .env("AI_USAGE_SIDEBAR_BACKEND", "x11")
        .env("GDK_BACKEND", "x11")
        .env(REEXEC_MARKER, "1")
        .exec();
    Err(format!("could not restart with XWayland: {error}"))
}

/// Turn the overlay windows into layer surfaces. Must run on the main thread
/// from `window::setup`, before anything shows or realises them: GTK
/// layer-shell can only take over a window that has not been realised yet.
///
/// Returns `Ok(())` without doing anything when the session is not Wayland;
/// every error leaves [`is_active`] false, so the caller can log and keep the
/// ordinary XWayland/X11 behaviour.
pub fn initialize(app: &AppHandle) -> anyhow::Result<()> {
    let Some(display) = gdk::Display::default() else {
        return Ok(());
    };
    if display.type_().name() != "GdkWaylandDisplay" {
        return Ok(());
    }
    let api = layer_api().map_err(anyhow::Error::msg)?;
    // SAFETY: GTK is initialised (a GdkDisplay exists) and this is the main
    // thread; the symbol has the `gboolean (*)(void)` C ABI.
    anyhow::ensure!(
        unsafe { (api.supported)() } != 0,
        "the compositor does not support native layer-shell docking"
    );
    for label in [windows::SIDEBAR, windows::POPOVER] {
        let win = app
            .get_webview_window(label)
            .ok_or_else(|| anyhow::anyhow!("overlay `{label}` is missing"))?;
        let native = win.gtk_window()?;
        anyhow::ensure!(
            !native.is_realized(),
            "overlay `{label}` was realised before layer-shell initialisation"
        );
        let pointer = native.upcast_ref::<gtk::Window>().to_glib_none().0;
        // Hidden Tauri windows keep their original GTK/WebKit ownership; no
        // reparenting and no second unmanaged toplevel is involved.
        // SAFETY: `pointer` is a live GtkWindow owned by `native`, the window
        // is not realised, and we are on the GTK main thread.
        unsafe {
            (api.init)(pointer);
            (api.set_namespace)(pointer, c"ai-usage-sidebar".as_ptr());
            if let Some(set_keyboard) = api.set_keyboard {
                set_keyboard(pointer, KEYBOARD_MODE_NONE);
            }
            (api.set_exclusive_zone)(pointer, 0); // never reserve desktop space
        }
        // SAFETY: as above.
        anyhow::ensure!(
            unsafe { (api.is_layer)(pointer) } != 0,
            "could not turn overlay `{label}` into a layer surface"
        );
        // GTK pins a non-resizable window to the webview's natural minimum
        // (200 px), which would make the collapsed handle impossible.
        native.set_resizable(true);
    }
    ACTIVE.store(true, Ordering::Relaxed);
    log::info!("native Wayland layer-shell docking enabled for the sidebar and the popover");
    Ok(())
}

/// The GDK monitor matching `settings.monitor`. tao reports
/// `gdk_monitor_get_model()` as `Monitor::name()`, which is what ends up in
/// the settings, so the same accessor is used here.
fn target_gdk_monitor(display: &gdk::Display, wanted: Option<&str>) -> Option<gdk::Monitor> {
    wanted
        .filter(|name| !name.is_empty())
        .and_then(|name| {
            (0..display.n_monitors())
                .filter_map(|i| display.monitor(i))
                .find(|monitor| monitor.model().as_deref() == Some(name))
        })
        .or_else(|| display.primary_monitor())
        .or_else(|| display.monitor(0))
}

/// Layer-shell margins are **monitor-local** logical pixels, never global
/// desktop coordinates, so this also works on negative-origin and mixed-DPI
/// layouts.
///
/// The surface is anchored to the docked edge plus the start of the edge it
/// runs along (left/right → also top; top/bottom → also left), so only two of
/// the four margins can ever be non-zero: the distance from the docked edge
/// and the distance along it. Returns `(left, right, top, bottom)`.
fn margins(monitor: LogicalRect, rect: LogicalRect, edge: Edge) -> (i32, i32, i32, i32) {
    let px = |v: f64| v.max(0.0).round() as i32;
    let (left, top) = (px(rect.x - monitor.x), px(rect.y - monitor.y));
    let (right, bottom) = (
        px(monitor.right() - rect.right()),
        px(monitor.bottom() - rect.bottom()),
    );
    match edge {
        Edge::Left => (left, 0, top, 0),
        Edge::Right => (0, right, top, 0),
        Edge::Top => (left, 0, top, 0),
        Edge::Bottom => (left, 0, 0, bottom),
    }
}

/// Which layer-shell anchors a docked edge needs: the edge itself plus the
/// start of the axis the bar runs along.
fn anchors(edge: Edge) -> (bool, bool, bool, bool) {
    (
        edge != Edge::Right,  // left
        edge == Edge::Right,  // right
        edge != Edge::Bottom, // top
        edge == Edge::Bottom, // bottom
    )
}

/// Place an overlay through layer-shell. Returns `false` when layer-shell is
/// not active, and the caller must then move/resize the window itself.
///
/// The work is dispatched to the GTK main thread: placement is triggered from
/// Tauri command handlers and from watchdog tasks on the async runtime, and
/// every GTK/GDK call below is main-thread-only.
pub fn place(win: &WebviewWindow, rect: LogicalRect) -> bool {
    if !is_active() {
        return false;
    }
    let settings = super::settings_of(win.app_handle());
    let native_window = win.clone();
    if let Err(error) = win.run_on_main_thread(move || {
        let (Ok(api), Ok(native), Some(display)) = (
            layer_api(),
            native_window.gtk_window(),
            gdk::Display::default(),
        ) else {
            return;
        };
        let Some(monitor) = target_gdk_monitor(&display, settings.monitor.as_deref()) else {
            return;
        };
        // GDK reports the work area in application (logical) pixels, the same
        // space `LogicalRect` uses.
        let area = monitor.workarea();
        let bounds = LogicalRect::new(
            area.x().into(),
            area.y().into(),
            area.width().into(),
            area.height().into(),
        );
        let (margin_left, margin_right, margin_top, margin_bottom) =
            margins(bounds, rect, settings.edge);
        let (anchor_left, anchor_right, anchor_top, anchor_bottom) = anchors(settings.edge);
        let pointer = native.upcast_ref::<gtk::Window>().to_glib_none().0;
        let monitor_pointer = monitor.to_glib_none().0;
        // SAFETY: live GtkWindow / GdkMonitor pointers, GTK main thread, and
        // the signatures above match `gtk-layer-shell.h`.
        unsafe {
            // Assigning a different output remaps a visible layer surface, so
            // only do it when it actually changed.
            if (api.get_monitor)(pointer) != monitor_pointer {
                (api.set_monitor)(pointer, monitor_pointer);
            }
            (api.set_layer)(
                pointer,
                if settings.always_on_top {
                    LAYER_TOP
                } else {
                    LAYER_BOTTOM
                },
            );
            (api.set_anchor)(pointer, EDGE_LEFT, i32::from(anchor_left));
            (api.set_anchor)(pointer, EDGE_RIGHT, i32::from(anchor_right));
            (api.set_anchor)(pointer, EDGE_TOP, i32::from(anchor_top));
            (api.set_anchor)(pointer, EDGE_BOTTOM, i32::from(anchor_bottom));
            (api.set_margin)(pointer, EDGE_LEFT, margin_left);
            (api.set_margin)(pointer, EDGE_RIGHT, margin_right);
            (api.set_margin)(pointer, EDGE_TOP, margin_top);
            (api.set_margin)(pointer, EDGE_BOTTOM, margin_bottom);
        }
        // The documented gtk-layer-shell resize sequence: a size request plus
        // a deliberately too small `resize()` lets the surface shrink again.
        // Unlike an xdg toplevel, the compositor honours the anchors/margins.
        native.set_size_request(
            rect.w.round().max(1.0) as i32,
            rect.h.round().max(1.0) as i32,
        );
        native.resize(1, 1);
    }) {
        log::warn!("layer-shell placement of `{}` failed: {error}", win.label());
    }
    true
}

/// Report "the pointer left" from GTK instead of from the DOM.
///
/// On XWayland, WebKitGTK delivers `mouseenter` and `mousemove` to the overlay
/// webviews but **never** `mouseleave` (verified in the hover debug log: not
/// one `false` report in either window, not even when the pointer moves from
/// the bar into the popover). The hover state machine then believes the
/// pointer is still there and neither the popover nor the auto-hide bar ever
/// goes away. `leave-notify-event` is the windowing system's own crossing
/// event and does not depend on WebKit's synthesised DOM event.
pub fn watch_pointer_leave(app: &AppHandle) {
    for (label, source) in [(windows::SIDEBAR, "bar"), (windows::POPOVER, "popover")] {
        let Some(win) = app.get_webview_window(label) else {
            continue;
        };
        let handle = app.clone();
        let result = win.with_webview(move |webview| {
            // The crossing event goes to the innermost GdkWindow: the web view.
            webview.inner().connect_leave_notify_event(move |_, event| {
                // `Inferior`: the pointer moved into a child window of ours.
                // A non-normal mode is a grab starting/ending (a drag), not a
                // real exit.
                if event.detail() != gdk::NotifyType::Inferior
                    && event.mode() == gdk::CrossingMode::Normal
                {
                    let handle = handle.clone();
                    // Never run window operations inside a GTK signal handler.
                    tauri::async_runtime::spawn(async move {
                        super::hover::report(&handle, source, false);
                    });
                }
                gtk::glib::Propagation::Proceed
            });
        });
        if let Err(e) = result {
            log::warn!("cannot watch pointer crossings on `{label}`: {e}");
        }
    }
}

/// Declare the overlays as `_NET_WM_WINDOW_TYPE_DOCK` on X11.
///
/// They were ordinary `NORMAL` toplevels, which is what every compositor
/// add-on targets: GNOME's "Rounded Window Corners" clips them with its own
/// shader and puts a *white* shadow actor underneath — invisible below an
/// opaque window, but it replaced our translucent bar with a blank white pill.
/// Window animations and per-application blur make the same assumption. A dock
/// is what the bar is, those add-ons skip it, and mutter neither focuses a dock
/// on click nor constrains its position. Must run before the window is mapped.
pub fn mark_as_dock(app: &AppHandle) {
    if is_active() {
        return; // a layer surface is not an xdg/X11 toplevel at all
    }
    for label in [windows::SIDEBAR, windows::POPOVER] {
        let Some(win) = app.get_webview_window(label) else {
            continue;
        };
        let Ok(native) = win.gtk_window() else {
            continue;
        };
        if native.display().type_().name() != "GdkX11Display" {
            continue;
        }
        if native.is_visible() {
            log::warn!("overlay {label} is already mapped, leaving its window type alone");
            continue;
        }
        native.set_type_hint(gdk::WindowTypeHint::Dock);
    }
}

/// KWin's X11 blur protocol: an empty `CARDINAL` region means "blur the whole
/// window". Other X11 compositors ignore the property, and `solid` removes it.
///
/// Wayland has no equivalent client-side protocol, so this is X11 only; under
/// layer-shell there is nothing to do.
pub fn apply_blur(win: &WebviewWindow, style: SurfaceStyle) {
    if is_active() {
        return;
    }
    let native_window = win.clone();
    // Settings arrive on an async worker; GDK is main-thread-only.
    if let Err(e) = win.run_on_main_thread(move || {
        let Ok(native) = native_window.gtk_window() else {
            return;
        };
        if native.display().type_().name() != "GdkX11Display" {
            return;
        }
        let apply = move |native: &gtk::ApplicationWindow| {
            let Some(surface) = native.window() else {
                return;
            };
            let atom = gdk::Atom::intern("_KDE_NET_WM_BLUR_BEHIND_REGION");
            match style {
                SurfaceStyle::Glass => gdk::property_change(
                    &surface,
                    &atom,
                    &gdk::Atom::intern("CARDINAL"),
                    32,
                    gdk::PropMode::Replace,
                    gdk::ChangeData::ULongs(&[]),
                ),
                SurfaceStyle::Solid | SurfaceStyle::Cyber => gdk::property_delete(&surface, &atom),
            }
        };
        if native.is_realized() {
            apply(&native);
        } else {
            // The overlays are created hidden, so the X window only exists
            // after the first `show()`.
            native.connect_realize(apply);
        }
    }) {
        log::debug!("scheduling the KDE blur hint on `{}`: {e}", win.label());
    }
}

/// Opt-in smoke test for machines that actually have gtk-layer-shell
/// installed: `cargo test --features native-smoke -- --ignored` is not needed,
/// the feature itself gates it. It only resolves symbols (no GTK, no display,
/// no window), which is exactly the part that cannot be checked on a machine
/// without the library.
#[cfg(all(test, feature = "native-smoke"))]
mod smoke {
    #[test]
    fn every_required_symbol_resolves() {
        let api = super::layer_api().expect("libgtk-layer-shell.so.0 with all required symbols");
        assert!(
            api.set_keyboard.is_some(),
            "gtk_layer_set_keyboard_mode is missing; the library predates 0.6"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::monitors::MonitorRect;

    #[test]
    fn anchors_are_output_local_on_mixed_dpi_desktops() {
        let monitor = MonitorRect::new("secondary", -1280.0, 24.0, 1280.0, 696.0, 2.0);
        let bar = LogicalRect::new(-76.0, 282.0, 76.0, 180.0);
        assert_eq!(margins(monitor.rect, bar, Edge::Right), (0, 0, 258, 0));
        let popover = LogicalRect::new(-426.0, 240.0, 340.0, 220.0);
        assert_eq!(margins(monitor.rect, popover, Edge::Right), (0, 86, 216, 0));
        assert_eq!(
            margins(
                monitor.rect,
                LogicalRect::new(-1194.0, 24.0, 340.0, 220.0),
                Edge::Left
            ),
            (86, 0, 0, 0)
        );
    }

    #[test]
    fn horizontal_edges_anchor_along_x() {
        let monitor = MonitorRect::new("secondary", -1280.0, 24.0, 1280.0, 696.0, 2.0);
        // A bottom-docked bar, 320 px wide, 76 px tall, centred horizontally.
        let bar = LogicalRect::new(-800.0, 644.0, 320.0, 76.0);
        assert_eq!(margins(monitor.rect, bar, Edge::Bottom), (480, 0, 0, 0));
        assert_eq!(anchors(Edge::Bottom), (true, false, false, true));
        // Its popover, 10 px above it.
        let popover = LogicalRect::new(-810.0, 414.0, 340.0, 220.0);
        assert_eq!(
            margins(monitor.rect, popover, Edge::Bottom),
            (470, 0, 0, 86)
        );

        let top_bar = LogicalRect::new(-800.0, 24.0, 320.0, 76.0);
        assert_eq!(margins(monitor.rect, top_bar, Edge::Top), (480, 0, 0, 0));
        assert_eq!(anchors(Edge::Top), (true, false, true, false));
        assert_eq!(anchors(Edge::Left), (true, false, true, false));
        assert_eq!(anchors(Edge::Right), (false, true, true, false));
    }

    #[test]
    fn margins_never_go_negative() {
        // A rect clamped past the opposite edge must not produce a negative
        // margin: layer-shell would interpret it as an offset off-screen.
        let monitor = MonitorRect::new("main", 0.0, 0.0, 1920.0, 1080.0, 1.0);
        let off = LogicalRect::new(-40.0, -40.0, 100.0, 100.0);
        assert_eq!(margins(monitor.rect, off, Edge::Left), (0, 0, 0, 0));
        assert_eq!(margins(monitor.rect, off, Edge::Top), (0, 0, 0, 0));
        let past_right = LogicalRect::new(1900.0, 0.0, 100.0, 100.0);
        assert_eq!(margins(monitor.rect, past_right, Edge::Right), (0, 0, 0, 0));
        let past_bottom = LogicalRect::new(0.0, 1060.0, 100.0, 100.0);
        assert_eq!(
            margins(monitor.rect, past_bottom, Edge::Bottom),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn layer_shell_is_inactive_until_initialize_succeeds() {
        // Unit tests never initialise GTK, so nothing may claim to be docked.
        assert!(!is_active());
    }
}
