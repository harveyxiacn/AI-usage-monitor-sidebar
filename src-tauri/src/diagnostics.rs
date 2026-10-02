//! `get_diagnostics` and `open_folder`: what a bug report needs, with nothing
//! secret in it. [BACKEND]
//!
//! The report never contains tokens (the app does not even hold them past a
//! request), the contents of the credential files, or an unmasked e-mail
//! address, whatever `hideAccountEmail` says: a diagnostics blob gets pasted
//! into public issues. The log tail goes through the same best-effort
//! redaction as the AI-assessment preview.

use crate::model::{ProviderStatus, Settings};
use crate::state::AppState;
use serde::Serialize;
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

/// Log lines in the report.
const LOG_TAIL_LINES: usize = 80;
/// Never read more than this from the end of a log file.
const LOG_TAIL_BYTES: u64 = 64 * 1024;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDiagnostics {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub experimental: bool,
    pub logged_in: bool,
    pub status: Option<ProviderStatus>,
    pub plan_label: Option<String>,
    /// Always masked (`h•••@g•••.com`).
    pub account: Option<String>,
    pub error: Option<String>,
    pub fetched_at: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub app_version: String,
    pub os: String,
    pub arch: String,
    /// "x11" | "wayland" | "cocoa" | "win32"
    pub backend: String,
    /// `$XDG_SESSION_TYPE` on Linux, otherwise absent.
    pub session_type: Option<String>,
    pub providers: Vec<ProviderDiagnostics>,
    /// The effective settings, minus anything that could carry a secret.
    pub settings: Value,
    pub log_dir: String,
    pub config_dir: String,
    pub data_dir: String,
    /// File name of the log the tail comes from.
    pub log_file: Option<String>,
    pub log_tail: String,
}

/// `harvey@gmail.com` → `h•••@g•••.com`; the Rust twin of `maskEmail` in
/// `src/lib/privacy.ts`.
pub fn mask_email(email: &str) -> String {
    const MASK: &str = "•••";
    let first = |s: &str| s.chars().next().map(String::from).unwrap_or_default();
    let value = email.trim();
    if value.is_empty() {
        return String::new();
    }
    let Some(at) = value.rfind('@') else {
        return first(value) + MASK;
    };
    let (local, domain) = (&value[..at], &value[at + 1..]);
    let head = if local.is_empty() {
        MASK.to_string()
    } else {
        first(local) + MASK
    };
    if domain.is_empty() {
        return format!("{head}@");
    }
    let tail = match domain.rfind('.') {
        Some(dot) if dot > 0 => format!("{}{MASK}{}", first(domain), &domain[dot..]),
        _ => first(domain) + MASK,
    };
    format!("{head}@{tail}")
}

/// A URL without credentials, query or fragment: `pricingUrl` may be a private
/// source that carries a token.
pub fn sanitize_url(url: &str) -> String {
    let url = url.trim();
    if url.is_empty() {
        return String::new();
    }
    let without_tail = url.split(['?', '#']).next().unwrap_or_default();
    match without_tail.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
            let host = authority.rsplit('@').next().unwrap_or_default();
            if path.is_empty() {
                format!("{scheme}://{host}")
            } else {
                format!("{scheme}://{host}/{path}")
            }
        }
        None => "[not a URL]".into(),
    }
}

/// Settings for the report: everything that is in `settings.json`, with the
/// custom pricing URL stripped of credentials.
pub fn settings_for_report(settings: &Settings) -> Value {
    let mut value = serde_json::to_value(settings).unwrap_or(Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "pricingUrl".into(),
            Value::String(sanitize_url(&settings.pricing_url)),
        );
    }
    value
}

/// The last `lines` lines of `text`.
pub fn tail_lines(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// The most recently modified `*.log` file in `dir`.
fn newest_log(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

/// Redacted tail of one log file, reading at most [`LOG_TAIL_BYTES`].
fn read_log_tail(path: &Path) -> String {
    let read = || -> std::io::Result<String> {
        let mut file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        let start = len.saturating_sub(LOG_TAIL_BYTES);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.take(LOG_TAIL_BYTES).read_to_end(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        // Starting mid-file cut the first line in half.
        Ok(if start > 0 {
            text.split_once('\n')
                .map(|(_, rest)| rest.to_string())
                .unwrap_or_default()
        } else {
            text
        })
    };
    match read() {
        Ok(text) => crate::evaluation::redact(&tail_lines(&text, LOG_TAIL_LINES)),
        Err(e) => format!("(log unreadable: {e})"),
    }
}

fn log_dir(app: &AppHandle) -> PathBuf {
    app.path().app_log_dir().unwrap_or_default()
}

#[tauri::command]
pub async fn get_diagnostics(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Diagnostics, String> {
    let settings = state.settings.read().clone();
    let snapshot = state.snapshot.read().clone();
    let infos = crate::commands::providers::provider_infos(&state.provider_ctx, &settings);
    let providers = infos
        .iter()
        .map(|info| {
            let quota = snapshot.providers.iter().find(|p| p.key() == info.id);
            ProviderDiagnostics {
                id: info.id.clone(),
                display_name: info.display_name.clone(),
                enabled: settings.providers.get(&info.id).is_none_or(|p| p.enabled),
                experimental: info.experimental,
                logged_in: info.logged_in,
                status: quota.map(|q| q.status),
                plan_label: quota.and_then(|q| q.plan_label.clone()),
                account: quota
                    .and_then(|q| q.account.as_ref())
                    .and_then(|a| a.email.as_deref())
                    .map(mask_email),
                error: quota
                    .and_then(|q| q.error.as_deref())
                    .map(crate::evaluation::redact),
                fetched_at: quota.map(|q| q.fetched_at.clone()),
            }
        })
        .collect();

    let log_dir = log_dir(&app);
    let (log_file, log_tail) = match newest_log(&log_dir) {
        Some(path) => (
            path.file_name().map(|n| n.to_string_lossy().into_owned()),
            read_log_tail(&path),
        ),
        None => (None, String::new()),
    };

    Ok(Diagnostics {
        app_version: app.package_info().version.to_string(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        backend: crate::window::backend_name(),
        session_type: if cfg!(target_os = "linux") {
            std::env::var("XDG_SESSION_TYPE").ok()
        } else {
            None
        },
        providers,
        settings: settings_for_report(&settings),
        log_dir: log_dir.display().to_string(),
        config_dir: state.config_dir.display().to_string(),
        data_dir: state.data_dir.display().to_string(),
        log_file,
        log_tail,
    })
}

/// Open one of the app's own folders in the file manager. The argument is a
/// name, never a path, so the command cannot be used to open anything else.
#[tauri::command]
pub async fn open_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    which: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = match which.as_str() {
        "log" => log_dir(&app),
        "config" => state.config_dir.clone(),
        "data" => state.data_dir.clone(),
        other => return Err(format!("unknown folder `{other}`")),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::tempdir;

    #[test]
    fn emails_are_masked_like_the_frontend_does() {
        assert_eq!(mask_email("harvey@gmail.com"), "h•••@g•••.com");
        assert_eq!(mask_email("  "), "");
        assert_eq!(mask_email("nobody"), "n•••");
        assert_eq!(mask_email("@x.org"), "•••@x•••.org");
        assert_eq!(mask_email("a@"), "a•••@");
        assert_eq!(mask_email("a@localhost"), "a•••@l•••");
        assert_eq!(mask_email("张三@例子.中国"), "张•••@例•••.中国");
    }

    #[test]
    fn a_pricing_url_loses_credentials_query_and_fragment() {
        assert_eq!(sanitize_url(""), "");
        assert_eq!(
            sanitize_url("https://user:pw@example.com/p/pricing.json?token=abc#x"),
            "https://example.com/p/pricing.json"
        );
        assert_eq!(sanitize_url("https://example.com"), "https://example.com");
        assert_eq!(sanitize_url("secret-without-scheme"), "[not a URL]");
    }

    #[test]
    fn the_report_settings_never_carry_the_url_secret() {
        let settings = Settings {
            pricing_url: "https://example.com/p.json?key=hunter2".into(),
            ..Settings::default()
        };
        let report = settings_for_report(&settings).to_string();
        assert!(!report.contains("hunter2"));
        assert!(report.contains("https://example.com/p.json"));
    }

    #[test]
    fn tail_keeps_the_last_lines_only() {
        assert_eq!(tail_lines("a\nb\nc\nd", 2), "c\nd");
        assert_eq!(tail_lines("a\nb", 10), "a\nb");
        assert_eq!(tail_lines("", 3), "");
    }

    #[test]
    fn the_log_tail_is_bounded_and_redacted() {
        let dir = tempdir();
        let mut text = String::new();
        for i in 0..500 {
            text.push_str(&format!("line {i}\n"));
        }
        text.push_str("Authorization: Bearer abc.def\nkey sk-live-123 end\n");
        std::fs::write(dir.join("old.log"), "ancient").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(dir.join("new.log"), text).unwrap();
        let newest = newest_log(&dir).unwrap();
        assert_eq!(newest.file_name().unwrap(), "new.log");
        let tail = read_log_tail(&newest);
        assert_eq!(tail.lines().count(), LOG_TAIL_LINES);
        assert!(tail.contains("line 499"));
        assert!(!tail.contains("abc.def") && !tail.contains("sk-live-123"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_log_directory_has_no_newest_file() {
        assert!(newest_log(Path::new("/definitely/not/here")).is_none());
    }
}
