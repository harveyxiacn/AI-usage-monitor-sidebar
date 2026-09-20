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
        // Respect a user-supplied GDK backend unless the app-specific override
        // is explicit. Pure Wayland sessions can still open the dashboard.
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
}
