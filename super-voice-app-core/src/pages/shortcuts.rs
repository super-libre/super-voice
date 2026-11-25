// SPDX-License-Identifier: GPL-3.0-only
//! The Shortcuts page: asking the daemon where each shortcut stands, and
//! adding or removing one. The daemon answers every change with the whole
//! list, so the page shows what the desktop now has.
//!
//! On macOS the desktop binds nothing. The shortcut there is the app
//! bundle's listener (`voice hotkey`), which listens for the keys the daemon
//! keeps as its `hotkey` setting: the page reads, changes and resets them,
//! and says whether the listener's agent is set to run.

use super_voice_shared::models::hotkey;
use super_voice_shared::models::shortcuts::{
    Shortcut, ShortcutId, ShortcutList, ShortcutMethod, ShortcutState,
};

use crate::daemon::client::v1::settings::hotkey as hotkey_client;
use crate::daemon::client::v1::shortcuts as client;
use crate::state::connection::AgentStatus;
use crate::state::shortcuts::ShortcutsState;

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Read the list again: the page was opened.
    Refresh,
    Loaded(Result<ShortcutList, String>),
    Add(ShortcutId),
    Remove(ShortcutId),
    /// The daemon's answer to Add or Remove.
    Changed(Result<ShortcutList, String>),
    /// Read the listener's keys and its agent again: the page was opened, on
    /// macOS.
    RefreshListener,
    /// The daemon's answer about the listener's keys: to a read, a change or
    /// a reset.
    HotkeyRead(Result<String, String>),
    /// Listen for this binding instead.
    SetHotkey(String),
    /// Go back to the default binding.
    ResetHotkey,
    /// The daemon's answer to [`Input::SetHotkey`] or [`Input::ResetHotkey`].
    HotkeySaved(Result<String, String>),
    /// Another client changed the keys.
    HotkeyChanged,
    /// The listener's agent was read.
    ListenerRead(AgentStatus),
    /// Register was pressed.
    RegisterListener,
    /// The answer to registering.
    ListenerRegistered(Result<(), String>),
    /// Open Login Items was pressed.
    OpenLoginItems,
}

/// What the app should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    List,
    Add(ShortcutId),
    Remove(ShortcutId),
    /// Read the listener's keys ([`run_read_hotkey`]).
    ReadHotkey,
    /// Store this binding as the listener's keys ([`run_save_hotkey`]).
    SaveHotkey(String),
    /// Go back to the default keys ([`run_reset_hotkey`]).
    ResetHotkey,
    /// Read where the listener's agent stands ([`read_listener`], blocking).
    ReadListener,
    /// Register the listener's agent ([`register_listener`], blocking).
    RegisterListener,
    /// Open System Settings at Login Items ([`open_login_items`]).
    OpenLoginItems,
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    CouldNotLoad(String),
    CouldNotChange(String),
    /// These keys cannot be the shortcut, for this reason: the shared check
    /// said so, or the daemon did.
    HotkeyRefused(String),
    /// The listener's agent could not be registered.
    CouldNotRegister(String),
}

/// What an input came to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub effects: Vec<Effect>,
    /// Take the page's notice down.
    pub clear_notice: bool,
    pub notice: Option<Notice>,
}

/// Take in `input`, and say what to do.
pub fn update(state: &mut ShortcutsState, input: Input) -> Outcome {
    match input {
        Input::Refresh => Outcome {
            effects: vec![Effect::List],
            ..Outcome::default()
        },
        // One change at a time: the desktop may be asking the user about the
        // first, and its button waits for the answer.
        Input::Add(_) | Input::Remove(_) if state.changing.is_some() => Outcome::default(),
        Input::Add(id) => {
            state.changing = Some(id);
            Outcome {
                effects: vec![Effect::Add(id)],
                clear_notice: true,
                notice: None,
            }
        }
        Input::Remove(id) => {
            state.changing = Some(id);
            Outcome {
                effects: vec![Effect::Remove(id)],
                clear_notice: true,
                notice: None,
            }
        }
        Input::Loaded(Ok(list)) => {
            state.list = Some(list);
            Outcome::default()
        }
        Input::Changed(Ok(list)) => {
            state.changing = None;
            state.list = Some(list);
            Outcome::default()
        }
        Input::Loaded(Err(e)) => Outcome {
            notice: Some(Notice::CouldNotLoad(e)),
            ..Outcome::default()
        },
        // Read the list again: what failed may have changed something. The
        // user turning the shortcut down in the desktop's own dialog is their
        // answer, not a failure: the page offers to add it again.
        Input::Changed(Err(e)) => {
            state.changing = None;
            changed_failed(e)
        }
        input => update_listener(state, input),
    }
}

/// A change to the daemon's shortcuts failed.
fn changed_failed(e: String) -> Outcome {
    if turned_down(&e) {
        log::info!("The shortcut was turned down in the desktop's dialog");
        Outcome {
            effects: vec![Effect::List],
            ..Outcome::default()
        }
    } else {
        log::warn!("Could not change a shortcut: {e}");
        Outcome {
            effects: vec![Effect::List],
            clear_notice: false,
            notice: Some(Notice::CouldNotChange(e)),
        }
    }
}

/// The macOS listener's half of [`update`].
fn update_listener(state: &mut ShortcutsState, input: Input) -> Outcome {
    let effects = |effects| Outcome {
        effects,
        ..Outcome::default()
    };
    match input {
        Input::RefreshListener => effects(vec![Effect::ReadHotkey, Effect::ReadListener]),
        Input::HotkeyChanged => effects(vec![Effect::ReadHotkey]),
        Input::HotkeyRead(Ok(binding)) | Input::HotkeySaved(Ok(binding)) => {
            state.hotkey = Some(binding);
            Outcome::default()
        }
        Input::HotkeyRead(Err(e)) => Outcome {
            notice: Some(Notice::CouldNotLoad(e)),
            ..Outcome::default()
        },
        // Checked here first, with the check the daemon makes, so a binding
        // it would refuse is refused at once and in the user's words.
        Input::SetHotkey(binding) => match hotkey::validate(&binding) {
            Ok(parsed) => Outcome {
                effects: vec![Effect::SaveHotkey(parsed.to_string())],
                clear_notice: true,
                notice: None,
            },
            Err(e) => Outcome {
                notice: Some(Notice::HotkeyRefused(e.to_string())),
                ..Outcome::default()
            },
        },
        Input::ResetHotkey => Outcome {
            effects: vec![Effect::ResetHotkey],
            clear_notice: true,
            notice: None,
        },
        // Read the keys again: the page shows what the daemon has.
        Input::HotkeySaved(Err(e)) => {
            log::warn!("Could not change the shortcut's keys: {e}");
            Outcome {
                effects: vec![Effect::ReadHotkey],
                clear_notice: false,
                notice: Some(Notice::HotkeyRefused(e)),
            }
        }
        Input::ListenerRead(status) => {
            state.listener = Some(status);
            Outcome::default()
        }
        Input::RegisterListener => Outcome {
            effects: vec![Effect::RegisterListener],
            clear_notice: true,
            notice: None,
        },
        Input::ListenerRegistered(result) => Outcome {
            effects: vec![Effect::ReadListener],
            clear_notice: false,
            notice: result.err().map(Notice::CouldNotRegister),
        },
        Input::OpenLoginItems => effects(vec![Effect::OpenLoginItems]),
        // The daemon's shortcuts, which `update` takes.
        Input::Refresh
        | Input::Loaded(_)
        | Input::Add(_)
        | Input::Remove(_)
        | Input::Changed(_) => Outcome::default(),
    }
}

/// What the listener's row offers for its agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerFix {
    /// Set to run: nothing to offer.
    None,
    /// Not registered: Register.
    Register,
    /// Switched off in Login Items: Open Login Items.
    TurnOn,
    /// The app is not running from its bundle, which carries the agent:
    /// nothing it can do.
    NotInBundle,
}

/// What the listener's row offers, once its agent was read.
#[must_use]
pub fn listener_fix(state: &ShortcutsState) -> Option<ListenerFix> {
    state.listener.map(|status| match status {
        AgentStatus::Enabled => ListenerFix::None,
        AgentStatus::NotRegistered => ListenerFix::Register,
        AgentStatus::RequiresApproval => ListenerFix::TurnOn,
        AgentStatus::NotInBundle => ListenerFix::NotInBundle,
    })
}

/// The listener's keys, as the page shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyKeys {
    /// The binding, as the daemon has it.
    pub binding: String,
    /// As a Mac shows it: `⌃⌥Space`.
    pub symbols: String,
    /// Whether it is the default, which Reset goes back to.
    pub is_default: bool,
}

/// The listener's keys, once read. A binding this build cannot read (a newer
/// daemon's) shows as the daemon wrote it.
#[must_use]
pub fn hotkey_keys(state: &ShortcutsState) -> Option<HotkeyKeys> {
    let binding = state.hotkey.clone()?;
    Some(match hotkey::Binding::parse(&binding) {
        Ok(parsed) => HotkeyKeys {
            symbols: parsed.symbols(),
            is_default: parsed.is_default(),
            binding,
        },
        Err(_) => HotkeyKeys {
            symbols: binding.clone(),
            is_default: false,
            binding,
        },
    })
}

/// What a key press records as the listener's keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyPress {
    /// These keys: hand `binding` to [`Input::SetHotkey`].
    Keys { binding: String, symbols: String },
    /// A key that types something, pressed with no modifier: keep listening.
    NeedsModifier,
    /// A key the listener cannot use: keep listening.
    Unusable,
}

/// What a key press records, as a toolkit names it: the modifiers held and
/// the key's name, such as `r`, `1`, `space`, `f5`, `up` or `pageup` (GPUI's
/// names, which the binding grammar reads in any case).
#[must_use]
pub fn key_press_named(modifiers: hotkey::Modifiers, key: &str) -> KeyPress {
    // A `+` would read as a separator, and an empty key as no key.
    if key.is_empty() || key.contains('+') {
        return KeyPress::Unusable;
    }
    let mut written = String::new();
    for (modifier, name) in [
        (hotkey::Modifiers::CONTROL, "ctrl"),
        (hotkey::Modifiers::OPTION, "alt"),
        (hotkey::Modifiers::SHIFT, "shift"),
        (hotkey::Modifiers::COMMAND, "cmd"),
    ] {
        if modifiers.contains(modifier) {
            written.push_str(name);
            written.push('+');
        }
    }
    written.push_str(key);
    let Ok(binding) = hotkey::Binding::parse(&written) else {
        return KeyPress::Unusable;
    };
    match binding.check_usable() {
        Ok(()) => KeyPress::Keys {
            binding: binding.to_string(),
            symbols: binding.symbols(),
        },
        Err(hotkey::BindingError::NoModifier) => KeyPress::NeedsModifier,
        Err(_) => KeyPress::Unusable,
    }
}

/// What a daemon event means for the page: another client changed the
/// listener's keys.
#[must_use]
pub fn heard(event: &crate::events::Event) -> Option<Input> {
    (crate::events::setting_changed(event) == Some(hotkey::SETTING)).then_some(Input::HotkeyChanged)
}

/// What a key press on a Mac records: `key_code` is an `NSEvent`'s
/// `keyCode`, `modifier_flags` its `modifierFlags` (the raw value).
#[must_use]
pub fn key_press(key_code: u16, modifier_flags: u64) -> KeyPress {
    let modifiers = hotkey::Modifiers::from_mac_flags(modifier_flags);
    let Some(binding) = hotkey::Binding::from_mac_key(key_code, modifiers) else {
        return KeyPress::Unusable;
    };
    match binding.check_usable() {
        Ok(()) => KeyPress::Keys {
            binding: binding.to_string(),
            symbols: binding.symbols(),
        },
        Err(hotkey::BindingError::NoModifier) => KeyPress::NeedsModifier,
        Err(_) => KeyPress::Unusable,
    }
}

pub async fn run_read_hotkey() -> Input {
    Input::HotkeyRead(hotkey_client::get_hotkey().await.map_err(|e| e.to_string()))
}

pub async fn run_save_hotkey(binding: String) -> Input {
    Input::HotkeySaved(
        hotkey_client::set_hotkey(binding)
            .await
            .map_err(|e| e.to_string()),
    )
}

pub async fn run_reset_hotkey() -> Input {
    Input::HotkeySaved(
        hotkey_client::clear_hotkey()
            .await
            .map_err(|e| e.to_string()),
    )
}

/// Where the listener's agent stands. Blocking: `ServiceManagement` answers
/// over XPC. Outside macOS there is no agent.
#[must_use]
pub fn read_listener() -> Input {
    #[cfg(target_os = "macos")]
    {
        Input::ListenerRead(
            super_voice_shared::launch_agents::Agent::Hotkey
                .status()
                .into(),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        Input::ListenerRead(AgentStatus::NotInBundle)
    }
}

/// Register the listener's agent, which starts it and runs it at every login.
/// Blocking, as [`read_listener`].
#[must_use]
pub fn register_listener() -> Input {
    #[cfg(target_os = "macos")]
    {
        let agent = super_voice_shared::launch_agents::Agent::Hotkey;
        Input::ListenerRegistered(agent.register().map_err(|e| format!("{e:#}")))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Input::ListenerRegistered(Err("there is no shortcut listener here".to_string()))
    }
}

/// Open System Settings at General › Login Items, the one place an agent the
/// user switched off goes back on. Does nothing outside macOS.
pub fn open_login_items() {
    #[cfg(target_os = "macos")]
    super_voice_shared::launch_agents::open_login_items_settings();
}

/// Whether a change failed because the user turned the shortcut down in the
/// desktop's own dialog.
#[must_use]
pub fn turned_down(error: &str) -> bool {
    error.contains("shortcut_refused")
}

/// Where a shortcut the daemon set up is changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangedIn {
    /// COSMIC's keyboard settings.
    Cosmic,
    /// GNOME keeps an app's shortcuts on the app's page in its Settings.
    Gnome,
    /// The desktop's own settings.
    Desktop,
}

/// What a shortcut's row shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// Set up on `keys`, and the daemon can take it off: Remove.
    Removable { keys: String, changed_in: ChangedIn },
    /// Set up through the desktop, which keeps it: changed there, Added
    /// shown greyed out.
    Added { keys: String },
    /// Not set up, its keys free: Add.
    Missing { keys: String },
    /// Not set up, another shortcut holds `keys`: bind `command` by hand.
    Taken { keys: String, command: String },
    /// This desktop has no method the daemon can use: bind `command` in its
    /// settings.
    Elsewhere { command: String },
}

/// What `shortcut`'s row shows, on the desktop `list` describes.
#[must_use]
pub fn row(shortcut: &Shortcut, list: &ShortcutList) -> Row {
    let keys = shortcut.keys.clone();
    let command = shortcut.command.clone();
    match shortcut.state {
        ShortcutState::Installed if list.removable => Row::Removable {
            keys,
            changed_in: if list.method == ShortcutMethod::Cosmic {
                ChangedIn::Cosmic
            } else if is_gnome(&list.desktop) {
                ChangedIn::Gnome
            } else {
                ChangedIn::Desktop
            },
        },
        ShortcutState::Installed => Row::Added { keys },
        ShortcutState::Missing => Row::Missing { keys },
        ShortcutState::Taken => Row::Taken { keys, command },
        ShortcutState::Manual => Row::Elsewhere { command },
    }
}

/// Whether `desktop`, an `XDG_CURRENT_DESKTOP`, names GNOME.
#[must_use]
pub fn is_gnome(desktop: &str) -> bool {
    desktop.split(':').any(|d| d.eq_ignore_ascii_case("GNOME"))
}

pub async fn run_list() -> Input {
    Input::Loaded(client::list_shortcuts().await.map_err(|e| e.to_string()))
}

pub async fn run_add(id: ShortcutId) -> Input {
    Input::Changed(client::add_shortcut(id).await.map_err(|e| e.to_string()))
}

pub async fn run_remove(id: ShortcutId) -> Input {
    Input::Changed(client::remove_shortcut(id).await.map_err(|e| e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the user saying no in the desktop's dialog goes without an error:
    /// every other failure is shown.
    #[test]
    fn only_a_refusal_is_the_user_turning_it_down() {
        assert!(turned_down("shortcut_refused (HTTP 409)"));
        assert!(!turned_down("shortcut_keys_taken (HTTP 409)"));
        let mut state = ShortcutsState::default();
        let refused = update(&mut state, Input::Changed(Err("shortcut_refused".into())));
        assert_eq!(refused.notice, None);
        assert_eq!(refused.effects, [Effect::List]);
        let failed = update(&mut state, Input::Changed(Err("no bus".into())));
        assert_eq!(failed.notice, Some(Notice::CouldNotChange("no bus".into())));
    }

    /// Keys are checked before they are sent: ones the daemon would refuse
    /// are refused at once, and the rest go written the one way.
    #[test]
    fn keys_are_checked_before_they_are_saved() {
        let mut state = ShortcutsState::default();
        let refused = update(&mut state, Input::SetHotkey("r".into()));
        assert!(refused.effects.is_empty());
        assert!(matches!(refused.notice, Some(Notice::HotkeyRefused(_))));

        let saved = update(&mut state, Input::SetHotkey("Cmd+Shift+r".into()));
        assert_eq!(saved.effects, [Effect::SaveHotkey("shift+cmd+KeyR".into())]);
        assert!(saved.clear_notice);

        update(&mut state, Input::HotkeySaved(Ok("shift+cmd+KeyR".into())));
        assert_eq!(
            hotkey_keys(&state),
            Some(HotkeyKeys {
                binding: "shift+cmd+KeyR".into(),
                symbols: "⇧⌘R".into(),
                is_default: false,
            })
        );
    }

    /// The daemon refusing keys says why, and the page reads what it has.
    #[test]
    fn a_refused_change_is_said_and_the_keys_read_again() {
        let mut state = ShortcutsState::default();
        let outcome = update(
            &mut state,
            Input::HotkeySaved(Err("invalid_value (HTTP 400)".into())),
        );
        assert_eq!(
            outcome.notice,
            Some(Notice::HotkeyRefused("invalid_value (HTTP 400)".into()))
        );
        assert_eq!(outcome.effects, [Effect::ReadHotkey]);
    }

    #[test]
    fn reset_and_another_clients_change_read_the_daemons_keys() {
        let mut state = ShortcutsState::default();
        assert_eq!(
            update(&mut state, Input::ResetHotkey).effects,
            [Effect::ResetHotkey]
        );
        assert_eq!(
            update(&mut state, Input::HotkeyChanged).effects,
            [Effect::ReadHotkey]
        );
        assert_eq!(
            update(&mut state, Input::RefreshListener).effects,
            [Effect::ReadHotkey, Effect::ReadListener]
        );
        update(&mut state, Input::HotkeyRead(Ok("ctrl+alt+space".into())));
        let keys = hotkey_keys(&state).expect("read");
        assert_eq!(keys.symbols, "⌃⌥Space");
        assert!(keys.is_default);
    }

    #[test]
    fn the_listener_row_offers_what_its_agent_needs() {
        let mut state = ShortcutsState::default();
        assert_eq!(listener_fix(&state), None);
        for (status, fix) in [
            (AgentStatus::Enabled, ListenerFix::None),
            (AgentStatus::NotRegistered, ListenerFix::Register),
            (AgentStatus::RequiresApproval, ListenerFix::TurnOn),
            (AgentStatus::NotInBundle, ListenerFix::NotInBundle),
        ] {
            update(&mut state, Input::ListenerRead(status));
            assert_eq!(listener_fix(&state), Some(fix));
        }
        assert_eq!(
            update(&mut state, Input::RegisterListener).effects,
            [Effect::RegisterListener]
        );
        let failed = update(&mut state, Input::ListenerRegistered(Err("no".into())));
        assert_eq!(failed.notice, Some(Notice::CouldNotRegister("no".into())));
        assert_eq!(failed.effects, [Effect::ReadListener]);
        assert_eq!(
            update(&mut state, Input::OpenLoginItems).effects,
            [Effect::OpenLoginItems]
        );
    }

    #[test]
    fn a_key_press_records_keys_or_keeps_listening() {
        let command_shift = (1 << 20) | (1 << 17);
        assert_eq!(
            key_press(0x0f, command_shift),
            KeyPress::Keys {
                binding: "shift+cmd+KeyR".into(),
                symbols: "⇧⌘R".into(),
            }
        );
        assert_eq!(key_press(0x0f, 0), KeyPress::NeedsModifier);
        // F5 may go alone.
        assert!(matches!(key_press(0x60, 0), KeyPress::Keys { .. }));
        // A key code the listener has no key for.
        assert_eq!(key_press(0x3f, 1 << 18), KeyPress::Unusable);
    }

    #[test]
    fn a_named_key_press_records_keys_or_keeps_listening() {
        use hotkey::Modifiers;
        assert_eq!(
            key_press_named(Modifiers::COMMAND | Modifiers::SHIFT, "r"),
            KeyPress::Keys {
                binding: "shift+cmd+KeyR".into(),
                symbols: "⇧⌘R".into(),
            }
        );
        assert_eq!(
            key_press_named(Modifiers::CONTROL | Modifiers::OPTION, "space"),
            KeyPress::Keys {
                binding: "ctrl+alt+Space".into(),
                symbols: "⌃⌥Space".into(),
            }
        );
        assert_eq!(
            key_press_named(Modifiers::NONE, "r"),
            KeyPress::NeedsModifier
        );
        assert!(matches!(
            key_press_named(Modifiers::NONE, "f5"),
            KeyPress::Keys { .. }
        ));
        assert_eq!(key_press_named(Modifiers::CONTROL, "+"), KeyPress::Unusable);
        assert_eq!(key_press_named(Modifiers::CONTROL, ""), KeyPress::Unusable);
        assert_eq!(
            key_press_named(Modifiers::CONTROL, "nosuchkey"),
            KeyPress::Unusable
        );
    }

    /// A change waits for the desktop's answer, and a second press meanwhile
    /// is ignored.
    #[test]
    fn one_change_at_a_time() {
        let mut state = ShortcutsState::default();
        let added = update(&mut state, Input::Add(ShortcutId::Dictate));
        assert_eq!(added.effects, [Effect::Add(ShortcutId::Dictate)]);
        assert_eq!(state.changing, Some(ShortcutId::Dictate));
        assert!(
            update(&mut state, Input::Remove(ShortcutId::ReadAloud))
                .effects
                .is_empty()
        );
        update(&mut state, Input::Changed(Err("shortcut_refused".into())));
        assert_eq!(state.changing, None);
        assert_eq!(
            update(&mut state, Input::Remove(ShortcutId::ReadAloud)).effects,
            [Effect::Remove(ShortcutId::ReadAloud)]
        );
    }

    #[test]
    fn gnome_is_found_in_a_desktop_list() {
        assert!(is_gnome("GNOME"));
        assert!(is_gnome("ubuntu:GNOME"));
        assert!(!is_gnome("KDE"));
        assert!(!is_gnome("GNOME-Flashback-less"));
        assert!(!is_gnome(""));
    }

    #[test]
    fn a_set_up_shortcut_is_changed_where_its_desktop_keeps_it() {
        let shortcut = Shortcut {
            id: ShortcutId::Dictate,
            state: ShortcutState::Installed,
            keys: "Super+Space".into(),
            command: "voice record --write".into(),
        };
        let list = |method, removable, desktop: &str| ShortcutList {
            status: "success".into(),
            method,
            removable,
            desktop: desktop.into(),
            shortcuts: Vec::new(),
        };
        assert_eq!(
            row(
                &shortcut,
                &list(ShortcutMethod::Portal, true, "ubuntu:GNOME")
            ),
            Row::Removable {
                keys: "Super+Space".into(),
                changed_in: ChangedIn::Gnome
            }
        );
        assert_eq!(
            row(&shortcut, &list(ShortcutMethod::Portal, false, "Hyprland")),
            Row::Added {
                keys: "Super+Space".into()
            }
        );
    }
}
