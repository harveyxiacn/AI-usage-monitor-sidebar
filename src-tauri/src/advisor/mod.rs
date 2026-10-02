//! Decision support: advice computed from data the app already has. [BACKEND]
//!
//! * [`routing`] — which provider has room when another is about to run out
//!   (pure function over the snapshot + forecasts; optional notification).
//! * [`git`] — task-level cost: usage attributed to commits. Opt-in
//!   (`gitAttribution`), read-only `git log` metadata, see its module docs.
//!
//! The plan advisor (upgrade / downgrade / keep) works on quota cycles and
//! the subscription ROI that the History tab already computes, so it lives in
//! the frontend (`src/lib/plan-advisor.ts`).
//!
//! Nothing here is read, run or sent unless the user opened the matching view
//! or switched the matching setting on; none of it ever claims a billing fact.

pub mod git;
pub mod routing;

use tauri::State;

use crate::state::AppState;

/// The current routing recommendation, or `None` when there is nothing worth
/// saying. Computed from the in-memory snapshot only: no I/O.
#[tauri::command]
pub async fn get_routing_advice(
    state: State<'_, AppState>,
) -> Result<Option<routing::RoutingAdvice>, String> {
    let snapshot = state.snapshot.read().clone();
    let settings = state.settings.read().clone();
    Ok(routing::advise(
        &snapshot,
        &settings,
        crate::commands::store::now_ms(),
    ))
}

/// Commits of one project with the tokens / estimated cost attributed to each.
/// Does nothing (and spawns nothing) while `gitAttribution` is off.
#[tauri::command]
pub async fn get_project_commits(
    state: State<'_, AppState>,
    query: git::CommitsQuery,
) -> Result<git::CommitsResult, String> {
    let enabled = state.settings.read().git_attribution;
    let db = state.db()?;
    let pricing = state.pricing.read().clone();
    match tauri::async_runtime::spawn_blocking(move || {
        git::commits_for_project(&db, &pricing, enabled, &query, &|cwd, since, until| {
            git::run_git_log(cwd, since, until)
        })
    })
    .await
    {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(e)) => Err(format!("{e:#}")),
        Err(e) => Err(format!("background task failed: {e}")),
    }
}
