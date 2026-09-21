//! Backend (data) commands. [BACKEND owns this file]
//! Signatures are part of the contract (docs/ARCHITECTURE.md §5) — keep names
//! and argument names stable.
//!
//! `lib.rs` (owned by the platform layer) only declares `commands, model,
//! scheduler, state, window`, so every other backend module hangs off this one
//! via `#[path]` declarations.

#[path = "ingest/mod.rs"]
pub mod ingest;
#[path = "pricing.rs"]
pub mod pricing;
#[path = "providers/mod.rs"]
pub mod providers;
#[path = "settings.rs"]
pub mod settings;
#[path = "store/mod.rs"]
pub mod store;
#[cfg(test)]
#[path = "test_support.rs"]
pub mod test_support;

use crate::model::*;
use crate::state::AppState;
use tauri::{AppHandle, State};

/// SQLite work runs on the blocking pool; map both failure modes to a string.
async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(f).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(format!("{e:#}")),
        Err(e) => Err(format!("background task failed: {e}")),
    }
}

#[tauri::command]
pub async fn get_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    Ok(state.snapshot.read().clone())
}

#[tauri::command]
pub async fn refresh_now(app: AppHandle, provider: Option<String>) -> Result<AppSnapshot, String> {
    Ok(crate::scheduler::refresh_now(&app, provider).await)
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.settings.read().clone())
}

#[tauri::command]
pub async fn update_settings(app: AppHandle, patch: serde_json::Value) -> Result<Settings, String> {
    settings::update(&app, &patch).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn get_usage_history(
    state: State<'_, AppState>,
    query: HistoryQuery,
) -> Result<HistoryResult, String> {
    let db = state.db()?;
    let pricing = state.pricing.read().clone();
    blocking(move || store::query_history(&db, &query, &pricing)).await
}

#[tauri::command]
pub async fn get_quota_history(
    state: State<'_, AppState>,
    query: QuotaHistoryQuery,
) -> Result<Vec<QuotaSample>, String> {
    let db = state.db()?;
    blocking(move || store::query_quota_history(&db, &query)).await
}

#[tauri::command]
pub async fn get_pricing(state: State<'_, AppState>) -> Result<PricingTable, String> {
    Ok(state.pricing.read().clone())
}

#[tauri::command]
pub async fn set_pricing(
    state: State<'_, AppState>,
    table: PricingTable,
) -> Result<PricingTable, String> {
    let mut current = state.pricing.write();
    let merged = pricing::save(&state.config_dir, &table).map_err(|e| format!("{e:#}"))?;
    *current = merged.clone();
    Ok(merged)
}

/// "Refresh prices now" — only ever reaches the network when the user set
/// `pricingUrl`; a failure keeps the table that is already in use.
#[tauri::command]
pub async fn refresh_pricing(state: State<'_, AppState>) -> Result<PricingTable, String> {
    let (http, url, data_dir, config_dir) = {
        let settings = state.settings.read();
        (
            state.http.clone(),
            settings.pricing_url.clone(),
            state.data_dir.clone(),
            state.config_dir.clone(),
        )
    };
    pricing::refresh_remote(&http, &data_dir, &url, true)
        .await
        .map_err(|e| format!("{e:#}"))?;
    let merged = pricing::load_with_base(&config_dir, pricing::base_table(&data_dir, &url));
    *state.pricing.write() = merged.clone();
    Ok(merged)
}

#[tauri::command]
pub async fn reingest_logs(app: AppHandle) -> Result<IngestStats, String> {
    Ok(crate::scheduler::run_ingest(&app, true).await)
}

#[tauri::command]
pub async fn get_providers(state: State<'_, AppState>) -> Result<Vec<ProviderInfo>, String> {
    let settings = state.settings.read().clone();
    Ok(providers::provider_infos(&state.provider_ctx, &settings))
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
