// SPDX-License-Identifier: GPL-3.0-only
//! The Shortcuts page for an app written in another language: a store that
//! holds the page's state, runs the core's logic for it (`pages::shortcuts`)
//! on the shared runtime, and tells the app whenever the page changed.
//!
//! On macOS the page is the shortcut listener's: its keys, which the user
//! changes and resets here, and whether its agent is set to run. The store
//! follows the daemon's events, so keys another client sets show at once.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super_voice_app_core::events::{self, Event};
use super_voice_app_core::pages::shortcuts::{
    self as page, ChangedIn, Effect, Input, KeyPress, ListenerFix, Notice, Row,
};
use super_voice_app_core::state::shortcuts::ShortcutsState;
use super_voice_shared::models::hotkey;
use super_voice_shared::models::shortcuts::ShortcutId;
use tokio::sync::broadcast::error::RecvError;

use crate::RUNTIME;

/// What the app is told. Called from the store's own threads.
#[uniffi::export(with_foreign)]
pub trait ShortcutsObserver: Send + Sync {
    /// The page changed; draw this.
    fn changed(&self, page: ShortcutsPage);
}

/// One of Super Voice's shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ShortcutKind {
    Dictate,
    ReadAloud,
}

impl From<ShortcutId> for ShortcutKind {
    fn from(id: ShortcutId) -> Self {
        match id {
            ShortcutId::Dictate => Self::Dictate,
            ShortcutId::ReadAloud => Self::ReadAloud,
        }
    }
}

impl From<ShortcutKind> for ShortcutId {
    fn from(kind: ShortcutKind) -> Self {
        match kind {
            ShortcutKind::Dictate => Self::Dictate,
            ShortcutKind::ReadAloud => Self::ReadAloud,
        }
    }
}

/// What the user did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ShortcutsAction {
    /// The page was opened. On macOS this reads the listener's keys and its
    /// agent; elsewhere, the daemon's shortcuts as well.
    Refresh,
    Add {
        shortcut: ShortcutKind,
    },
    Remove {
        shortcut: ShortcutKind,
    },
    /// Listen for these keys: a binding [`shortcut_key_press`] gave.
    ChangeHotkey {
        binding: String,
    },
    /// Go back to ⌃⌥Space.
    ResetHotkey,
    /// Register the listener's background service.
    RegisterListener,
    /// Open System Settings at Login Items.
    OpenLoginItems,
}

/// The keys the macOS shortcut listener listens for.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HotkeyKeys {
    /// As the daemon has it: `ctrl+alt+space`.
    pub binding: String,
    /// As a Mac shows it: `⌃⌥Space`.
    pub symbols: String,
    /// Whether it is the default, which Reset goes back to.
    pub is_default: bool,
}

/// Where the shortcut listener's background service stands with macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ShortcutListener {
    /// Set to run, now and at every login.
    Running,
    /// Not registered: Register.
    NotRegistered,
    /// Switched off in Login Items: Open Login Items.
    SwitchedOff,
    /// The app is not running from its bundle, which carries the service.
    NotInBundle,
}

/// What a key press records as the listener's keys.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ShortcutKeyPress {
    /// These keys: send them as [`ShortcutsAction::ChangeHotkey`].
    Keys { binding: String, symbols: String },
    /// A key that types something, pressed with no modifier: keep
    /// listening.
    NeedsModifier,
    /// A key the listener cannot use: keep listening.
    Unusable,
}

/// What a key press records: `key_code` is an `NSEvent`'s `keyCode`,
/// `modifier_flags` its `modifierFlags.rawValue`.
#[must_use]
#[uniffi::export]
pub fn shortcut_key_press(key_code: u16, modifier_flags: u64) -> ShortcutKeyPress {
    match page::key_press(key_code, modifier_flags) {
        KeyPress::Keys { binding, symbols } => ShortcutKeyPress::Keys { binding, symbols },
        KeyPress::NeedsModifier => ShortcutKeyPress::NeedsModifier,
        KeyPress::Unusable => ShortcutKeyPress::Unusable,
    }
}

/// The default keys as a Mac shows them, `⌃⌥Space`: what Reset goes back to.
#[must_use]
#[uniffi::export]
pub fn default_hotkey_symbols() -> String {
    hotkey::Binding::default_binding().symbols()
}

/// Where a set-up shortcut is changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ShortcutChangedIn {
    Cosmic,
    Gnome,
    Desktop,
}

/// What a shortcut's row shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ShortcutRow {
    Removable {
        keys: String,
        changed_in: ShortcutChangedIn,
    },
    Added {
        keys: String,
    },
    Missing {
        keys: String,
    },
    Taken {
        keys: String,
        command: String,
    },
    Elsewhere {
        command: String,
    },
}

/// One shortcut.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ShortcutLine {
    pub shortcut: ShortcutKind,
    pub row: ShortcutRow,
}

/// Everything the page shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ShortcutsPage {
    pub loaded: bool,
    pub shortcuts: Vec<ShortcutLine>,
    /// The macOS shortcut listener's keys, once read.
    pub hotkey: Option<HotkeyKeys>,
    /// Where the listener's background service stands, once read. Only
    /// macOS has one.
    pub listener: Option<ShortcutListener>,
    /// What to tell the user, in the user's language.
    pub notice: Option<String>,
}

#[derive(Default)]
struct Held {
    state: ShortcutsState,
    notice: Option<String>,
}

/// The Shortcuts page's store.
#[derive(uniffi::Object)]
pub struct ShortcutsStore {
    held: Mutex<Held>,
    observer: Arc<dyn ShortcutsObserver>,
}

#[uniffi::export]
impl ShortcutsStore {
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn ShortcutsObserver>) -> Arc<Self> {
        let store = Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
        });
        follow(Arc::downgrade(&store));
        store
    }

    pub fn page(&self) -> ShortcutsPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: ShortcutsAction) {
        let input = match action {
            ShortcutsAction::Refresh => {
                // A Mac's desktop binds nothing: its shortcut is the
                // listener's, so the daemon's list has nothing to show there.
                #[cfg(not(target_os = "macos"))]
                Arc::clone(&self).dispatch(Input::Refresh);
                Input::RefreshListener
            }
            ShortcutsAction::Add { shortcut } => Input::Add(shortcut.into()),
            ShortcutsAction::Remove { shortcut } => Input::Remove(shortcut.into()),
            ShortcutsAction::ChangeHotkey { binding } => Input::SetHotkey(binding),
            ShortcutsAction::ResetHotkey => Input::ResetHotkey,
            ShortcutsAction::RegisterListener => Input::RegisterListener,
            ShortcutsAction::OpenLoginItems => Input::OpenLoginItems,
        };
        self.dispatch(input);
    }
}

impl ShortcutsStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn dispatch(self: Arc<Self>, input: Input) {
        let (page, effects) = {
            let mut held = self.lock();
            let outcome = page::update(&mut held.state, input);
            if outcome.clear_notice {
                held.notice = None;
            }
            if let Some(notice) = outcome.notice {
                held.notice = Some(notice_text(notice));
            }
            (snapshot(&held), outcome.effects)
        };
        self.observer.changed(page);
        for effect in effects {
            // On the caller's thread: the button's, the app's main thread.
            if effect == Effect::OpenLoginItems {
                page::open_login_items();
                continue;
            }
            let store = Arc::clone(&self);
            RUNTIME.spawn(async move {
                let input = match effect {
                    Effect::List => page::run_list().await,
                    Effect::Add(id) => page::run_add(id).await,
                    Effect::Remove(id) => page::run_remove(id).await,
                    Effect::ReadHotkey => page::run_read_hotkey().await,
                    Effect::SaveHotkey(binding) => page::run_save_hotkey(binding).await,
                    Effect::ResetHotkey => page::run_reset_hotkey().await,
                    // `ServiceManagement` answers over XPC: off the runtime.
                    Effect::ReadListener => {
                        match tokio::task::spawn_blocking(page::read_listener).await {
                            Ok(input) => input,
                            Err(_) => return,
                        }
                    }
                    Effect::RegisterListener => {
                        match tokio::task::spawn_blocking(page::register_listener).await {
                            Ok(input) => input,
                            Err(_) => return,
                        }
                    }
                    Effect::OpenLoginItems => return,
                };
                store.dispatch(input);
            });
        }
    }

    /// Whether the listener's keys were read once, so a change is worth
    /// reading again.
    fn showing_hotkey(&self) -> bool {
        self.lock().state.hotkey.is_some()
    }
}

/// Read the listener's keys again when another client changes them, and its
/// keys and agent when the daemon is back, until `store` is dropped.
fn follow(store: Weak<ShortcutsStore>) {
    let mut events = crate::events::listen();
    RUNTIME.spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            };
            let input = match (&event, events::setting_changed(&event)) {
                (Event::Connected, _) => Input::RefreshListener,
                (_, Some(hotkey::SETTING)) => Input::HotkeyChanged,
                _ => continue,
            };
            let Some(store) = store.upgrade() else {
                return;
            };
            if store.showing_hotkey() {
                store.dispatch(input);
            }
        }
    });
}

fn snapshot(held: &Held) -> ShortcutsPage {
    let list = held.state.list.as_ref();
    ShortcutsPage {
        loaded: list.is_some(),
        shortcuts: list.map_or_default(|list| {
            list.shortcuts
                .iter()
                .map(|shortcut| ShortcutLine {
                    shortcut: shortcut.id.into(),
                    row: match page::row(shortcut, list) {
                        Row::Removable { keys, changed_in } => ShortcutRow::Removable {
                            keys,
                            changed_in: match changed_in {
                                ChangedIn::Cosmic => ShortcutChangedIn::Cosmic,
                                ChangedIn::Gnome => ShortcutChangedIn::Gnome,
                                ChangedIn::Desktop => ShortcutChangedIn::Desktop,
                            },
                        },
                        Row::Added { keys } => ShortcutRow::Added { keys },
                        Row::Missing { keys } => ShortcutRow::Missing { keys },
                        Row::Taken { keys, command } => ShortcutRow::Taken { keys, command },
                        Row::Elsewhere { command } => ShortcutRow::Elsewhere { command },
                    },
                })
                .collect()
        }),
        hotkey: page::hotkey_keys(&held.state).map(|keys| HotkeyKeys {
            binding: keys.binding,
            symbols: keys.symbols,
            is_default: keys.is_default,
        }),
        listener: page::listener_fix(&held.state).map(|fix| match fix {
            ListenerFix::None => ShortcutListener::Running,
            ListenerFix::Register => ShortcutListener::NotRegistered,
            ListenerFix::TurnOn => ShortcutListener::SwitchedOff,
            ListenerFix::NotInBundle => ShortcutListener::NotInBundle,
        }),
        notice: held.notice.clone(),
    }
}

/// What the page tells the user, in the settings app's words.
fn notice_text(notice: Notice) -> String {
    let (key, error) = match notice {
        Notice::CouldNotLoad(e) => ("shortcuts-load-failed", e),
        Notice::CouldNotChange(e) => ("shortcut-change-failed", e),
        Notice::HotkeyRefused(e) => ("shortcut-refused", e),
        Notice::CouldNotRegister(e) => ("shortcut-listener-register-failed", e),
    };
    crate::text_with(key, HashMap::from([("error".to_string(), error)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super_voice_app_core::state::connection::AgentStatus;

    // A store is not made here: it follows the daemon's events, which would
    // reach the daemon running on this machine. Its page is `snapshot`.

    /// The page shows the listener's keys as a Mac does, and the state of
    /// its background service.
    #[test]
    fn the_page_shows_the_listeners_keys_and_service() {
        let mut held = Held::default();
        page::update(
            &mut held.state,
            Input::HotkeyRead(Ok("shift+cmd+KeyR".into())),
        );
        page::update(
            &mut held.state,
            Input::ListenerRead(AgentStatus::RequiresApproval),
        );
        let shown = snapshot(&held);
        assert_eq!(
            shown.hotkey,
            Some(HotkeyKeys {
                binding: "shift+cmd+KeyR".into(),
                symbols: "⇧⌘R".into(),
                is_default: false,
            })
        );
        assert_eq!(shown.listener, Some(ShortcutListener::SwitchedOff));
    }

    /// Keys the listener could not use are refused in the user's words,
    /// before anything is sent.
    #[test]
    fn unusable_keys_are_refused_at_once() {
        let mut held = Held::default();
        let outcome = page::update(&mut held.state, Input::SetHotkey("r".into()));
        assert!(outcome.effects.is_empty());
        let notice = notice_text(outcome.notice.expect("a notice"));
        assert!(notice.starts_with("Couldn't use these keys: "), "{notice}");
        assert!(notice.contains("modifier"), "{notice}");
    }

    #[test]
    fn a_key_press_gives_its_keys() {
        assert_eq!(
            shortcut_key_press(0x31, (1 << 18) | (1 << 19)),
            ShortcutKeyPress::Keys {
                binding: "ctrl+alt+Space".into(),
                symbols: "⌃⌥Space".into(),
            }
        );
        assert_eq!(shortcut_key_press(0x31, 0), ShortcutKeyPress::NeedsModifier);
        assert_eq!(default_hotkey_symbols(), "⌃⌥Space");
    }
}
