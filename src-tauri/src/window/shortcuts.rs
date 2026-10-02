//! Global shortcuts (tauri-plugin-global-shortcut). [PLATFORM]
//!
//! Since the overlays became `_NET_WM_WINDOW_TYPE_DOCK` windows they never get
//! keyboard focus, so a global shortcut is the only keyboard path to the
//! widget. Both shortcuts default to **empty** (= registered nothing): a widget
//! must not steal a key combination the user never asked us to take.
//!
//! What works where is a compositor decision, not ours — see
//! `docs/PLATFORM.md`: on X11/XWayland (the Linux default) this uses
//! `XGrabKey` and works; on a *native* Wayland session there is no protocol
//! for an X11 client to grab a global key, and only some compositors offer
//! one at all.

use crate::model::{
    RegistrationState, Settings, ShortcutRegistration, ShortcutRegistrations, ShortcutStatus,
};
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// What the two settings currently resolve to, plus why they do not work.
#[derive(Default)]
pub struct Shortcuts {
    inner: Mutex<Registered>,
}

#[derive(Default)]
struct Registered {
    toggle_sidebar: Option<Shortcut>,
    open_dashboard: Option<Shortcut>,
    status: ShortcutStatus,
    registrations: ShortcutRegistrations,
}

/// The actions a global shortcut can trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    ToggleSidebar,
    OpenDashboard,
}

/// Parse one settings value.
///
/// * empty / whitespace → `Ok(None)`, the shortcut is simply off;
/// * a combination without a modifier is refused — a bare `U` would swallow
///   that key in every application on the desktop;
/// * anything `global-hotkey` cannot parse comes back as its own message.
pub fn parse(value: &str) -> Result<Option<Shortcut>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let shortcut: Shortcut = value
        .parse()
        .map_err(|e| format!("{value:?} is not a shortcut: {e}"))?;
    if shortcut.mods.is_empty() {
        return Err(format!(
            "{value:?} needs at least one modifier, for example Ctrl+Alt+U"
        ));
    }
    Ok(Some(shortcut))
}

/// Registration outcome of one setting.
///
/// * empty value → `off`;
/// * a native Wayland session cannot grab global keys (no protocol for an X11
///   client, see `docs/PLATFORM.md`), so whatever the value is: `unsupported`;
/// * a recorded failure (unparsable, or the OS refused it) → `failed`;
/// * otherwise `registered`.
pub fn classify(value: &str, failure: Option<&str>, wayland: bool) -> ShortcutRegistration {
    if value.trim().is_empty() {
        return ShortcutRegistration::default();
    }
    if wayland {
        return ShortcutRegistration {
            state: RegistrationState::Unsupported,
            message: Some("native Wayland sessions do not allow global shortcuts".into()),
        };
    }
    match failure {
        Some(message) => ShortcutRegistration {
            state: RegistrationState::Failed,
            message: Some(message.to_string()),
        },
        None => ShortcutRegistration {
            state: RegistrationState::Registered,
            message: None,
        },
    }
}

/// The plugin, with the handler that maps a pressed shortcut to its action.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            // Key-up fires a second event for the same combination.
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let Some(action) = action_of(app.app_handle(), shortcut) else {
                return;
            };
            log::info!("global shortcut {shortcut:?} → {action:?}");
            match action {
                Action::ToggleSidebar => super::tray::toggle_sidebar(app.app_handle()),
                Action::OpenDashboard => super::dashboard::open(app.app_handle(), None),
            }
        })
        .build()
}

fn action_of(app: &AppHandle, shortcut: &Shortcut) -> Option<Action> {
    let state = app.try_state::<Shortcuts>()?;
    let registered = state.inner.lock();
    if registered.toggle_sidebar.as_ref() == Some(shortcut) {
        Some(Action::ToggleSidebar)
    } else if registered.open_dashboard.as_ref() == Some(shortcut) {
        Some(Action::OpenDashboard)
    } else {
        None
    }
}

/// Called from `window::setup` and again on every `settings-updated`.
///
/// Registration is all-or-nothing per shortcut: whatever fails is reported in
/// [`status`] and shown in the settings UI, the other one keeps working.
pub fn apply(app: &AppHandle, settings: &Settings) {
    if app.try_state::<Shortcuts>().is_none() {
        app.manage(Shortcuts::default());
    }
    let manager = app.global_shortcut();
    if let Err(e) = manager.unregister_all() {
        log::debug!("unregistering global shortcuts: {e}");
    }

    let mut next = Registered::default();
    for (value, slot, error) in [
        (
            &settings.shortcut_toggle_sidebar,
            &mut next.toggle_sidebar,
            &mut next.status.toggle_sidebar,
        ),
        (
            &settings.shortcut_open_dashboard,
            &mut next.open_dashboard,
            &mut next.status.open_dashboard,
        ),
    ] {
        match parse(value) {
            Ok(None) => {}
            Ok(Some(shortcut)) => match manager.register(shortcut) {
                Ok(()) => {
                    log::info!("global shortcut {value:?} registered");
                    *slot = Some(shortcut);
                }
                Err(e) => {
                    log::warn!("global shortcut {value:?} could not be registered: {e}");
                    *error = Some(e.to_string());
                }
            },
            Err(e) => {
                log::warn!("global shortcut {value:?} is invalid: {e}");
                *error = Some(e);
            }
        }
    }

    let wayland = cfg!(target_os = "linux") && super::backend_name() == "wayland";
    next.registrations = ShortcutRegistrations {
        toggle_sidebar: classify(
            &settings.shortcut_toggle_sidebar,
            next.status.toggle_sidebar.as_deref(),
            wayland,
        ),
        open_dashboard: classify(
            &settings.shortcut_open_dashboard,
            next.status.open_dashboard.as_deref(),
            wayland,
        ),
    };

    if let Some(state) = app.try_state::<Shortcuts>() {
        *state.inner.lock() = next;
    }
}

/// Why the configured shortcuts are (not) active — for the settings UI.
pub fn status(app: &AppHandle) -> ShortcutStatus {
    match app.try_state::<Shortcuts>() {
        Some(state) => state.inner.lock().status.clone(),
        None => ShortcutStatus::default(),
    }
}

#[tauri::command]
pub async fn get_shortcut_status(app: AppHandle) -> Result<ShortcutStatus, String> {
    Ok(status(&app))
}

/// Per shortcut: off / registered / failed / unsupported, for the recorder.
#[tauri::command]
pub async fn get_shortcut_registrations(app: AppHandle) -> Result<ShortcutRegistrations, String> {
    Ok(match app.try_state::<Shortcuts>() {
        Some(state) => state.inner.lock().registrations.clone(),
        None => ShortcutRegistrations::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_state_follows_value_failure_and_session() {
        assert_eq!(classify("", None, false).state, RegistrationState::Off);
        assert_eq!(classify(" ", Some("x"), true).state, RegistrationState::Off);
        assert_eq!(
            classify("Ctrl+Alt+U", None, false).state,
            RegistrationState::Registered
        );
        let failed = classify("Ctrl+Alt+U", Some("already registered"), false);
        assert_eq!(failed.state, RegistrationState::Failed);
        assert_eq!(failed.message.as_deref(), Some("already registered"));
        assert_eq!(
            classify("Ctrl+Alt+U", None, true).state,
            RegistrationState::Unsupported
        );
    }

    /// Every key name the dashboard's recorder can produce
    /// (`shortcut-recorder.ts`) must parse here, or a recorded shortcut would
    /// be refused.
    #[test]
    fn every_recorder_key_name_parses() {
        let mut keys: Vec<String> = ('A'..='Z').map(String::from).collect();
        keys.extend((0..=9).map(|d| d.to_string()));
        keys.extend((1..=24).map(|n| format!("F{n}")));
        keys.extend(
            [
                "Space", "Enter", "Tab", "Up", "Down", "Left", "Right", "Home", "End", "PageUp",
                "PageDown", "Insert", "Delete",
            ]
            .map(String::from),
        );
        for key in keys {
            for combo in [format!("Ctrl+Alt+Shift+Super+{key}"), format!("Ctrl+{key}")] {
                assert!(parse(&combo).unwrap().is_some(), "{combo}");
            }
        }
    }

    #[test]
    fn an_empty_setting_disables_the_shortcut() {
        assert_eq!(parse(""), Ok(None));
        assert_eq!(parse("   "), Ok(None));
    }

    #[test]
    fn a_normal_combination_parses() {
        let shortcut = parse("Ctrl+Alt+U").unwrap().expect("a shortcut");
        // `Control`, not `CommandOrControl`: the latter is Cmd on macOS, and the
        // user typed Ctrl.
        assert_eq!(shortcut, "Control+Alt+KeyU".parse().unwrap());
        assert!(parse("Shift+Super+D").unwrap().is_some());
    }

    #[test]
    fn a_bare_key_is_refused_so_we_never_swallow_it_desktop_wide() {
        let error = parse("U").unwrap_err();
        assert!(error.contains("modifier"), "{error}");
    }

    #[test]
    fn nonsense_reports_the_parser_message_instead_of_panicking() {
        let error = parse("Ctrl+NotAKey").unwrap_err();
        assert!(error.contains("Ctrl+NotAKey"), "{error}");
    }
}
