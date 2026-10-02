//! `snapshot.json`: a small, versioned, machine-readable copy of the quota
//! snapshot for scripts, status bars and `ai-usage-sidebar --print`
//! (docs/STATUSLINE.md).
//!
//! The file is written atomically (temp file + rename) next to the database
//! after every snapshot while `Settings.exportSnapshot` is on. It never holds
//! account e-mails, names or any path — only what a status bar needs.
//!
//! Schema 1 (additive changes keep the number, a breaking one bumps it):
//!
//! ```json
//! {"schema":1,"updatedAt":"2026-10-02T10:00:00Z","providers":[
//!   {"id":"claude","name":"Claude","status":"ok","plan":"Claude Max 5x",
//!    "fetchedAt":"2026-10-02T09:59:40Z",
//!    "windows":[{"kind":"five_hour","label":"5 hour","usedPercent":73,
//!                "resetsAt":"2026-10-02T11:12:00Z","scope":null,
//!                "forecast":{"projectedPercentAtReset":96,"exhaustsAt":null}}]}]}
//! ```

use crate::model::{AppSnapshot, ProviderQuota, QuotaWindow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};

pub const SCHEMA: u32 = 1;
pub const FILE_NAME: &str = "snapshot.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotFile {
    pub schema: u32,
    /// RFC 3339 UTC: when this file was written.
    pub updated_at: String,
    pub providers: Vec<ExportProvider>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExportProvider {
    pub id: String,
    pub name: String,
    /// `ok | not_logged_in | token_expired | rate_limited | error | disabled`
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// RFC 3339 UTC: when the numbers were fetched from the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<String>,
    pub windows: Vec<ExportWindow>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExportWindow {
    /// `five_hour | seven_day | other`
    pub kind: String,
    pub label: String,
    /// Used percent, 0..100.
    pub used_percent: f64,
    pub resets_at: Option<String>,
    /// Set for per-model / per-feature windows, absent for the account-wide ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forecast: Option<ExportForecast>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExportForecast {
    pub projected_percent_at_reset: f64,
    pub exhausts_at: Option<String>,
}

fn status_str(q: &ProviderQuota) -> String {
    serde_json::to_value(q.status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "error".into())
}

fn export_window(w: &QuotaWindow) -> ExportWindow {
    ExportWindow {
        kind: w.kind.as_str().to_string(),
        label: w.label.clone(),
        used_percent: if w.used_percent.is_finite() {
            w.used_percent.clamp(0.0, 100.0)
        } else {
            0.0
        },
        resets_at: w.resets_at.clone(),
        scope: w.scope.clone(),
        forecast: w.forecast.as_ref().map(|f| ExportForecast {
            projected_percent_at_reset: f.projected_percent_at_reset,
            exhausts_at: f.exhausts_at.clone(),
        }),
    }
}

/// Project the in-memory snapshot onto the public schema. The account (and so
/// the e-mail) is deliberately not copied.
pub fn build(snapshot: &AppSnapshot, updated_at: &str) -> SnapshotFile {
    SnapshotFile {
        schema: SCHEMA,
        updated_at: updated_at.to_string(),
        providers: snapshot
            .providers
            .iter()
            .map(|q| ExportProvider {
                id: q.provider.clone(),
                name: q.display_name.clone(),
                status: status_str(q),
                plan: q.plan_label.clone().or_else(|| q.plan.clone()),
                fetched_at: Some(q.fetched_at.clone()).filter(|s| !s.is_empty()),
                windows: q.windows.iter().map(export_window).collect(),
            })
            .collect(),
    }
}

pub fn file_path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE_NAME)
}

/// Serialize and atomically replace `<data_dir>/snapshot.json`.
pub fn write(data_dir: &Path, snapshot: &AppSnapshot) -> anyhow::Result<()> {
    let file = build(snapshot, &crate::commands::providers::now_rfc3339());
    let bytes = serde_json::to_vec_pretty(&file)?;
    crate::commands::settings::write_atomic(&file_path(data_dir), &bytes)
}

/// Called after every snapshot update. Failures are logged, never fatal.
pub fn write_if_enabled(app: &AppHandle, snapshot: &AppSnapshot) {
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    if !state.settings.read().export_snapshot {
        return;
    }
    if let Err(e) = write(&state.data_dir, snapshot) {
        log::warn!("could not write {FILE_NAME}: {e:#}");
    }
}

static WAS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Settings changed: write the file right away when the export was just turned
/// on, delete it when it was turned off (a stale file would only mislead).
/// Takes the new settings by value of the one flag because the caller holds
/// the settings write lock.
pub fn on_settings_changed(app: &AppHandle, enabled: bool) {
    if WAS_ENABLED.swap(enabled, Ordering::SeqCst) == enabled {
        return;
    }
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    if enabled {
        let snapshot = state.snapshot.read().clone();
        if let Err(e) = write(&state.data_dir, &snapshot) {
            log::warn!("could not write {FILE_NAME}: {e:#}");
        }
    } else if let Err(e) = std::fs::remove_file(file_path(&state.data_dir)) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::debug!("could not remove {FILE_NAME}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn sample() -> AppSnapshot {
        AppSnapshot {
            generated_at: "2026-10-02T10:00:00Z".into(),
            providers: vec![ProviderQuota {
                provider: "claude".into(),
                display_name: "Claude".into(),
                plan: Some("max".into()),
                plan_label: Some("Claude Max 5x".into()),
                account: Some(AccountInfo {
                    email: Some("someone@example.com".into()),
                    name: Some("Someone Private".into()),
                }),
                windows: vec![
                    QuotaWindow {
                        kind: WindowKind::FiveHour,
                        label: "5 hour".into(),
                        window_seconds: Some(18_000),
                        used_percent: 73.0,
                        resets_at: Some("2026-10-02T11:12:00Z".into()),
                        scope: None,
                        is_primary: true,
                        forecast: Some(QuotaForecast {
                            projected_percent_at_reset: 96.0,
                            exhausts_at: None,
                            rate_percent_per_hour: 10.0,
                            confidence: ForecastConfidence::High,
                        }),
                    },
                    QuotaWindow {
                        kind: WindowKind::SevenDay,
                        label: "Weekly".into(),
                        window_seconds: None,
                        used_percent: 140.0,
                        resets_at: None,
                        scope: Some("Fable".into()),
                        is_primary: false,
                        forecast: None,
                    },
                ],
                fetched_at: "2026-10-02T09:59:40Z".into(),
                source: DataSource::Api,
                status: ProviderStatus::RateLimited,
                error: None,
                credits: None,
                extras: vec![],
                next_attempt_at: None,
            }],
        }
    }

    #[test]
    fn serializes_the_stable_versioned_schema() {
        let file = build(&sample(), "2026-10-02T10:00:05Z");
        let json = serde_json::to_value(&file).unwrap();
        assert_eq!(json["schema"], 1);
        assert_eq!(json["updatedAt"], "2026-10-02T10:00:05Z");
        let p = &json["providers"][0];
        assert_eq!(p["id"], "claude");
        assert_eq!(p["name"], "Claude");
        assert_eq!(p["status"], "rate_limited");
        assert_eq!(p["plan"], "Claude Max 5x");
        assert_eq!(p["fetchedAt"], "2026-10-02T09:59:40Z");
        let w = &p["windows"][0];
        assert_eq!(w["kind"], "five_hour");
        assert_eq!(w["label"], "5 hour");
        assert_eq!(w["usedPercent"], 73.0);
        assert_eq!(w["resetsAt"], "2026-10-02T11:12:00Z");
        assert_eq!(w["forecast"]["projectedPercentAtReset"], 96.0);
        assert!(w["forecast"]["exhaustsAt"].is_null());
        assert!(w.get("scope").is_none(), "absent, not null");
        let scoped = &p["windows"][1];
        assert_eq!(scoped["usedPercent"], 100.0, "clamped");
        assert!(scoped["resetsAt"].is_null());
        assert_eq!(scoped["scope"], "Fable");
        assert!(scoped.get("forecast").is_none());
        // and it reads back
        let back: SnapshotFile = serde_json::from_value(json).unwrap();
        assert_eq!(back, file);
    }

    #[test]
    fn never_contains_the_account() {
        let text = serde_json::to_string(&build(&sample(), "x")).unwrap();
        assert!(!text.contains("someone@example.com"));
        assert!(!text.contains("Someone Private"));
        assert!(!text.contains('@'));
        assert!(!text.contains("account"));
    }

    #[test]
    fn writes_atomically_and_replaces_the_previous_file() {
        let dir = crate::commands::test_support::tempdir();
        std::fs::write(file_path(&dir), "old").unwrap();
        write(&dir, &sample()).unwrap();
        let text = std::fs::read_to_string(file_path(&dir)).unwrap();
        let parsed: SnapshotFile = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.schema, SCHEMA);
        assert_eq!(parsed.providers.len(), 1);
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "no temp file left"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
