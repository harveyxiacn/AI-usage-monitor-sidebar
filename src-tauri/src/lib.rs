//! Tauri builder wiring. [PLATFORM owns this file]
//! Command handler list must stay in sync with docs/ARCHITECTURE.md §5.

pub mod commands;
pub mod model;
pub mod scheduler;
pub mod state;
pub mod window;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_log::Builder::new().level(log::LevelFilter::Info).build())
        .setup(|app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .expect("app config dir");
            let data_dir = app.path().app_data_dir().expect("app data dir");
            std::fs::create_dir_all(&config_dir).ok();
            std::fs::create_dir_all(&data_dir).ok();
            app.manage(state::AppState::new(config_dir, data_dir));
            window::setup(app.handle())?;
            scheduler::start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // backend
            commands::get_snapshot,
            commands::refresh_now,
            commands::get_settings,
            commands::update_settings,
            commands::get_usage_history,
            commands::get_quota_history,
            commands::get_pricing,
            commands::set_pricing,
            commands::reingest_logs,
            commands::get_providers,
            commands::get_app_info,
            // platform
            window::sidebar_set_expanded,
            window::sidebar_relayout,
            window::popover_show,
            window::popover_relayout,
            window::popover_hide,
            window::popover_set_pinned,
            window::hover_report,
            window::open_dashboard,
            window::apply_window_settings,
            window::get_monitors,
            window::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
