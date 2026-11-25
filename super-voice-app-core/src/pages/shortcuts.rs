// SPDX-License-Identifier: GPL-3.0-only
//! The Shortcuts page: asking the daemon where each shortcut stands, and
//! adding or removing one. The daemon answers every change with the whole
//! list, so the page shows what the desktop now has.

use super_voice_shared::models::shortcuts::{
    Shortcut, ShortcutId, ShortcutList, ShortcutMethod, ShortcutState,
};

use crate::daemon::client::v1::shortcuts as client;
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
}

/// What the app should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    List,
    Add(ShortcutId),
    Remove(ShortcutId),
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    CouldNotLoad(String),
    CouldNotChange(String),
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
        Input::Add(id) => Outcome {
            effects: vec![Effect::Add(id)],
            clear_notice: true,
            notice: None,
        },
        Input::Remove(id) => Outcome {
            effects: vec![Effect::Remove(id)],
            clear_notice: true,
            notice: None,
        },
        Input::Loaded(Ok(list)) | Input::Changed(Ok(list)) => {
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
    }
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
