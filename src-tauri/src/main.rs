// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Platform environment tweaks must happen before GTK/WebKit initialise.
    #[cfg(target_os = "linux")]
    {
        // GNOME Wayland cannot position or keep-above xdg toplevels, so run on
        // XWayland unless the user explicitly opts into native Wayland.
        let backend = std::env::var("AI_USAGE_SIDEBAR_BACKEND").unwrap_or_default();
        if backend != "wayland" && std::env::var("GDK_BACKEND").is_err() {
            std::env::set_var("GDK_BACKEND", "x11");
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
