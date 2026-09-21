// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Platform environment tweaks must happen before GTK/WebKit initialise.
    #[cfg(target_os = "linux")]
    {
        // GNOME Wayland cannot position or keep-above xdg toplevels, so run on
        // XWayland unless the user explicitly opts into native Wayland.
        if let Some(backend) = linux_backend(
            std::env::var("AI_USAGE_SIDEBAR_BACKEND").ok().as_deref(),
            std::env::var("GDK_BACKEND").ok().as_deref(),
            std::env::var_os("DISPLAY").is_some(),
            std::env::var_os("WAYLAND_DISPLAY").is_some(),
        ) {
            std::env::set_var("GDK_BACKEND", backend);
        }
        // Only does something when the decision above left us on Wayland: it
        // checks gtk-layer-shell and falls back to XWayland when the
        // compositor cannot dock an edge widget. A failure here is not fatal —
        // the app then behaves exactly like it did before layer-shell existed,
        // i.e. an ordinary Wayland window the compositor places.
        if let Err(e) = ai_usage_sidebar_lib::window::linux::prepare_backend() {
            eprintln!("AI Usage Sidebar: {e}");
        }
        // WebKitGTK + proprietary NVIDIA driver renders blank/black windows
        // with the DMABUF renderer.
        if std::path::Path::new("/proc/driver/nvidia/version").exists()
            && std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err()
        {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }
    ai_usage_sidebar_lib::run()
}

/// Which GDK backend to force, or `None` to leave `GDK_BACKEND` untouched.
///
/// The policy, in order:
///
/// 1. `AI_USAGE_SIDEBAR_BACKEND=wayland|x11` always wins.
/// 2. A `GDK_BACKEND` the user set themselves is respected as-is — that is
///    how `GDK_BACKEND=wayland` opts into native layer-shell docking.
/// 3. Without an X display there is nothing to fall back to: Wayland.
/// 4. Otherwise **XWayland**, because it is the only backend on which the
///    widget is guaranteed to be positionable and kept above on every
///    desktop, GNOME/mutter included.
///
/// Native Wayland stays strictly opt-in: `window::linux::prepare_backend`
/// then verifies gtk-layer-shell and falls back to XWayland when it is
/// missing or the compositor does not implement `zwlr_layer_shell_v1`.
#[cfg(target_os = "linux")]
fn linux_backend(
    requested: Option<&str>,
    existing: Option<&str>,
    x11: bool,
    wayland: bool,
) -> Option<&'static str> {
    match requested {
        Some("wayland") => Some("wayland"),
        Some("x11") => Some("x11"),
        _ if existing.is_some_and(|value| !value.is_empty()) => None,
        _ if !x11 && wayland => Some("wayland"),
        _ => Some("x11"),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::linux_backend;

    #[test]
    fn explicit_override_takes_precedence_over_gdk() {
        assert_eq!(
            linux_backend(Some("wayland"), Some("x11"), true, true),
            Some("wayland")
        );
        assert_eq!(
            linux_backend(Some("x11"), Some("wayland"), true, true),
            Some("x11")
        );
    }

    #[test]
    fn defaults_to_xwayland_but_can_launch_without_xwayland() {
        assert_eq!(linux_backend(None, None, true, true), Some("x11"));
        assert_eq!(linux_backend(None, None, false, true), Some("wayland"));
        assert_eq!(linux_backend(None, Some("wayland"), true, true), None);
    }

    /// The regression that matters on GNOME: as long as an X display exists,
    /// nothing may silently pick native Wayland. Only the two explicit opt-ins
    /// (`AI_USAGE_SIDEBAR_BACKEND=wayland`, a user-set `GDK_BACKEND=wayland`)
    /// and the no-X-display case may end up there.
    #[test]
    fn xwayland_is_the_default_whenever_a_display_exists() {
        for wayland in [false, true] {
            assert_eq!(linux_backend(None, None, true, wayland), Some("x11"));
            assert_eq!(linux_backend(None, Some(""), true, wayland), Some("x11"));
            assert_eq!(linux_backend(Some(""), None, true, wayland), Some("x11"));
            assert_eq!(
                linux_backend(Some("auto"), None, true, wayland),
                Some("x11"),
                "an unknown override must not enable the native path"
            );
        }
    }

    #[test]
    fn a_user_set_gdk_backend_is_never_overwritten() {
        for value in ["wayland", "x11", "broadway"] {
            assert_eq!(linux_backend(None, Some(value), true, true), None);
            assert_eq!(linux_backend(None, Some(value), false, true), None);
        }
    }

    #[test]
    fn headless_falls_back_to_x11_as_before() {
        // No DISPLAY and no WAYLAND_DISPLAY: keep the historical behaviour so
        // the failure comes from GTK with its own message.
        assert_eq!(linux_backend(None, None, false, false), Some("x11"));
    }
}
