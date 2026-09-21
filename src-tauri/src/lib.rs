//! Tauri builder wiring. [PLATFORM owns this file]
//! Command handler list must stay in sync with docs/ARCHITECTURE.md §5.

pub mod commands;
pub mod export;
pub mod model;
pub mod scheduler;
pub mod state;
pub mod updater;
pub mod window;

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;

/// Log verbosity, overridable with `AI_USAGE_SIDEBAR_LOG=trace|debug|warn|error`
/// (window placement decisions are logged at `debug`).
fn log_level() -> log::LevelFilter {
    match std::env::var("AI_USAGE_SIDEBAR_LOG")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "trace" => log::LevelFilter::Trace,
        "debug" => log::LevelFilter::Debug,
        "warn" => log::LevelFilter::Warn,
        "error" => log::LevelFilter::Error,
        _ => log::LevelFilter::Info,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be the first plugin: a second launch hands its argv to the
        // running instance instead of starting another sidebar.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            // A second autostart entry (`--hidden`) must not pop the dashboard.
            if argv.iter().any(|arg| arg == "--hidden") {
                log::info!("second hidden instance ({argv:?} in {cwd}), ignoring");
                return;
            }
            log::info!("second instance ({argv:?} in {cwd}), focusing the dashboard");
            window::dashboard::focus(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_log::Builder::new().level(log_level()).build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(window::shortcuts::plugin())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .setup(|app| {
            // A tray widget has no business in the Dock or the app switcher.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let config_dir = app.path().app_config_dir().expect("app config dir");
            // SQLite (WAL) must not live in the Windows roaming profile, which
            // may be a redirected network share. Same path as before elsewhere.
            let roaming_dir = app.path().app_data_dir().expect("app data dir");
            let data_dir = match app.path().app_local_data_dir() {
                // Keep a database that an earlier version already created.
                Ok(local) if !roaming_dir.join("usage.db").exists() => local,
                _ => roaming_dir,
            };
            std::fs::create_dir_all(&config_dir).ok();
            std::fs::create_dir_all(&data_dir).ok();
            app.manage(state::AppState::new(config_dir, data_dir));
            window::setup(app.handle())?;
            updater::setup(app.handle());
            scheduler::start(app.handle().clone());
            // External edits of settings.json apply without a restart.
            commands::settings::watch(app.handle().clone());
            // No-op unless the user opted into a remote pricing table.
            commands::pricing::start_remote_refresh(app.handle().clone());

            #[cfg(debug_assertions)]
            if std::env::var("AI_USAGE_SIDEBAR_DEVTOOLS").as_deref() == Ok("1") {
                for label in [model::windows::DASHBOARD, model::windows::SIDEBAR] {
                    if let Some(win) = app.get_webview_window(label) {
                        win.open_devtools();
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // backend
            commands::get_snapshot,
            commands::refresh_now,
            commands::get_settings,
            commands::update_settings,
            commands::get_usage_history,
            commands::get_usage_calendar,
            commands::get_usage_sessions,
            commands::get_quota_history,
            commands::get_pricing,
            commands::set_pricing,
            commands::refresh_pricing,
            commands::reingest_logs,
            commands::get_providers,
            commands::get_app_info,
            export::export_usage_csv,
            // updater
            updater::get_update_status,
            updater::check_for_updates,
            updater::install_update,
            // platform
            window::shortcuts::get_shortcut_status,
            window::sidebar_set_expanded,
            window::sidebar_relayout,
            window::sidebar_drag,
            window::popover_show,
            window::popover_relayout,
            window::popover_hide,
            window::popover_set_pinned,
            window::hover_report,
            window::open_dashboard,
            window::apply_window_settings,
            window::get_monitors,
            window::quit_app,
            window::debug_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
