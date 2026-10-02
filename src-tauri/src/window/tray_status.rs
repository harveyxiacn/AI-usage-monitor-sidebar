//! Pure helpers behind the tray: language detection, the per-provider usage
//! lines, the worst-severity summary and the severity dot drawn onto the icon.
//! Nothing here touches Tauri, so it is all unit-tested. [PLATFORM]

use crate::model::{AppSnapshot, PercentMode, ProviderQuota, ProviderStatus, Settings};

// ---------- language ----------

/// A locale tag (`zh_CN.UTF-8`, `zh-Hans-CN`, `en-US`) -> "use the Chinese
/// catalogue". Only the language part counts, matching the dashboard.
pub fn locale_is_chinese(tag: &str) -> bool {
    tag.trim().to_ascii_lowercase().starts_with("zh")
}

/// Windows `LANGID` -> Chinese? The primary language id is the low 10 bits.
pub fn langid_is_chinese(langid: u16) -> bool {
    langid & 0x03ff == 0x04
}

/// First entry of `defaults read -g AppleLanguages`
/// (`(\n    "zh-Hans-CN",\n    "en-CN"\n)`).
pub fn first_apple_language(output: &str) -> Option<String> {
    let rest = &output[output.find('"')? + 1..];
    Some(rest[..rest.find('"')?].to_string())
}

#[cfg(not(target_os = "windows"))]
fn env_prefers_chinese() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
        .is_some_and(|v| locale_is_chinese(&v))
}

#[cfg(target_os = "windows")]
pub fn system_prefers_chinese() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    // SAFETY: no arguments and no pointers; returns the user's display language.
    let id = unsafe { GetUserDefaultUILanguage() };
    langid_is_chinese(id)
}

#[cfg(target_os = "macos")]
pub fn system_prefers_chinese() -> bool {
    // A GUI app launched from the Dock has no LANG, so ask the system once.
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        std::process::Command::new("defaults")
            .args(["read", "-g", "AppleLanguages"])
            .output()
            .ok()
            .and_then(|o| first_apple_language(&String::from_utf8_lossy(&o.stdout)))
            .map(|tag| locale_is_chinese(&tag))
            .unwrap_or_else(env_prefers_chinese)
    })
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn system_prefers_chinese() -> bool {
    env_prefers_chinese()
}

// ---------- usage lines ----------

/// "51 min", "1 h 5 min", "2 d 3 h" (zh: "51 分钟", "1 小时 5 分钟", "2 天 3 小时").
pub fn format_duration(ms: i64, chinese: bool) -> String {
    let minutes = (ms.max(0) + 59_999) / 60_000;
    let (d, h, m) = (minutes / 1440, (minutes / 60) % 24, minutes % 60);
    match (chinese, d, h) {
        (false, 0, 0) => format!("{m} min"),
        (false, 0, _) if m == 0 => format!("{h} h"),
        (false, 0, _) => format!("{h} h {m} min"),
        (false, _, 0) => format!("{d} d"),
        (false, _, _) => format!("{d} d {h} h"),
        (true, 0, 0) => format!("{m} 分钟"),
        (true, 0, _) if m == 0 => format!("{h} 小时"),
        (true, 0, _) => format!("{h} 小时 {m} 分钟"),
        (true, _, 0) => format!("{d} 天"),
        (true, _, _) => format!("{d} 天 {h} 小时"),
    }
}

fn shown_percent(used: f64, mode: PercentMode, chinese: bool) -> String {
    let used = if used.is_finite() {
        used.clamp(0.0, 100.0)
    } else {
        0.0
    };
    match mode {
        PercentMode::Used => format!("{}%", used.round() as i64),
        PercentMode::Remaining => {
            let left = (100.0 - used).round() as i64;
            if chinese {
                format!("剩余 {left}%")
            } else {
                format!("{left}% left")
            }
        }
    }
}

/// The window the menu line talks about: the primary one, else the busiest.
fn headline_window(q: &ProviderQuota) -> Option<&crate::model::QuotaWindow> {
    q.windows.iter().find(|w| w.is_primary).or_else(|| {
        q.windows
            .iter()
            .max_by(|a, b| a.used_percent.total_cmp(&b.used_percent))
    })
}

fn status_text(status: ProviderStatus, chinese: bool) -> &'static str {
    match (status, chinese) {
        (ProviderStatus::NotLoggedIn, false) => "not signed in",
        (ProviderStatus::NotLoggedIn, true) => "未登录",
        (ProviderStatus::TokenExpired, false) => "sign-in expired",
        (ProviderStatus::TokenExpired, true) => "登录已过期",
        (ProviderStatus::RateLimited, false) => "rate limited",
        (ProviderStatus::RateLimited, true) => "已被限流",
        (ProviderStatus::Error, false) => "error",
        (ProviderStatus::Error, true) => "出错",
        (_, false) => "no data",
        (_, true) => "暂无数据",
    }
}

/// "Claude · 5h 73% · resets in 51 min" (a read-only tray menu line).
pub fn provider_line(q: &ProviderQuota, settings: &Settings, now_ms: i64, chinese: bool) -> String {
    let mut parts = vec![q.display_name.clone()];
    match headline_window(q) {
        Some(w) if matches!(q.status, ProviderStatus::Ok | ProviderStatus::RateLimited) => {
            parts.push(format!(
                "{} {}",
                w.label,
                shown_percent(w.used_percent, settings.percent_mode, chinese)
            ));
            let reset = w
                .resets_at
                .as_deref()
                .and_then(crate::commands::providers::rfc3339_to_ms);
            if let Some(reset) = reset.filter(|r| *r > now_ms) {
                let d = format_duration(reset - now_ms, chinese);
                parts.push(if chinese {
                    format!("{d}后重置")
                } else {
                    format!("resets in {d}")
                });
            }
            if q.status == ProviderStatus::RateLimited {
                parts.push(status_text(q.status, chinese).to_string());
            }
        }
        _ => parts.push(status_text(q.status, chinese).to_string()),
    }
    parts.join(" · ")
}

fn enabled(settings: &Settings, id: &str) -> bool {
    settings.providers.get(id).is_some_and(|p| p.enabled)
}

fn visible(settings: &Settings, id: &str) -> bool {
    settings
        .providers
        .get(id)
        .is_some_and(|p| p.enabled && p.show_in_sidebar)
}

/// One `(provider id, line)` per enabled provider, in the configured order.
pub fn usage_lines(
    snapshot: &AppSnapshot,
    settings: &Settings,
    now_ms: i64,
    chinese: bool,
) -> Vec<(String, String)> {
    let mut rows: Vec<&ProviderQuota> = snapshot
        .providers
        .iter()
        .filter(|q| enabled(settings, &q.provider))
        .collect();
    rows.sort_by_key(|q| settings.providers.get(&q.provider).map_or(0, |p| p.order));
    rows.into_iter()
        .map(|q| (q.key(), provider_line(q, settings, now_ms, chinese)))
        .collect()
}

// ---------- severity ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Ok,
    Warn,
    Error,
    Critical,
}

/// The busiest window `(provider name, window label, used percent)`.
pub type Busiest = (String, String, f64);

/// Worst severity across the providers shown on the bar, plus the busiest
/// window for the tooltip.
pub fn worst(snapshot: &AppSnapshot, settings: &Settings) -> (Severity, Option<Busiest>) {
    let mut severity = Severity::Ok;
    let mut busiest: Option<Busiest> = None;
    for q in snapshot
        .providers
        .iter()
        .filter(|q| visible(settings, &q.provider))
    {
        if matches!(
            q.status,
            ProviderStatus::Error | ProviderStatus::TokenExpired
        ) {
            severity = severity.max(Severity::Error);
        }
        for w in &q.windows {
            let used = if w.used_percent.is_finite() {
                w.used_percent
            } else {
                0.0
            };
            if used >= settings.thresholds.critical {
                severity = severity.max(Severity::Critical);
            } else if used >= settings.thresholds.warn {
                severity = severity.max(Severity::Warn);
            }
            if busiest.as_ref().is_none_or(|b| used > b.2) {
                busiest = Some((q.display_name.clone(), w.label.clone(), used));
            }
        }
    }
    (severity, busiest)
}

/// Compact tray tooltip: the busiest window, or just the app name.
pub fn tooltip(
    busiest: Option<&Busiest>,
    settings: &Settings,
    focus_on: bool,
    chinese: bool,
) -> String {
    let mut text = match busiest {
        Some((name, label, used)) => format!(
            "AI Usage Sidebar\n{name} · {label} {}",
            shown_percent(*used, settings.percent_mode, chinese)
        ),
        None => "AI Usage Sidebar".to_string(),
    };
    if focus_on {
        text.push_str(if chinese {
            "\n专注模式已开启"
        } else {
            "\nFocus mode on"
        });
    }
    text
}

// ---------- icon dot ----------

fn parse_hex(value: &str) -> Option<[u8; 3]> {
    let v = value.trim().strip_prefix('#')?;
    let digit = |c: char| c.to_digit(16).map(|d| d as u8);
    let chars: Vec<char> = v.chars().collect();
    match chars.len() {
        3 => Some([
            digit(chars[0])? * 17,
            digit(chars[1])? * 17,
            digit(chars[2])? * 17,
        ]),
        6 | 8 => {
            let byte = |i: usize| Some(digit(chars[i])? * 16 + digit(chars[i + 1])?);
            Some([byte(0)?, byte(2)?, byte(4)?])
        }
        _ => None,
    }
}

/// Dot colour for a severity; follows the user's warn/critical colours.
pub fn dot_color(severity: Severity, settings: &Settings) -> Option<[u8; 3]> {
    match severity {
        Severity::Ok => None,
        Severity::Warn => Some(parse_hex(&settings.colors.warn).unwrap_or([245, 197, 66])),
        Severity::Critical => Some(parse_hex(&settings.colors.critical).unwrap_or([255, 59, 48])),
        Severity::Error => Some([167, 139, 250]),
    }
}

/// Paint a filled dot with a thin dark rim in the bottom-right corner of an
/// RGBA image. Returns a new buffer; the input is left untouched.
pub fn overlay_dot(rgba: &[u8], width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    if rgba.len() != (width as usize) * (height as usize) * 4 {
        return out;
    }
    let side = width.min(height) as f32;
    let radius = side * 0.27;
    let rim = (side * 0.06).max(1.0);
    let (cx, cy) = (width as f32 - radius - 0.5, height as f32 - radius - 0.5);
    for y in 0..height {
        for x in 0..width {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > radius + rim {
                continue;
            }
            let px = (y * width + x) as usize * 4;
            let tint = if dist <= radius { color } else { [24, 24, 28] };
            out[px..px + 3].copy_from_slice(&tint);
            out[px + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DataSource, QuotaWindow, WindowKind};

    fn window(label: &str, used: f64, primary: bool, resets: Option<&str>) -> QuotaWindow {
        QuotaWindow {
            kind: WindowKind::FiveHour,
            label: label.into(),
            window_seconds: None,
            used_percent: used,
            resets_at: resets.map(str::to_string),
            scope: None,
            is_primary: primary,
            forecast: None,
        }
    }

    fn quota(
        id: &str,
        name: &str,
        status: ProviderStatus,
        windows: Vec<QuotaWindow>,
    ) -> ProviderQuota {
        ProviderQuota {
            provider: id.into(),
            display_name: name.into(),
            plan: None,
            plan_label: None,
            account: None,
            windows,
            fetched_at: String::new(),
            source: DataSource::Api,
            status,
            error: None,
            credits: None,
            extras: vec![],
            next_attempt_at: None,
            account_id: None,
            account_label: None,
        }
    }

    #[test]
    fn locale_tags_map_to_the_chinese_catalogue() {
        for tag in ["zh_CN.UTF-8", "zh-Hans-CN", "ZH-tw", " zh "] {
            assert!(locale_is_chinese(tag), "{tag}");
        }
        for tag in ["en_US.UTF-8", "C", "", "de-DE"] {
            assert!(!locale_is_chinese(tag), "{tag}");
        }
    }

    #[test]
    fn windows_langids_map_by_primary_language() {
        assert!(langid_is_chinese(0x0804), "zh-CN");
        assert!(langid_is_chinese(0x0404), "zh-TW");
        assert!(!langid_is_chinese(0x0409), "en-US");
        assert!(!langid_is_chinese(0x0407), "de-DE");
    }

    #[test]
    fn the_first_apple_language_is_extracted() {
        let out = "(\n    \"zh-Hans-CN\",\n    \"en-CN\"\n)\n";
        assert_eq!(first_apple_language(out).as_deref(), Some("zh-Hans-CN"));
        assert_eq!(first_apple_language("()"), None);
    }

    #[test]
    fn durations_are_compact_in_both_languages() {
        assert_eq!(format_duration(51 * 60_000, false), "51 min");
        assert_eq!(format_duration(65 * 60_000, false), "1 h 5 min");
        assert_eq!(format_duration(120 * 60_000, false), "2 h");
        assert_eq!(format_duration(27 * 60 * 60_000, false), "1 d 3 h");
        assert_eq!(format_duration(65 * 60_000, true), "1 小时 5 分钟");
        assert_eq!(format_duration(-5, false), "0 min");
    }

    #[test]
    fn a_usage_line_names_window_percent_and_reset() {
        let s = Settings::default();
        let now = 1_700_000_000_000;
        let reset = crate::commands::providers::rfc3339_from_unix_ms(now + 51 * 60_000);
        let q = quota(
            "claude",
            "Claude",
            ProviderStatus::Ok,
            vec![
                window("7d", 10.0, false, None),
                window("5h", 73.4, true, reset.as_deref()),
            ],
        );
        assert_eq!(
            provider_line(&q, &s, now, false),
            "Claude · 5h 73% · resets in 51 min"
        );
        assert_eq!(
            provider_line(&q, &s, now, true),
            "Claude · 5h 73% · 51 分钟后重置"
        );
        let mut remaining = s.clone();
        remaining.percent_mode = PercentMode::Remaining;
        assert!(provider_line(&q, &remaining, now, false).contains("27% left"));
    }

    #[test]
    fn a_provider_without_data_says_why() {
        let s = Settings::default();
        let q = quota("codex", "Codex", ProviderStatus::NotLoggedIn, vec![]);
        assert_eq!(provider_line(&q, &s, 0, false), "Codex · not signed in");
    }

    #[test]
    fn severity_follows_thresholds_and_ignores_hidden_providers() {
        let mut s = Settings::default();
        let snap = AppSnapshot {
            providers: vec![
                quota(
                    "claude",
                    "Claude",
                    ProviderStatus::Ok,
                    vec![window("5h", 75.0, true, None)],
                ),
                quota(
                    "codex",
                    "Codex",
                    ProviderStatus::Ok,
                    vec![window("5h", 95.0, true, None)],
                ),
            ],
            generated_at: String::new(),
        };
        let (sev, busiest) = worst(&snap, &s);
        assert_eq!(sev, Severity::Critical);
        assert_eq!(busiest.unwrap().0, "Codex");
        s.providers.get_mut("codex").unwrap().show_in_sidebar = false;
        assert_eq!(worst(&snap, &s).0, Severity::Warn);
        let err = AppSnapshot {
            providers: vec![quota("claude", "Claude", ProviderStatus::Error, vec![])],
            generated_at: String::new(),
        };
        assert_eq!(worst(&err, &s).0, Severity::Error);
        assert_eq!(worst(&AppSnapshot::default(), &s).0, Severity::Ok);
    }

    #[test]
    fn the_dot_lands_bottom_right_and_leaves_the_rest_alone() {
        let (w, h) = (32u32, 32u32);
        let base = vec![0u8; (w * h * 4) as usize];
        let out = overlay_dot(&base, w, h, [255, 0, 0]);
        let at = |x: u32, y: u32| out[((y * w + x) * 4) as usize..][..4].to_vec();
        assert_eq!(at(0, 0), vec![0, 0, 0, 0], "top-left untouched");
        assert_eq!(at(26, 26), vec![255, 0, 0, 255], "dot centre");
        assert_eq!(base, vec![0u8; base.len()], "input not mutated");
        assert_eq!(
            overlay_dot(&[1, 2, 3], w, h, [1, 1, 1]),
            vec![1, 2, 3],
            "bad size is a no-op"
        );
    }

    #[test]
    fn dot_colours_follow_the_user_palette() {
        let mut s = Settings::default();
        assert_eq!(dot_color(Severity::Ok, &s), None);
        s.colors.warn = "#102030".into();
        assert_eq!(dot_color(Severity::Warn, &s), Some([0x10, 0x20, 0x30]));
        s.colors.critical = "#f00".into();
        assert_eq!(dot_color(Severity::Critical, &s), Some([255, 0, 0]));
        s.colors.critical = "nonsense".into();
        assert_eq!(dot_color(Severity::Critical, &s), Some([255, 59, 48]));
    }
}
