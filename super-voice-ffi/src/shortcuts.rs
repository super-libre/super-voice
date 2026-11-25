// SPDX-License-Identifier: GPL-3.0-only
//! The Shortcuts page for an app written in another language: a store that
//! holds the page's state, runs the core's logic for it (`pages::shortcuts`)
//! on the shared runtime, and tells the app whenever the page changed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super_voice_app_core::pages::shortcuts::{self as page, ChangedIn, Effect, Input, Notice, Row};
use super_voice_app_core::state::shortcuts::ShortcutsState;
use super_voice_shared::models::shortcuts::ShortcutId;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ShortcutsAction {
    Refresh,
    Add { shortcut: ShortcutKind },
    Remove { shortcut: ShortcutKind },
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
        crate::identify();
        Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
        })
    }

    pub fn page(&self) -> ShortcutsPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: ShortcutsAction) {
        self.dispatch(match action {
            ShortcutsAction::Refresh => Input::Refresh,
            ShortcutsAction::Add { shortcut } => Input::Add(shortcut.into()),
            ShortcutsAction::Remove { shortcut } => Input::Remove(shortcut.into()),
        });
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
            let store = Arc::clone(&self);
            RUNTIME.spawn(async move {
                let input = match effect {
                    Effect::List => page::run_list().await,
                    Effect::Add(id) => page::run_add(id).await,
                    Effect::Remove(id) => page::run_remove(id).await,
                };
                store.dispatch(input);
            });
        }
    }
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
        notice: held.notice.clone(),
    }
}

/// What the page tells the user, in the settings app's words.
fn notice_text(notice: Notice) -> String {
    let (key, error) = match notice {
        Notice::CouldNotLoad(e) => ("shortcuts-load-failed", e),
        Notice::CouldNotChange(e) => ("shortcut-change-failed", e),
    };
    crate::text_with(key, HashMap::from([("error".to_string(), error)]))
}
