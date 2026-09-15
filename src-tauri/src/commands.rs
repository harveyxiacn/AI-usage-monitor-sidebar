//! Backend (data) commands. [BACKEND owns this file]
//! Signatures are part of the contract (docs/ARCHITECTURE.md §5) — keep names
//! and argument names stable; replace the stub bodies with real logic.

use crate::model::*;
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub async fn get_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    Ok(state.snapshot.read().clone())
}

#[tauri::command]
pub async fn refresh_now(
    state: State<'_, AppState>,
    provider: Option<String>,
) -> Result<AppSnapshot, String> {
    let _ = provider;
    Ok(state.snapshot.read().clone())
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.settings.read().clone())
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppState>,
    patch: serde_json::Value,
) -> Result<Settings, String> {
    let _ = patch;
    Ok(state.settings.read().clone())
}

#[tauri::command]
pub async fn get_usage_history(
    state: State<'_, AppState>,
    query: HistoryQuery,
) -> Result<HistoryResult, String> {
    let _ = (state, query);
    Ok(HistoryResult::default())
}

#[tauri::command]
pub async fn get_quota_history(
    state: State<'_, AppState>,
    query: QuotaHistoryQuery,
) -> Result<Vec<QuotaSample>, String> {
    let _ = (state, query);
    Ok(vec![])
}

#[tauri::command]
pub async fn get_pricing(state: State<'_, AppState>) -> Result<PricingTable, String> {
    let _ = state;
    Ok(PricingTable::default())
}

#[tauri::command]
pub async fn set_pricing(
    state: State<'_, AppState>,
    table: PricingTable,
) -> Result<PricingTable, String> {
    let _ = state;
    Ok(table)
}

#[tauri::command]
pub async fn reingest_logs(state: State<'_, AppState>) -> Result<IngestStats, String> {
    let _ = state;
    Ok(IngestStats::default())
}

#[tauri::command]
pub async fn get_providers(state: State<'_, AppState>) -> Result<Vec<ProviderInfo>, String> {
    let _ = state;
    Ok(vec![])
}

#[tauri::command]
pub async fn get_app_info(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<AppInfo, String> {
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: state.data_dir.display().to_string(),
        config_dir: state.config_dir.display().to_string(),
        platform: if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            "unknown"
        }
        .into(),
        backend: crate::window::backend_name(),
    })
}
