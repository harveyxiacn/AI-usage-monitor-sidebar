//! Pure helpers behind the tray's "Presets" submenu: which entries it has,
//! when it has to be rebuilt and which settings patch a click applies.
//! Nothing here touches Tauri, so it is all unit-tested. [PLATFORM]
//!
//! The built-in presets live in `src/lib/builtin-presets.json`, which the
//! dashboard imports too, so both lists are always the same.

use crate::model::Settings;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Menu-id prefix of every preset entry.
pub const ID_PREFIX: &str = "preset:";

const BUILTIN_JSON: &str = include_str!("../../../src/lib/builtin-presets.json");

/// The built-ins, in menu order, with their English and Chinese labels.
pub const BUILTIN: [(&str, &str, &str); 4] = [
    ("minimal", "Minimal", "极简"),
    ("power", "Power user", "高级用户"),
    ("screenShare", "Screen sharing", "屏幕共享"),
    ("cyber", "Cyber", "赛博"),
];

fn builtin_patches() -> &'static BTreeMap<String, Value> {
    static PATCHES: OnceLock<BTreeMap<String, Value>> = OnceLock::new();
    PATCHES.get_or_init(|| serde_json::from_str(BUILTIN_JSON).unwrap_or_default())
}

/// One entry of the submenu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Menu id (`preset:builtin:power`, `preset:custom:My look`).
    pub id: String,
    pub label: String,
}

/// Menu entries for the current settings: the built-ins, then the user's own
/// presets by name. Custom ones are marked so a name can never be mistaken
/// for a built-in.
pub fn entries(settings: &Settings, chinese: bool) -> (Vec<Entry>, Vec<Entry>) {
    let builtin = BUILTIN
        .iter()
        .filter(|(id, _, _)| builtin_patches().contains_key(*id))
        .map(|(id, en, zh)| Entry {
            id: format!("{ID_PREFIX}builtin:{id}"),
            label: (if chinese { *zh } else { *en }).to_string(),
        })
        .collect();
    let custom = settings
        .custom_presets
        .keys()
        .map(|name| Entry {
            id: format!("{ID_PREFIX}custom:{name}"),
            label: name.clone(),
        })
        .collect();
    (builtin, custom)
}

/// Everything the submenu's shape depends on. The tray rebuilds it only when
/// this string changes, not on every settings write.
pub fn signature(settings: &Settings, chinese: bool) -> String {
    let (builtin, custom) = entries(settings, chinese);
    builtin
        .iter()
        .chain(custom.iter())
        .map(|e| format!("{}={}", e.id, e.label))
        .collect::<Vec<_>>()
        .join("\u{1f}")
}

/// The patch a menu id stands for, `None` for an unknown or removed preset.
pub fn patch_for(settings: &Settings, id: &str) -> Option<Value> {
    let rest = id.strip_prefix(ID_PREFIX)?;
    if let Some(name) = rest.strip_prefix("builtin:") {
        return builtin_patches().get(name).cloned();
    }
    let name = rest.strip_prefix("custom:")?;
    settings.custom_presets.get(name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_listed_builtin_exists_in_the_shared_file() {
        let (builtin, custom) = entries(&Settings::default(), false);
        assert_eq!(builtin.len(), BUILTIN.len());
        assert!(custom.is_empty());
        assert_eq!(builtin_patches().len(), BUILTIN.len(), "no unlisted preset");
        for (id, _, _) in BUILTIN {
            let patch = patch_for(&Settings::default(), &format!("preset:builtin:{id}"));
            assert!(patch.is_some_and(|p| p.is_object()), "{id}");
        }
    }

    #[test]
    fn builtin_patches_are_accepted_by_the_settings_merge() {
        // each preset must change something and survive validation
        let base = Settings::default();
        for (id, _, _) in BUILTIN {
            let patch = patch_for(&base, &format!("preset:builtin:{id}")).unwrap();
            let merged = crate::commands::settings::merge(&base, &patch);
            let differs =
                serde_json::to_value(&merged).unwrap() != serde_json::to_value(&base).unwrap();
            assert!(differs, "{id} should change the defaults");
        }
    }

    #[test]
    fn custom_presets_follow_the_built_ins_and_resolve_by_name() {
        let mut s = Settings::default();
        s.custom_presets
            .insert("Work".into(), json!({"theme": "light"}));
        s.custom_presets
            .insert("Night".into(), json!({"opacity": 0.5}));
        let (_, custom) = entries(&s, true);
        let labels: Vec<_> = custom.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Night", "Work"]);
        assert_eq!(
            patch_for(&s, "preset:custom:Work"),
            Some(json!({"theme": "light"}))
        );
        assert_eq!(patch_for(&s, "preset:custom:Gone"), None);
        assert_eq!(patch_for(&s, "preset:builtin:nope"), None);
        assert_eq!(patch_for(&s, "refresh_now"), None);
    }

    #[test]
    fn labels_follow_the_language() {
        let s = Settings::default();
        assert_eq!(entries(&s, false).0[1].label, "Power user");
        assert_eq!(entries(&s, true).0[1].label, "高级用户");
    }

    #[test]
    fn the_signature_changes_only_with_the_menu_shape() {
        let mut s = Settings::default();
        let before = signature(&s, false);
        s.theme = crate::model::Theme::Light;
        s.polling_paused = true;
        assert_eq!(
            signature(&s, false),
            before,
            "unrelated settings are ignored"
        );
        s.custom_presets.insert("A".into(), json!({}));
        let with_one = signature(&s, false);
        assert_ne!(with_one, before);
        s.custom_presets
            .insert("A".into(), json!({"theme": "dark"}));
        assert_eq!(
            signature(&s, false),
            with_one,
            "editing a patch keeps the menu as is"
        );
        assert_ne!(signature(&s, true), with_one, "language rebuilds it");
    }
}
