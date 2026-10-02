//! Settings files written by released versions, and hostile files, through
//! both entry points: `settings::load` (start-up) and `settings::reload_action`
//! (hot reload). Fixture provenance: `tests/fixtures/settings/README.md`.
//!
//! Invariants checked here:
//! * a file the app cannot read never replaces live settings on a hot reload;
//! * at start-up it falls back to the defaults, leaves the file untouched, and
//!   the next save keeps a copy (`settings.json.bad-<ms>`);
//! * every field a file specifies with a valid value survives, whichever
//!   version wrote the file.

use crate::commands::settings::{self, ReloadAction};
use crate::commands::test_support::tempdir;
use crate::model::Settings;
use serde_json::{json, Value};
use std::path::PathBuf;

const FILES: &[(&str, &str, &str)] = &[
    (
        "v0.2.2-default",
        include_str!("../../tests/fixtures/settings/v0.2.2-default.json"),
        "v0.2.2",
    ),
    (
        "v0.2.2-customised",
        include_str!("../../tests/fixtures/settings/v0.2.2-customised.json"),
        "v0.2.2",
    ),
    (
        "v0.4.0-default",
        include_str!("../../tests/fixtures/settings/v0.4.0-default.json"),
        "v0.4.0",
    ),
    (
        "v0.4.0-customised",
        include_str!("../../tests/fixtures/settings/v0.4.0-customised.json"),
        "v0.4.0",
    ),
    (
        "v0.6.0-default",
        include_str!("../../tests/fixtures/settings/v0.6.0-default.json"),
        "v0.6.0",
    ),
    (
        "v0.6.0-customised",
        include_str!("../../tests/fixtures/settings/v0.6.0-customised.json"),
        "v0.6.0",
    ),
];

fn startup(bytes: &[u8]) -> (Settings, PathBuf) {
    let dir = tempdir();
    std::fs::write(settings::settings_path(&dir), bytes).unwrap();
    (settings::load(&dir), dir)
}

fn to_value(s: &Settings) -> Value {
    serde_json::to_value(s).unwrap()
}

/// What a hot reload of `bytes` leaves live, given the current `live`.
/// `None` = the reload waited (the file is not usable yet).
fn reload(bytes: &[u8], live: &Settings) -> Option<Settings> {
    match settings::reload_action(Some(bytes), None, live) {
        ReloadAction::Wait => None,
        ReloadAction::Ignore => Some(live.clone()),
        ReloadAction::Apply(next) => Some(*next),
    }
}

/// A recognisable, non-default live state.
fn live() -> Settings {
    let mut s = settings::parse(FILES[5].1).unwrap();
    s.onboarded = true;
    s.last_seen_version = "0.6.0".into();
    s.skipped_version = "0.6.1".into();
    s
}

fn defaults_onboarded() -> Settings {
    Settings {
        onboarded: true,
        ..Settings::default()
    }
}

/// Keys whose value is interpreted per OS (an absolute path is absolute only
/// on one platform) and is covered by the dedicated account tests.
const PLATFORM_DEPENDENT: &[&str] = &["accounts"];

/// JSON equality that does not care whether a number was written `150` or `150.0`.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

fn assert_preserved(label: &str, file: &Value, loaded: &Settings) {
    let loaded = to_value(loaded);
    for (key, want) in file.as_object().unwrap() {
        if PLATFORM_DEPENDENT.contains(&key.as_str()) {
            continue;
        }
        match (want, &loaded[key]) {
            (Value::Object(w), Value::Object(l)) if key != "customPresets" => {
                for (k, v) in w {
                    assert!(
                        l.get(k).is_some_and(|x| same(x, v)),
                        "{label}: {key}.{k} was changed: {:?} -> {:?}",
                        v,
                        l.get(k)
                    );
                }
            }
            (w, l) => assert!(same(l, w), "{label}: `{key}` was changed: {w} -> {l}"),
        }
    }
}

// ---------- released versions ----------

#[test]
fn files_from_every_released_version_keep_everything_they_specify() {
    for (name, text, _) in FILES {
        let file: Value = serde_json::from_str(text).unwrap();
        let (loaded, dir) = startup(text.as_bytes());
        assert_preserved(&format!("{name} (start-up)"), &file, &loaded);
        let applied = reload(text.as_bytes(), &Settings::default())
            .unwrap_or_else(|| panic!("{name}: a valid file must not be waited out"));
        assert_preserved(&format!("{name} (hot reload)"), &file, &applied);
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn a_file_without_the_onboarded_key_counts_as_onboarded_at_startup() {
    for (name, text, version) in FILES {
        let (loaded, dir) = startup(text.as_bytes());
        let file: Value = serde_json::from_str(text).unwrap();
        let mentions = file.get("onboarded").is_some();
        // v0.6.0 wrote the key itself (false until the wizard was finished)
        assert_eq!(mentions, *version == "v0.6.0", "{name}: fixture shape");
        let expected = if mentions {
            file["onboarded"].as_bool().unwrap()
        } else {
            true
        };
        assert_eq!(loaded.onboarded, expected, "{name}");
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn keys_added_after_a_version_load_with_their_defaults() {
    let (old, dir) = startup(FILES[0].1.as_bytes());
    let d = Settings::default();
    assert_eq!(old.percent_position, d.percent_position);
    assert_eq!(old.auto_pricing_check, d.auto_pricing_check);
    assert_eq!(old.quota_retention_days, 365);
    assert!(old.providers.contains_key("openrouter"));
    assert!(
        !old.providers["openrouter"].enabled,
        "opt-in provider stays off"
    );
    assert_eq!(old.webhook, d.webhook);
    assert!(old.accounts.is_empty());
    assert!(old.sidebar_animations);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn the_default_fixtures_still_match_todays_defaults() {
    // Documents every default that changed since a release. A user who never
    // touched a setting keeps the value their version wrote, so a difference
    // here is a deliberate behaviour change that must be intentional.
    let today = to_value(&Settings::default());
    for (name, text, _) in FILES.iter().filter(|(n, _, _)| n.ends_with("-default")) {
        let file: Value = serde_json::from_str(text).unwrap();
        for (key, v) in file.as_object().unwrap() {
            if key == "providers" || key == "onboarded" {
                continue; // copilot depends on local credentials
            }
            if let (Value::Object(w), Value::Object(t)) = (v, &today[key]) {
                for (k, v) in w {
                    assert!(
                        t.get(k).is_some_and(|x| same(x, v)),
                        "{name}: default {key}.{k} changed"
                    );
                }
            } else {
                assert!(same(&today[key], v), "{name}: default `{key}` changed");
            }
        }
    }
}

#[test]
fn removed_and_unknown_keys_are_tolerated_and_dropped_on_save() {
    let mut file: Value = serde_json::from_str(FILES[1].1).unwrap();
    file["removedInV0_9"] = json!({"nested": [1, 2, 3]});
    file["showScopedRingTypo"] = json!(true);
    let text = serde_json::to_string(&file).unwrap();
    let (loaded, dir) = startup(text.as_bytes());
    assert_eq!(loaded.theme, settings::parse(FILES[1].1).unwrap().theme);
    assert_preserved(
        "unknown keys",
        &serde_json::from_str(FILES[1].1).unwrap(),
        &loaded,
    );
    settings::save(&dir, &loaded).unwrap();
    let saved = std::fs::read_to_string(settings::settings_path(&dir)).unwrap();
    assert!(!saved.contains("removedInV0_9"));
    assert!(
        !std::fs::read_dir(&dir).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".bad-")),
        "a readable file needs no .bad copy"
    );
    std::fs::remove_dir_all(dir).ok();
}

// ---------- readable but unusual files ----------

fn customised() -> String {
    FILES[1].1.to_string()
}

fn expect_customised(label: &str, s: &Settings) {
    assert_preserved(label, &serde_json::from_str(FILES[1].1).unwrap(), s);
}

#[test]
fn bom_utf16_and_crlf_encodings_of_a_good_file_all_apply() {
    let text = customised();
    let utf8_bom = [b"\xEF\xBB\xBF".as_slice(), text.as_bytes()].concat();
    let crlf = text.replace('\n', "\r\n");
    let utf16 = |le: bool, s: &str| {
        let mut out = if le {
            vec![0xff, 0xfe]
        } else {
            vec![0xfe, 0xff]
        };
        for u in s.encode_utf16() {
            out.extend(if le { u.to_le_bytes() } else { u.to_be_bytes() });
        }
        out
    };
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("utf-8 bom", utf8_bom.clone()),
        ("crlf", crlf.clone().into_bytes()),
        (
            "utf-8 bom + crlf",
            [b"\xEF\xBB\xBF".as_slice(), crlf.as_bytes()].concat(),
        ),
        ("utf-16 le bom (PowerShell 5 redirect)", utf16(true, &text)),
        ("utf-16 be bom", utf16(false, &text)),
        ("utf-16 le bom + crlf", utf16(true, &crlf)),
        (
            "leading blank lines and tabs",
            format!("\n\n\t{text}\n\n").into_bytes(),
        ),
    ];
    for (name, bytes) in cases {
        let (loaded, dir) = startup(&bytes);
        expect_customised(&format!("{name} (start-up)"), &loaded);
        let applied =
            reload(&bytes, &Settings::default()).unwrap_or_else(|| panic!("{name}: reload waited"));
        expect_customised(&format!("{name} (reload)"), &applied);
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn a_two_megabyte_file_with_unknown_junk_is_fine() {
    let mut file: Value = serde_json::from_str(FILES[1].1).unwrap();
    file["junk"] = json!("x".repeat(2 * 1024 * 1024));
    let bytes = serde_json::to_vec(&file).unwrap();
    assert!(bytes.len() > 2 * 1024 * 1024);
    let (loaded, dir) = startup(&bytes);
    expect_customised("2 MB (start-up)", &loaded);
    expect_customised(
        "2 MB (reload)",
        &reload(&bytes, &Settings::default()).unwrap(),
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn nested_junk_within_the_parser_limit_is_ignored() {
    let deep = format!("{}1{}", "[".repeat(100), "]".repeat(100));
    let text = format!(
        r#"{{"junk": {deep}, "theme": "light", "future": {{"a": {{"b": {{"c": null}}}}}}}}"#
    );
    let (loaded, dir) = startup(text.as_bytes());
    assert_eq!(to_value(&loaded)["theme"], "light");
    assert_eq!(
        reload(text.as_bytes(), &Settings::default()).unwrap().theme,
        loaded.theme
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn duplicate_keys_resolve_to_the_last_one() {
    let text = r#"{"theme":"light","edge":"left","theme":"dark","onboarded":true}"#;
    let (loaded, dir) = startup(text.as_bytes());
    let v = to_value(&loaded);
    assert_eq!(
        (v["theme"].as_str(), v["edge"].as_str()),
        (Some("dark"), Some("left"))
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_wrong_type_in_any_field_costs_only_that_field() {
    let defaults = to_value(&defaults_onboarded());
    let wrong = |v: &Value| match v {
        Value::String(_) => json!(12345),
        Value::Number(_) | Value::Bool(_) => json!("not-a-number"),
        Value::Object(_) => json!([1, 2]),
        Value::Array(_) => json!({"x": 1}),
        Value::Null => json!(["x"]),
    };
    for (key, default) in defaults.as_object().unwrap() {
        let file = json!({ key: wrong(default), "scale": 1.25, "theme": "light" });
        let text = file.to_string();
        let (loaded, dir) = startup(text.as_bytes());
        let after = reload(text.as_bytes(), &defaults_onboarded()).unwrap();
        for (label, s) in [("start-up", &loaded), ("reload", &after)] {
            let v = to_value(s);
            if key != "scale" && key != "theme" {
                assert_eq!(v[key], *default, "wrong `{key}` must fall back ({label})");
                assert_eq!(v["scale"], json!(1.25), "`{key}` poisoned scale ({label})");
                assert_eq!(
                    v["theme"],
                    json!("light"),
                    "`{key}` poisoned theme ({label})"
                );
            }
        }
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn a_wrong_type_inside_a_group_costs_only_that_member() {
    let text = r##"{"thresholds":{"warn":"high","critical":95},
                   "colors":{"claude":7,"codex":"#010203"},
                   "sizes":{"ringSize":"big","barGap":12},
                   "sidebarItems":{"weekly":"no","logo":false},
                   "providers":{"claude":{"enabled":"off","order":3},"codex":{"enabled":false}},
                   "onboarded":true}"##;
    let (s, dir) = startup(text.as_bytes());
    let d = Settings::default();
    assert_eq!(s.thresholds.warn, d.thresholds.warn);
    assert_eq!(s.thresholds.critical, 95.0);
    assert_eq!(s.colors.claude, d.colors.claude);
    assert_eq!(s.colors.codex, "#010203");
    assert_eq!(s.sizes.ring_size, d.sizes.ring_size);
    assert_eq!(s.sizes.bar_gap, 12.0);
    assert!(s.sidebar_items.weekly && !s.sidebar_items.logo);
    // a provider entry is merged as a unit: one bad member drops that entry
    assert_eq!(s.providers["claude"], d.providers["claude"]);
    assert!(!s.providers["codex"].enabled);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn out_of_range_numbers_are_clamped_not_reset() {
    let text = r#"{"opacity":99,"scale":-5,"refreshIntervalSec":0,"verticalOffset":99999999,
        "autoHideDelayMs":100000000000,"popoverTimeoutSec":18446744073709551615,
        "monthlyBudgetUsd":-50,"quotaRetentionDays":4000000000,
        "thresholds":{"warn":150,"critical":-5},"sizes":{"ringSize":5000,"labelSize":0},
        "theme":"light","onboarded":true}"#;
    let (s, dir) = startup(text.as_bytes());
    let again = settings::clamp(s.clone());
    assert_eq!(again, s, "clamp must be idempotent");
    assert_eq!(s.opacity, 1.0);
    assert_eq!(s.scale, 0.75);
    assert!(s.refresh_interval_sec >= 1);
    assert!(s.monthly_budget_usd == 0.0);
    assert!(s.quota_retention_days <= 3650);
    assert!(s.thresholds.warn < s.thresholds.critical);
    assert!((40.0..=96.0).contains(&s.sizes.ring_size));
    assert_eq!(to_value(&s)["theme"], "light", "valid neighbours survive");
    let hot = reload(text.as_bytes(), &Settings::default()).unwrap();
    assert_eq!(hot, s);
    std::fs::remove_dir_all(dir).ok();
}

// ---------- the app's own records ----------

#[test]
fn a_hot_reload_keeps_the_app_records_the_file_does_not_mention() {
    let live = live();
    for text in [
        r#"{"theme":"dark"}"#,
        "{}",
        r#"{"theme":"dark","onboarded":null,"lastSeenVersion":7,"skippedVersion":[]}"#,
        r#"{"theme":"dark","onboarded":"yes"}"#,
    ] {
        let after = reload(text.as_bytes(), &live).unwrap();
        assert!(after.onboarded, "{text}: the wizard must not come back");
        assert_eq!(after.last_seen_version, "0.6.0", "{text}");
        assert_eq!(after.skipped_version, "0.6.1", "{text}");
    }
    // …but a file that does say something wins
    let after = reload(
        br#"{"onboarded":false,"lastSeenVersion":"0.7.0","skippedVersion":""}"#,
        &live,
    )
    .unwrap();
    assert!(!after.onboarded);
    assert_eq!(after.last_seen_version, "0.7.0");
    assert_eq!(after.skipped_version, "");
}

#[test]
fn at_startup_only_a_boolean_onboarded_counts_as_present() {
    for (text, want) in [
        (r#"{"theme":"dark"}"#, true),
        (r#"{"onboarded":"yes"}"#, true),
        (r#"{"onboarded":null}"#, true),
        (r#"{"onboarded":false}"#, false),
        (r#"{"onboarded":true}"#, true),
    ] {
        let (s, dir) = startup(text.as_bytes());
        assert_eq!(s.onboarded, want, "{text}");
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn an_empty_object_resets_preferences_by_design_but_never_the_app_records() {
    // `{}` is a valid, deliberately minimal file: keys it omits take their
    // defaults (AGENTS.md section 5). That is the documented contract.
    let after = reload(b"{}", &live()).unwrap();
    let expected = Settings {
        onboarded: true,
        last_seen_version: "0.6.0".into(),
        skipped_version: "0.6.1".into(),
        ..Settings::default()
    };
    assert_eq!(after, expected);
}

// ---------- unusable files ----------

fn utf16_le(s: &str) -> Vec<u8> {
    let mut out = vec![0xff, 0xfe];
    for u in s.encode_utf16() {
        out.extend(u.to_le_bytes());
    }
    out
}

/// `(name, bytes, is_blank)`; blank files hold nothing worth keeping.
fn unusable() -> Vec<(&'static str, Vec<u8>, bool)> {
    let good = customised();
    let truncated = good.as_bytes()[..good.len() / 2].to_vec();
    let mut odd_utf16 = utf16_le("{}");
    odd_utf16.pop();
    let mut lone_surrogate = vec![0xff, 0xfe];
    lone_surrogate.extend([0x00, 0xd8, b'{', 0x00]);
    let too_deep = format!("{}1{}", "[".repeat(5000), "]".repeat(5000));
    vec![
        ("empty file", vec![], true),
        ("whitespace only", b" \r\n\t\r\n".to_vec(), true),
        ("half-written (first half)", truncated, false),
        (
            "half-written mid-string",
            br#"{"theme":"li"#.to_vec(),
            false,
        ),
        (
            "half-written after comma",
            br#"{"theme":"light","#.to_vec(),
            false,
        ),
        (
            "trailing garbage",
            format!("{good}\n}}garbage").into_bytes(),
            false,
        ),
        (
            "two documents",
            br#"{"theme":"light"}{"theme":"dark"}"#.to_vec(),
            false,
        ),
        ("trailing comma", br#"{"theme":"light",}"#.to_vec(), false),
        ("single quotes", b"{'theme': 'light'}".to_vec(), false),
        (
            "comments",
            b"{ // dark please\n \"theme\": \"dark\" }".to_vec(),
            false,
        ),
        ("null root", b"null".to_vec(), false),
        ("array root", b"[]".to_vec(), false),
        ("array of settings", format!("[{good}]").into_bytes(), false),
        ("string root", br#""theme""#.to_vec(), false),
        ("number root", b"42".to_vec(), false),
        ("bool root", b"true".to_vec(), false),
        ("utf-8 bom only", b"\xEF\xBB\xBF".to_vec(), false),
        (
            "invalid utf-8",
            b"{\"monitor\":\"\xff\xfe\"}".to_vec(),
            false,
        ),
        (
            "legacy codepage text",
            b"{\"monitor\":\"\xb1\xea\xbb\xb0\"}".to_vec(),
            false,
        ),
        ("utf-16 odd length", odd_utf16, false),
        ("utf-16 lone surrogate", lone_surrogate, false),
        (
            "utf-16 without bom",
            {
                let mut v = vec![];
                for u in "{}".encode_utf16() {
                    v.extend(u.to_le_bytes());
                }
                v
            },
            false,
        ),
        ("nul bytes", vec![0u8; 64], false),
        (
            "number out of range",
            br#"{"opacity":1e999}"#.to_vec(),
            false,
        ),
        (
            "nesting beyond the parser limit",
            too_deep.into_bytes(),
            false,
        ),
        ("binary noise", (0..=255u8).collect(), false),
    ]
}

#[test]
fn startup_with_an_unusable_file_uses_defaults_and_leaves_the_file_alone() {
    for (name, bytes, _) in unusable() {
        let (loaded, dir) = startup(&bytes);
        assert_eq!(loaded, Settings::default(), "{name}");
        assert_eq!(
            std::fs::read(settings::settings_path(&dir)).unwrap(),
            bytes,
            "{name}: loading must not touch the file"
        );
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn a_hot_reload_never_applies_an_unusable_file() {
    let live = live();
    for (name, bytes, _) in unusable() {
        assert!(
            reload(&bytes, &live).is_none(),
            "{name}: live settings must not be replaced (nor defaults applied)"
        );
    }
    // unreadable at the moment of the event
    assert!(matches!(
        settings::reload_action(None, None, &live),
        ReloadAction::Wait
    ));
}

#[test]
fn every_prefix_of_a_good_file_is_waited_out() {
    let live = live();
    let good = customised();
    let trimmed = good.trim_end().len();
    for cut in 0..trimmed {
        let prefix = &good.as_bytes()[..cut];
        assert!(
            reload(prefix, &live).is_none(),
            "a file cut after {cut} of {trimmed} bytes was applied: {:?}",
            String::from_utf8_lossy(prefix)
        );
    }
    assert!(reload(good.as_bytes(), &Settings::default()).is_some());
}

#[test]
fn saving_over_an_unusable_file_keeps_a_copy_first() {
    for (name, bytes, blank) in unusable() {
        let (loaded, dir) = startup(&bytes);
        settings::save(&dir, &loaded).unwrap();
        let copies: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("settings.json.bad-")
            })
            .collect();
        if blank {
            assert!(copies.is_empty(), "{name}: nothing to preserve");
        } else {
            assert_eq!(copies.len(), 1, "{name}: exactly one copy expected");
            assert_eq!(std::fs::read(copies[0].path()).unwrap(), bytes, "{name}");
        }
        // the live file is now a good one
        assert!(
            settings::parse(&std::fs::read_to_string(settings::settings_path(&dir)).unwrap())
                .is_some()
        );
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn bad_copies_are_capped() {
    let dir = tempdir();
    let path = settings::settings_path(&dir);
    for i in 0..6 {
        std::fs::write(&path, format!("{{ broken {i}")).unwrap();
        settings::save(&dir, &Settings::default()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    let n = std::fs::read_dir(&dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("settings.json.bad-")
        })
        .count();
    assert_eq!(n, 3);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_missing_file_is_defaults_and_is_not_created_by_loading() {
    let dir = tempdir();
    assert_eq!(settings::load(&dir), Settings::default());
    assert!(!settings::settings_path(&dir).exists());
    std::fs::remove_dir_all(dir).ok();
}
