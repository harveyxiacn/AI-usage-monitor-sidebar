//! `ai-usage-sidebar --print [--format json|line|statusline] [--provider ID]`
//!
//! Prints the numbers from `snapshot.json` (see `export_snapshot.rs`) and
//! exits, so a shell prompt, tmux, polybar or Claude Code's `statusLine` can
//! show them. It is handled at the very top of `main()`, before Tauri is
//! built: no window, no single-instance handshake with a running app, no
//! network and no credentials. Documented in docs/STATUSLINE.md.
//!
//! Exit codes: 0 printed; 1 bad arguments; 2 no usable data (file missing,
//! unreadable, older than [`MAX_AGE_SEC`] or no such provider — the reason is
//! on stderr and stdout stays empty).

use crate::export_snapshot::{self, ExportProvider, ExportWindow, SnapshotFile};
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

/// A snapshot file older than this is not trusted (the app is not running or
/// polling is paused).
pub const MAX_AGE_SEC: i64 = 15 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Json,
    Line,
    Statusline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintArgs {
    pub format: Format,
    pub provider: Option<String>,
}

/// `None` when `--print` is absent (a normal GUI start). `Some(Err)` for a
/// `--print` call with bad arguments.
pub fn parse_args(args: &[String]) -> Option<Result<PrintArgs, String>> {
    if !args.iter().any(|a| a == "--print") {
        return None;
    }
    let mut out = PrintArgs {
        format: Format::Line,
        provider: None,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        match key {
            "--print" => {}
            "--format" | "--provider" => {
                let Some(value) = inline.or_else(|| it.next().cloned()) else {
                    return Some(Err(format!("{key} needs a value")));
                };
                if key == "--format" {
                    out.format = match value.as_str() {
                        "json" => Format::Json,
                        "line" => Format::Line,
                        "statusline" => Format::Statusline,
                        other => {
                            return Some(Err(format!(
                                "unknown format `{other}` (json, line or statusline)"
                            )))
                        }
                    };
                } else {
                    out.provider = Some(value.to_ascii_lowercase());
                }
            }
            other => return Some(Err(format!("unknown option `{other}` for --print"))),
        }
    }
    Some(Ok(out))
}

// ---------- rendering ----------

/// "1h12m", "3d4h", "45m", "<1m".
pub fn format_countdown(secs: i64) -> String {
    if secs < 60 {
        return "<1m".into();
    }
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3_600, secs % 3_600 / 60);
    if d > 0 {
        format!("{d}d{h}h")
    } else if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}m")
    }
}

fn window_tag(w: &ExportWindow) -> String {
    match w.kind.as_str() {
        "five_hour" => "5h".into(),
        "seven_day" => "7d".into(),
        _ => w.label.clone(),
    }
}

fn percent(w: &ExportWindow) -> i64 {
    w.used_percent.round() as i64
}

fn status_text(status: &str) -> &'static str {
    match status {
        "not_logged_in" => "not signed in",
        "token_expired" => "token expired",
        "rate_limited" => "rate limited",
        "error" => "error",
        _ => "",
    }
}

fn short_name(p: &ExportProvider) -> String {
    let base = p.id.split('@').next().unwrap_or(&p.id);
    let short: String = match base {
        "claude" => "CC".into(),
        "codex" => "CX".into(),
        "copilot" => "GH".into(),
        other => other.chars().take(2).collect::<String>().to_uppercase(),
    };
    // an extra account: `CC@work`
    match &p.account {
        Some(account) => format!("{short}@{account}"),
        None => short,
    }
}

/// The account-wide windows (scoped per-model ones stay in the JSON only).
fn main_windows(p: &ExportProvider) -> impl Iterator<Item = &ExportWindow> {
    p.windows.iter().filter(|w| w.scope.is_none())
}

fn countdown(w: &ExportWindow, now_ms: i64) -> Option<String> {
    let reset = chrono::DateTime::parse_from_rfc3339(w.resets_at.as_deref()?)
        .ok()?
        .timestamp_millis();
    (reset > now_ms).then(|| format_countdown((reset - now_ms) / 1000))
}

/// `Claude 5h 73% (1h12m) · 7d 41% | Codex 5h 12%`
fn render_line(file: &SnapshotFile, now_ms: i64) -> String {
    let parts: Vec<String> = file
        .providers
        .iter()
        .filter(|p| p.status != "disabled")
        .map(|p| {
            let windows: Vec<String> = main_windows(p)
                .enumerate()
                .map(|(i, w)| {
                    let reset = if i == 0 { countdown(w, now_ms) } else { None };
                    match reset {
                        Some(r) => format!("{} {}% ({r})", window_tag(w), percent(w)),
                        None => format!("{} {}%", window_tag(w), percent(w)),
                    }
                })
                .collect();
            let mut text = format!("{} {}", p.name, windows.join(" · "))
                .trim_end()
                .to_string();
            let note = status_text(&p.status);
            if !note.is_empty() {
                text.push_str(&format!(" ({note})"));
            }
            text
        })
        .collect();
    parts.join(" | ")
}

/// `CC 5h 73% 7d 41% | CX 5h 12%` — short names, no countdown; a window at
/// 90 % or more gets a `!`, a provider that needs attention a trailing `?`.
fn render_statusline(file: &SnapshotFile) -> String {
    let parts: Vec<String> = file
        .providers
        .iter()
        .filter(|p| p.status != "disabled")
        .map(|p| {
            let mut text = short_name(p);
            for w in main_windows(p) {
                let flag = if w.used_percent >= 90.0 { "!" } else { "" };
                text.push_str(&format!(" {} {}%{flag}", window_tag(w), percent(w)));
            }
            if !matches!(p.status.as_str(), "ok" | "rate_limited") {
                text.push_str(" ?");
            }
            text
        })
        .collect();
    parts.join(" | ")
}

/// Everything after reading the file: filter, then format.
pub fn render(file: &SnapshotFile, args: &PrintArgs, now_ms: i64) -> Result<String, String> {
    let mut file = file.clone();
    if let Some(wanted) = &args.provider {
        file.providers.retain(|p| &p.id == wanted);
        if file.providers.is_empty() {
            return Err(format!("provider `{wanted}` is not in the snapshot"));
        }
    }
    Ok(match args.format {
        Format::Json => serde_json::to_string(&file).map_err(|e| e.to_string())?,
        Format::Line => render_line(&file, now_ms),
        Format::Statusline => render_statusline(&file),
    })
}

// ---------- file access ----------

/// Read and validate `<data_dir>/snapshot.json`; the error text is what the
/// user sees on stderr.
pub fn load(data_dir: &Path, now_ms: i64) -> Result<SnapshotFile, String> {
    let path = export_snapshot::file_path(data_dir);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!(
                "{} not found. Start AI Usage Sidebar and turn on Settings > Integrations > \
                 Export snapshot (\"exportSnapshot\": true in settings.json).",
                path.display()
            )
        } else {
            format!("cannot read {}: {e}", path.display())
        }
    })?;
    let file: SnapshotFile = serde_json::from_str(&text)
        .map_err(|e| format!("{} is not a valid snapshot: {e}", path.display()))?;
    if file.schema != export_snapshot::SCHEMA {
        return Err(format!(
            "{} uses schema {}, this build understands {}",
            path.display(),
            file.schema,
            export_snapshot::SCHEMA
        ));
    }
    let updated = chrono::DateTime::parse_from_rfc3339(&file.updated_at)
        .map_err(|_| format!("{} has no valid updatedAt", path.display()))?
        .timestamp_millis();
    let age = (now_ms - updated) / 1000;
    if age > MAX_AGE_SEC {
        return Err(format!(
            "snapshot is stale ({} old, limit {}): is AI Usage Sidebar running, and is polling \
             not paused?",
            format_countdown(age),
            format_countdown(MAX_AGE_SEC)
        ));
    }
    Ok(file)
}

/// The directory the running app keeps `usage.db` and `snapshot.json` in,
/// computed without Tauri (same rule as `lib.rs`).
pub fn default_data_dir() -> Option<PathBuf> {
    const ID: &str = "io.github.harveyxiacn.ai-usage-sidebar";
    let roaming = dirs::data_dir()?.join(ID);
    let local = dirs::data_local_dir().map(|d| d.join(ID));
    Some(crate::state::pick_data_dir(roaming, local))
}

/// The whole command with injectable I/O; returns the exit code.
pub fn run(
    args: &PrintArgs,
    data_dir: &Path,
    now_ms: i64,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match load(data_dir, now_ms).and_then(|file| render(&file, args, now_ms)) {
        Ok(text) => {
            // A closed pipe (`| head -c0`) is not an error worth reporting.
            let _ = writeln!(out, "{text}");
            0
        }
        Err(message) => {
            let _ = writeln!(err, "ai-usage-sidebar: {message}");
            2
        }
    }
}

/// Entry point called first thing in `main()`. `Some(code)` = handled, exit.
pub fn handle(args: impl Iterator<Item = OsString>) -> Option<i32> {
    let args: Vec<String> = args
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let parsed = parse_args(&args)?;
    let (mut out, mut err) = (console::stdout(), console::stderr());
    let parsed = match parsed {
        Ok(p) => p,
        Err(message) => {
            let _ = writeln!(
                err,
                "ai-usage-sidebar: {message}\nusage: ai-usage-sidebar --print [--format json|line|statusline] [--provider ID]"
            );
            return Some(1);
        }
    };
    let Some(dir) = default_data_dir() else {
        let _ = writeln!(
            err,
            "ai-usage-sidebar: cannot locate the app data directory"
        );
        return Some(2);
    };
    let now = chrono::Utc::now().timestamp_millis();
    Some(run(&parsed, &dir, now, &mut out, &mut err))
}

/// Standard streams that also work in the Windows GUI-subsystem release
/// build, which starts without any (a terminal shows nothing). When a stream
/// is not redirected we attach to the parent's console and open it directly.
mod console {
    use std::io::Write;

    #[cfg(not(windows))]
    pub fn stdout() -> Box<dyn Write> {
        Box::new(std::io::stdout())
    }

    #[cfg(not(windows))]
    pub fn stderr() -> Box<dyn Write> {
        Box::new(std::io::stderr())
    }

    #[cfg(windows)]
    mod win {
        extern "system" {
            fn AttachConsole(process_id: u32) -> i32;
            fn GetStdHandle(handle: u32) -> isize;
        }
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        pub const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
        pub const STD_ERROR_HANDLE: u32 = -12i32 as u32;

        pub fn has_handle(which: u32) -> bool {
            // SAFETY: plain Win32 query with no pointer arguments.
            let h = unsafe { GetStdHandle(which) };
            h != 0 && h != -1
        }

        pub fn attach_parent_console() {
            // SAFETY: no pointer arguments; failure (no parent console) is fine.
            unsafe {
                AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }
    }

    #[cfg(windows)]
    fn stream(which: u32, fallback: Box<dyn Write>) -> Box<dyn Write> {
        if win::has_handle(which) {
            return fallback;
        }
        win::attach_parent_console();
        match std::fs::OpenOptions::new().write(true).open("CONOUT$") {
            Ok(f) => Box::new(f),
            Err(_) => Box::new(std::io::sink()),
        }
    }

    #[cfg(windows)]
    pub fn stdout() -> Box<dyn Write> {
        stream(win::STD_OUTPUT_HANDLE, Box::new(std::io::stdout()))
    }

    #[cfg(windows)]
    pub fn stderr() -> Box<dyn Write> {
        stream(win::STD_ERROR_HANDLE, Box::new(std::io::stderr()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export_snapshot::ExportForecast;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    fn window(kind: &str, used: f64, resets: Option<&str>) -> ExportWindow {
        ExportWindow {
            kind: kind.into(),
            label: kind.into(),
            used_percent: used,
            resets_at: resets.map(str::to_string),
            scope: None,
            forecast: None::<ExportForecast>,
        }
    }

    fn provider(id: &str, name: &str, status: &str, windows: Vec<ExportWindow>) -> ExportProvider {
        ExportProvider {
            id: id.into(),
            account: id.split_once('@').map(|(_, a)| a.to_string()),
            name: name.into(),
            status: status.into(),
            plan: None,
            fetched_at: None,
            windows,
        }
    }

    const NOW: &str = "2026-10-02T10:00:00Z";

    fn now_ms() -> i64 {
        chrono::DateTime::parse_from_rfc3339(NOW)
            .unwrap()
            .timestamp_millis()
    }

    fn file() -> SnapshotFile {
        SnapshotFile {
            schema: 1,
            updated_at: NOW.into(),
            providers: vec![
                provider(
                    "claude",
                    "Claude",
                    "ok",
                    vec![
                        window("five_hour", 72.6, Some("2026-10-02T11:12:30Z")),
                        window("seven_day", 41.0, Some("2026-10-05T10:00:00Z")),
                        ExportWindow {
                            scope: Some("Fable".into()),
                            ..window("seven_day", 99.0, None)
                        },
                    ],
                ),
                provider(
                    "codex",
                    "Codex",
                    "ok",
                    vec![window("five_hour", 12.0, None)],
                ),
                provider("copilot", "GitHub Copilot", "disabled", vec![]),
            ],
        }
    }

    #[test]
    fn no_print_flag_means_a_normal_start() {
        assert_eq!(parse_args(&s(&["--hidden"])), None);
        assert_eq!(parse_args(&s(&[])), None);
    }

    #[test]
    fn parses_format_and_provider_in_both_spellings() {
        let a = parse_args(&s(&["--print"])).unwrap().unwrap();
        assert_eq!((a.format, a.provider), (Format::Line, None));
        let a = parse_args(&s(&["--print", "--format", "json", "--provider", "Claude"]))
            .unwrap()
            .unwrap();
        assert_eq!(
            (a.format, a.provider.as_deref()),
            (Format::Json, Some("claude"))
        );
        let a = parse_args(&s(&["--format=statusline", "--print"]))
            .unwrap()
            .unwrap();
        assert_eq!(a.format, Format::Statusline);
        assert!(parse_args(&s(&["--print", "--format", "xml"]))
            .unwrap()
            .is_err());
        assert!(parse_args(&s(&["--print", "--format"])).unwrap().is_err());
        assert!(parse_args(&s(&["--print", "--bogus"])).unwrap().is_err());
    }

    #[test]
    fn countdowns_are_compact() {
        assert_eq!(format_countdown(30), "<1m");
        assert_eq!(format_countdown(45 * 60), "45m");
        assert_eq!(format_countdown(72 * 60), "1h12m");
        assert_eq!(format_countdown(3 * 86_400 + 4 * 3_600 + 59), "3d4h");
    }

    #[test]
    fn the_line_format_matches_the_documented_shape() {
        let args = PrintArgs {
            format: Format::Line,
            provider: None,
        };
        assert_eq!(
            render(&file(), &args, now_ms()).unwrap(),
            "Claude 5h 73% (1h12m) · 7d 41% | Codex 5h 12%"
        );
    }

    #[test]
    fn the_statusline_is_compact_and_flags_trouble() {
        let mut f = file();
        f.providers[0].windows[0].used_percent = 95.0;
        f.providers[1].status = "token_expired".into();
        let args = PrintArgs {
            format: Format::Statusline,
            provider: None,
        };
        assert_eq!(
            render(&f, &args, now_ms()).unwrap(),
            "CC 5h 95%! 7d 41% | CX 5h 12% ?"
        );
    }

    #[test]
    fn json_is_filtered_by_provider_and_unknown_providers_fail() {
        let args = PrintArgs {
            format: Format::Json,
            provider: Some("codex".into()),
        };
        let text = render(&file(), &args, now_ms()).unwrap();
        let back: SnapshotFile = serde_json::from_str(&text).unwrap();
        assert_eq!(back.providers.len(), 1);
        assert_eq!(back.providers[0].id, "codex");
        let args = PrintArgs {
            provider: Some("nope".into()),
            ..args
        };
        assert!(render(&file(), &args, now_ms()).is_err());
    }

    #[test]
    fn a_provider_needing_login_says_so_in_the_line() {
        let mut f = file();
        f.providers[1].status = "not_logged_in".into();
        f.providers[1].windows.clear();
        let args = PrintArgs {
            format: Format::Line,
            provider: Some("codex".into()),
        };
        assert_eq!(
            render(&f, &args, now_ms()).unwrap(),
            "Codex (not signed in)"
        );
    }

    fn run_in(dir: &Path, args: PrintArgs, now: i64) -> (i32, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&args, dir, now, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn exit_codes_cover_missing_stale_and_fresh() {
        let dir = crate::commands::test_support::tempdir();
        let args = PrintArgs {
            format: Format::Line,
            provider: None,
        };

        let (code, out, err) = run_in(&dir, args.clone(), now_ms());
        assert_eq!(code, 2);
        assert!(out.is_empty());
        assert!(
            err.contains("not found") && err.contains("exportSnapshot"),
            "{err}"
        );

        std::fs::write(
            export_snapshot::file_path(&dir),
            serde_json::to_vec(&file()).unwrap(),
        )
        .unwrap();
        let (code, out, _) = run_in(&dir, args.clone(), now_ms() + 14 * 60_000);
        assert_eq!(code, 0);
        assert!(out.starts_with("Claude 5h 73%"), "{out}");

        let (code, out, err) = run_in(&dir, args.clone(), now_ms() + 16 * 60_000);
        assert_eq!(code, 2);
        assert!(out.is_empty());
        assert!(err.contains("stale"), "{err}");

        std::fs::write(export_snapshot::file_path(&dir), "{ not json").unwrap();
        assert_eq!(run_in(&dir, args.clone(), now_ms()).0, 2);

        let mut future = file();
        future.schema = 2;
        std::fs::write(
            export_snapshot::file_path(&dir),
            serde_json::to_vec(&future).unwrap(),
        )
        .unwrap();
        let (code, _, err) = run_in(&dir, args, now_ms());
        assert_eq!(code, 2);
        assert!(err.contains("schema 2"), "{err}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_account_is_selected_with_provider_at_account() {
        let mut f = file();
        f.providers.push(provider(
            "claude@work",
            "Claude Code · Work",
            "ok",
            vec![window("five_hour", 12.0, None)],
        ));
        let args = |p: &str| PrintArgs {
            format: Format::Statusline,
            provider: Some(p.into()),
        };
        assert_eq!(
            render(&f, &args("claude@work"), now_ms()).unwrap(),
            "CC@work 5h 12%"
        );
        let primary = render(&f, &args("claude"), now_ms()).unwrap();
        assert!(
            primary.starts_with("CC ") && !primary.contains("work"),
            "{primary}"
        );
        let parsed = parse_args(&["--print".into(), "--provider".into(), "Claude@Work".into()])
            .unwrap()
            .unwrap();
        assert_eq!(parsed.provider.as_deref(), Some("claude@work"));
    }
}
