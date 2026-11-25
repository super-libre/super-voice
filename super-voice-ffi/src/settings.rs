// SPDX-License-Identifier: GPL-3.0-only
//! The Settings page for an app written in another language: a store that
//! holds the page's state, runs the core's logic for it (`pages::settings`) on
//! the shared runtime, reads a setting again when another client changes it,
//! and tells the app whenever the page changed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super_voice_app_core::events::{self, Event};
use super_voice_app_core::languages;
use super_voice_app_core::pages::settings::{
    self as page, Effect, IdleTime, Input, NOTIFICATION_METHODS, Notice,
};
use super_voice_app_core::state::language::LanguageState;
use super_voice_app_core::state::settings::SettingsState;
use super_voice_shared::models::notification_method::NotificationMethod;
use tokio::sync::broadcast::error::RecvError;

use crate::RUNTIME;
use crate::behavior::Choice;

/// What the app is told. Called from the store's own threads.
#[uniffi::export(with_foreign)]
pub trait SettingsObserver: Send + Sync {
    /// The page changed; draw this.
    fn changed(&self, page: SettingsPage);
}

/// What the user did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SettingsAction {
    /// Read the settings again: the page was opened.
    Reload,
    /// Picked how failures are reported, by its id.
    NotificationMethodPicked { id: String },
    /// Switched Sleep When Idle.
    SleepToggled { on: bool },
    /// Picked how long, in seconds.
    IdleUnloadPicked { after_secs: u64 },
    /// Picked a primary language, or `None` to unset it.
    PrimaryLanguagePicked { tag: Option<String> },
}

/// Everything the page shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SettingsPage {
    /// Whether the settings have been read.
    pub loaded: bool,
    /// The primary language's tag; `None` when unset.
    pub primary_language: Option<String>,
    /// Its name, or "Automatic" when unset.
    pub primary_language_name: String,
    pub notification_methods: Vec<Choice>,
    pub notification_method: String,
    /// Whether Sleep When Idle is on.
    pub sleep: bool,
    /// The idle times offered while it is on, with the one in force among
    /// them; each id is its seconds.
    pub idle_choices: Vec<Choice>,
    pub idle_unload_secs: u64,
    /// What to tell the user, in the user's language.
    pub notice: Option<String>,
}

/// The page's state, and the notice it shows.
#[derive(Default)]
struct Held {
    loaded: bool,
    state: SettingsState,
    language: LanguageState,
    notice: Option<String>,
}

/// The Settings page's store.
#[derive(uniffi::Object)]
pub struct SettingsStore {
    held: Mutex<Held>,
    observer: Arc<dyn SettingsObserver>,
}

#[uniffi::export]
impl SettingsStore {
    /// A store that tells `observer` what changes, and follows changes other
    /// clients make. Nothing is read until the app sends
    /// [`SettingsAction::Reload`].
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn SettingsObserver>) -> Arc<Self> {
        crate::identify();
        let store = Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
        });
        follow(Arc::downgrade(&store));
        store
    }

    /// What the page shows now.
    pub fn page(&self) -> SettingsPage {
        snapshot(&self.lock())
    }

    /// The language picker's rows for `query`: the tags the primary language
    /// can be, with their names, sorted by name. Each id is its tag.
    pub fn language_rows(&self, query: &str) -> Vec<Choice> {
        let offers = self.lock().language.global_offers.clone();
        languages::picker_rows(&offers, query)
            .into_iter()
            .map(|(id, name)| Choice { id, name })
            .collect()
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: SettingsAction) {
        let input = match action {
            SettingsAction::Reload => {
                self.reload(Reading::All);
                return;
            }
            SettingsAction::NotificationMethodPicked { id } => match id.parse() {
                Ok(method) => Input::NotificationMethodPicked(method),
                Err(_) => return,
            },
            SettingsAction::SleepToggled { on } => Input::SleepToggled(on),
            SettingsAction::IdleUnloadPicked { after_secs } => Input::IdleUnloadPicked(after_secs),
            SettingsAction::PrimaryLanguagePicked { tag } => Input::PrimaryLanguagePicked(tag),
        };
        self.dispatch(input);
    }
}

/// What to read from the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    All,
    NotificationMethod,
    IdleUnload,
    Language,
}

impl SettingsStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Read `what` from the daemon, and hand it to the page.
    fn reload(self: Arc<Self>, what: Reading) {
        RUNTIME.spawn(async move {
            let all = what == Reading::All;
            let mut inputs = Vec::new();
            if all || what == Reading::NotificationMethod {
                inputs.push(page::run_load_notification_method().await);
            }
            if all || what == Reading::IdleUnload {
                inputs.push(page::run_load_idle_unload().await);
            }
            if all || what == Reading::Language {
                inputs.push(page::run_load_primary_language().await);
                inputs.push(page::run_load_languages().await);
            }
            for input in inputs {
                Arc::clone(&self).dispatch(input);
            }
            if all {
                let page = {
                    let mut held = self.lock();
                    held.loaded = true;
                    snapshot(&held)
                };
                self.observer.changed(page);
            }
        });
    }

    /// Hand `input` to the page's logic, tell the app what changed, and start
    /// the work it asked for.
    fn dispatch(self: Arc<Self>, input: Input) {
        let (page, effects) = {
            let mut held = self.lock();
            let Held {
                state, language, ..
            } = &mut *held;
            let outcome = page::update(state, language, input);
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
            Arc::clone(&self).run(effect);
        }
    }

    /// Start `effect`. Its answer comes back through [`Self::dispatch`].
    fn run(self: Arc<Self>, effect: Effect) {
        RUNTIME.spawn(async move {
            let input = match effect {
                Effect::SaveNotificationMethod(method) => {
                    page::run_save_notification_method(method).await
                }
                Effect::SaveIdleUnload(after_secs) => page::run_save_idle_unload(after_secs).await,
                Effect::SavePrimaryLanguage(tag) => page::run_save_primary_language(tag).await,
                // Nothing this app shows follows the primary language yet.
                Effect::LanguageChanged => return,
            };
            self.dispatch(input);
        });
    }
}

/// Read a setting again when another client changes it, and everything when
/// the daemon is back, until `store` is dropped.
fn follow(store: Weak<SettingsStore>) {
    let mut events = crate::events::listen();
    RUNTIME.spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            };
            let what = match (&event, events::setting_changed(&event)) {
                (Event::Connected, _) => Reading::All,
                (_, Some("notification_method")) => Reading::NotificationMethod,
                (_, Some("idle_unload")) => Reading::IdleUnload,
                (_, Some("language")) => Reading::Language,
                _ => continue,
            };
            let Some(store) = store.upgrade() else {
                return;
            };
            // Until the page has read everything once, it reads it all then.
            if store.lock().loaded {
                store.reload(what);
            }
        }
    });
}

fn snapshot(held: &Held) -> SettingsPage {
    let state = &held.state;
    let primary = held.language.primary_language.clone();
    SettingsPage {
        loaded: held.loaded,
        primary_language_name: primary.as_deref().map_or_else(
            || super_voice_app_core::i18n::text("automatic", &[]),
            languages::friendly_name,
        ),
        primary_language: primary,
        notification_methods: NOTIFICATION_METHODS
            .iter()
            .map(|method| Choice {
                id: method.to_string(),
                name: notification_method_name(*method),
            })
            .collect(),
        notification_method: state.notification_method.to_string(),
        sleep: state.idle_unload_secs > 0,
        idle_choices: page::idle_unload_choices(state.idle_unload_secs)
            .into_iter()
            .map(|secs| Choice {
                id: secs.to_string(),
                name: idle_time_name(secs),
            })
            .collect(),
        idle_unload_secs: state.idle_unload_secs,
        notice: held.notice.clone(),
    }
}

/// A way failures are reported, under the settings app's key.
fn notification_method_name(method: NotificationMethod) -> String {
    super_voice_app_core::i18n::text(
        match method {
            NotificationMethod::Auto => "auto-recommended",
            NotificationMethod::Desktop => "desktop-notification",
            NotificationMethod::Typed => "type-into-window",
            NotificationMethod::Off => "off",
        },
        &[],
    )
}

/// An idle time as the settings app names it.
fn idle_time_name(after_secs: u64) -> String {
    let (key, arg, n) = match page::idle_time(after_secs) {
        IdleTime::Minutes(n) => ("idle-minutes", "minutes", n),
        IdleTime::Seconds(n) => ("idle-seconds", "seconds", n),
    };
    crate::text_with(key, HashMap::from([(arg.to_string(), n.to_string())]))
}

/// What the page tells the user, in the settings app's words.
fn notice_text(notice: Notice) -> String {
    let (key, error) = match notice {
        Notice::CouldNotSaveNotificationMethod(e) => ("notification-method-could-not-save", e),
        Notice::CouldNotSaveIdleUnload(e) => ("idle-unload-could-not-save", e),
        Notice::CouldNotSaveLanguage(e) => ("language-could-not-save", e),
    };
    crate::text_with(key, HashMap::from([("error".to_string(), error)]))
}

#[cfg(test)]
mod tests {
    use super::{idle_time_name, notice_text};
    use super_voice_app_core::pages::settings::Notice;

    #[test]
    fn idle_times_and_notices_read_in_words() {
        assert_eq!(idle_time_name(300), "5 minutes");
        assert_eq!(
            notice_text(Notice::CouldNotSaveIdleUnload("down".into())),
            "Couldn't save Sleep When Idle: down"
        );
    }
}
