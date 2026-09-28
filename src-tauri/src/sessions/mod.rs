//! Local session metadata and bounded, opt-in transcript access.
pub mod model;
mod parser;
mod store;
pub use store::{
    clear_analysis, content_page, detail, ensure_schema, index_codex_titles, index_file, list,
    set_alias,
};

use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub async fn list_sessions(
    state: State<'_, AppState>,
    query: model::SessionListQuery,
) -> Result<model::SessionListResult, String> {
    let db = state.db()?;
    let pricing = state.pricing.read().clone();
    let content = crate::evaluation::load_settings(&state.config_dir)
        .map_err(|e| e.to_string())?
        .content_enabled;
    tauri::async_runtime::spawn_blocking(move || list(&db, &query, &pricing, content))
        .await
        .map_err(|e| format!("session worker failed: {e}"))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_session_detail(
    state: State<'_, AppState>,
    provider: String,
    session_id: String,
    offset: Option<u32>,
    limit: Option<u32>,
) -> Result<model::SessionDetail, String> {
    let db = state.db()?;
    let pricing = state.pricing.read().clone();
    let content = crate::evaluation::load_settings(&state.config_dir)
        .map_err(|e| e.to_string())?
        .content_enabled;
    tauri::async_runtime::spawn_blocking(move || {
        detail(
            &db,
            &provider,
            &session_id,
            offset.unwrap_or(0),
            limit.unwrap_or(30),
            &pricing,
            content,
        )
    })
    .await
    .map_err(|e| format!("session worker failed: {e}"))?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_session_alias(
    state: State<'_, AppState>,
    provider: String,
    session_id: String,
    alias: String,
) -> Result<(), String> {
    let db = state.db()?;
    tauri::async_runtime::spawn_blocking(move || set_alias(&db, &provider, &session_id, &alias))
        .await
        .map_err(|e| format!("session worker failed: {e}"))?
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
