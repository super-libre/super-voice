// SPDX-License-Identifier: GPL-3.0-only
//! A backend's Configure sheet for an app written in another language: a
//! store that holds the sheet's state, runs the core's logic for it
//! (`pages::configure`) on the shared runtime, follows the daemon's events so
//! another client's change shows, and tells the app whenever the sheet
//! changed.
//!
//! Opening the sheet over a backend reads the backend list (each option's
//! value comes with it), which of that backend's secrets are stored, what it
//! uses for a dictation context, and the contexts to offer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super_voice_app_core::daemon::backends::{BackendInfo, BackendOption, BackendSecret};
use super_voice_app_core::daemon::client::v1::contexts::{ContextCatalog, list_contexts};
use super_voice_app_core::events::{self, Event};
use super_voice_app_core::pages::configure::{
    self as page, ConfigureState, ContextPick, Effect, Input, Notice, pick_of, takes_context,
};
use super_voice_app_core::rules::status::unmet_requirements;
use super_voice_shared::models::contexts::DictationContext;
use tokio::sync::broadcast::error::RecvError;

use crate::RUNTIME;

/// What the app is told. Called from the store's own threads.
#[uniffi::export(with_foreign)]
pub trait ConfigureObserver: Send + Sync {
    /// The sheet changed; draw this.
    fn changed(&self, sheet: ConfigureSheet);
}

/// Which dictation context a backend is sent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ContextChoice {
    /// Whichever is active. The default.
    FollowActive,
    /// None at all.
    Nothing,
    /// This one, whatever is active.
    Pinned { id: String },
}

impl From<ContextChoice> for ContextPick {
    fn from(choice: ContextChoice) -> Self {
        match choice {
            ContextChoice::FollowActive => Self::FollowActive,
            ContextChoice::Nothing => Self::Nothing,
            ContextChoice::Pinned { id } => Self::Pinned(id),
        }
    }
}

impl From<ContextPick> for ContextChoice {
    fn from(pick: ContextPick) -> Self {
        match pick {
            ContextPick::FollowActive => Self::FollowActive,
            ContextPick::Nothing => Self::Nothing,
            ContextPick::Pinned(id) => Self::Pinned { id },
        }
    }
}

/// What the user did on the sheet. Each acts on the backend it is open over.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ConfigureAction {
    /// Open the sheet over this backend, and read what it shows.
    Open { source: String },
    /// The sheet closed.
    Close,
    /// A secret's field now holds this.
    SecretTyped { name: String, value: String },
    /// Save was pressed on a secret.
    SaveSecret { name: String },
    /// Remove was pressed on a stored secret.
    RemoveSecret { name: String },
    /// A text option's field now holds this.
    OptionTyped { name: String, value: String },
    /// Save was pressed on a text option. An empty field clears it.
    SaveOption { name: String },
    /// A switch was flipped.
    ToggleOption { name: String, on: bool },
    /// A value was picked from an option's list.
    ChooseOption { name: String, value: String },
    /// Reset was pressed on a text option.
    ResetOption { name: String },
    /// Which context the backend is sent.
    PickContext { choice: ContextChoice },
}

/// The control a row draws.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ConfigureControl {
    /// A secret, which never reads back: Configured and Remove when stored,
    /// else a password field and Save.
    Secret { configured: bool, input: String },
    /// A switch: the flip is the save.
    Bool { on: bool },
    /// A pick from a list: picking is the save. `selected` is `None` when
    /// the stored value is no longer offered.
    Choice {
        choices: Vec<String>,
        selected: Option<String>,
    },
    /// A text field and Save, and Reset while it is set away from its
    /// default. `value` is what the daemon has: the field is filled from it
    /// again each time it changes.
    Text {
        input: String,
        value: Option<String>,
        overridden: bool,
    },
}

/// One secret or option.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConfigureRow {
    /// What the daemon knows it by: what each action names.
    pub name: String,
    pub label: String,
    /// Its description, with its default for a text option.
    pub hint: Option<String>,
    /// It must be set before the backend can be used.
    pub required: bool,
    pub control: ConfigureControl,
}

/// One entry in the context picker.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ContextOption {
    pub choice: ContextChoice,
    /// In the user's language.
    pub name: String,
}

/// Which dictation context the backend is sent, for a backend that takes one.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ContextPicker {
    /// Follow the active context, no context, then each context.
    pub options: Vec<ContextOption>,
    pub selected: ContextChoice,
    /// What the backend is sent now, in words.
    pub hint: String,
}

/// Everything the sheet shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConfigureSheet {
    /// The backend it is open over.
    pub source: Option<String>,
    /// The backend's name, for the title.
    pub name: String,
    /// The backend list has been read since the sheet opened.
    pub loaded: bool,
    /// The context picker, first, for a backend that takes a context.
    pub context: Option<ContextPicker>,
    /// Its secrets, then its options.
    pub rows: Vec<ConfigureRow>,
    /// The labels of what is required and not set yet.
    pub unmet: Vec<String>,
    /// What to tell the user.
    pub notice: Option<String>,
}

#[derive(Default)]
struct Held {
    state: ConfigureState,
    backends: Vec<BackendInfo>,
    contexts: ContextCatalog,
    loaded: bool,
    notice: Option<String>,
}

/// The Configure sheet's store.
#[derive(uniffi::Object)]
pub struct ConfigureStore {
    held: Mutex<Held>,
    observer: Arc<dyn ConfigureObserver>,
}

#[uniffi::export]
impl ConfigureStore {
    /// A store that tells `observer` what changes. Nothing is read until the
    /// app sends [`ConfigureAction::Open`].
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn ConfigureObserver>) -> Arc<Self> {
        let store = Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
        });
        follow(Arc::downgrade(&store));
        store
    }

    /// What the sheet shows now.
    pub fn sheet(&self) -> ConfigureSheet {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: ConfigureAction) {
        let source = match action {
            ConfigureAction::Open { source } => {
                self.open(source);
                return;
            }
            ConfigureAction::Close => {
                self.dispatch(Input::Close);
                return;
            }
            _ => match self.lock().state.open.clone() {
                Some(source) => source,
                None => return,
            },
        };
        let input = match action {
            ConfigureAction::SecretTyped { name, value } => Input::SecretTyped {
                source,
                name,
                value,
            },
            ConfigureAction::SaveSecret { name } => Input::SaveSecret { source, name },
            ConfigureAction::RemoveSecret { name } => Input::RemoveSecret { source, name },
            ConfigureAction::OptionTyped { name, value } => Input::OptionTyped {
                source,
                name,
                value,
            },
            ConfigureAction::SaveOption { name } => Input::SaveOption { source, name },
            ConfigureAction::ToggleOption { name, on } => Input::ToggleOption {
                source,
                name,
                value: on,
            },
            ConfigureAction::ChooseOption { name, value } => Input::ChooseOption {
                source,
                name,
                value,
            },
            ConfigureAction::ResetOption { name } => Input::ResetOption { source, name },
            ConfigureAction::PickContext { choice } => Input::PickContext {
                source,
                pick: choice.into(),
            },
            ConfigureAction::Open { .. } | ConfigureAction::Close => return,
        };
        self.dispatch(input);
    }
}

impl ConfigureStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn changed(&self) {
        let sheet = snapshot(&self.lock());
        self.observer.changed(sheet);
    }

    /// Open the sheet over `source` once the backend list is read: what it
    /// supports decides what the sheet reads.
    fn open(self: Arc<Self>, source: String) {
        {
            let mut held = self.lock();
            held.loaded = false;
            held.notice = None;
        }
        self.changed();
        RUNTIME.spawn(async move {
            self.read_backends().await;
            self.lock().loaded = true;
            Arc::clone(&self).dispatch(Input::Open(source));
            self.dispatch(Input::BackendsLoaded);
        });
    }

    /// Read the backend list. A failed read keeps the list as it was, and
    /// says so.
    async fn read_backends(&self) {
        match super_voice_app_core::daemon::client::list_backends().await {
            Ok(backends) => self.lock().backends = backends,
            Err(e) => {
                self.lock().notice = Some(text_with("backends-could-not-load", e.to_string()));
            }
        }
    }

    /// Read the contexts to offer.
    fn read_contexts(self: Arc<Self>) {
        RUNTIME.spawn(async move {
            // A failed read keeps what the picker had: following the active
            // context and no context are offered either way.
            if let Ok(catalog) = list_contexts().await {
                self.lock().contexts = catalog;
            }
            self.changed();
        });
    }

    /// Hand `input` to the sheet's logic, tell the app what changed, and
    /// start the work it asked for.
    fn dispatch(self: Arc<Self>, input: Input) {
        let effects = {
            let mut held = self.lock();
            let Held {
                state,
                backends,
                notice,
                ..
            } = &mut *held;
            let outcome = page::update(state, backends, input);
            if outcome.clear_notice {
                *notice = None;
            }
            if let Some(said) = outcome.notice {
                *notice = Some(notice_text(said));
            }
            outcome.effects
        };
        self.changed();
        for effect in effects {
            Arc::clone(&self).run(effect);
        }
    }

    fn run(self: Arc<Self>, effect: Effect) {
        let work: std::pin::Pin<Box<dyn Future<Output = Input> + Send>> = match effect {
            // Only the open backend's are shown here.
            Effect::ListSecrets(source) => {
                if self.lock().state.open.as_ref() != Some(&source) {
                    return;
                }
                Box::pin(page::run_list_secrets(source))
            }
            Effect::SaveSecret {
                source,
                name,
                value,
            } => Box::pin(page::run_save_secret(source, name, value)),
            Effect::RemoveSecret { source, name } => {
                Box::pin(page::run_remove_secret(source, name))
            }
            Effect::SetOption {
                source,
                name,
                value,
            } => Box::pin(page::run_set_option(source, name, value)),
            Effect::ClearOption { source, name } => Box::pin(page::run_clear_option(source, name)),
            Effect::ReadContext(source) => Box::pin(page::run_read_context(source)),
            Effect::PickContext { source, pick } => Box::pin(page::run_pick_context(source, pick)),
            Effect::ReloadBackends => {
                RUNTIME.spawn(async move {
                    self.read_backends().await;
                    self.dispatch(Input::BackendsLoaded);
                });
                return;
            }
            Effect::ReloadContexts => {
                self.read_contexts();
                return;
            }
        };
        RUNTIME.spawn(async move {
            let input = work.await;
            self.dispatch(input);
        });
    }
}

/// Follow the daemon's events while the sheet is open, until `store` is
/// dropped: another client's change to a secret, an option or the contexts
/// shows, and everything is read again when the daemon is back.
fn follow(store: Weak<ConfigureStore>) {
    let mut events = crate::events::listen();
    RUNTIME.spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            };
            let Some(store) = store.upgrade() else {
                return;
            };
            let Some(source) = store.lock().state.open.clone() else {
                continue;
            };
            match (&event, events::setting_changed(&event)) {
                (Event::Connected, _) => store.send(ConfigureAction::Open { source }),
                (_, Some("options" | "secrets")) => store.run(Effect::ReloadBackends),
                (_, Some("contexts")) => {
                    Arc::clone(&store).run(Effect::ReloadContexts);
                    store.dispatch(Input::ContextsChanged);
                }
                _ => {}
            }
        }
    });
}

fn snapshot(held: &Held) -> ConfigureSheet {
    let state = &held.state;
    let backend = state
        .open
        .as_ref()
        .and_then(|source| held.backends.iter().find(|b| &b.source == source));
    let Some(backend) = backend else {
        return ConfigureSheet {
            source: state.open.clone(),
            name: state.open.clone().unwrap_or_default(),
            loaded: held.loaded,
            context: None,
            rows: Vec::new(),
            unmet: Vec::new(),
            notice: held.notice.clone(),
        };
    };
    let rows = backend
        .secrets
        .iter()
        .map(|secret| secret_row(state, &backend.source, secret))
        .chain(
            backend
                .options
                .iter()
                .map(|option| option_row(state, &backend.source, option)),
        )
        .collect();
    ConfigureSheet {
        source: Some(backend.source.clone()),
        name: backend.name.clone(),
        loaded: held.loaded,
        context: takes_context(&held.backends, &backend.source)
            .then(|| context_picker(held, &backend.source)),
        rows,
        unmet: unmet_requirements(&state.secrets_configured, backend)
            .into_iter()
            .map(str::to_string)
            .collect(),
        notice: held.notice.clone(),
    }
}

fn secret_row(state: &ConfigureState, source: &str, secret: &BackendSecret) -> ConfigureRow {
    ConfigureRow {
        name: secret.name.clone(),
        label: secret.label.clone().unwrap_or_else(|| secret.name.clone()),
        hint: (!secret.description.is_empty()).then(|| secret.description.clone()),
        required: secret.required,
        control: ConfigureControl::Secret {
            configured: state.secret_configured(source, &secret.name),
            input: state.secret_input(source, &secret.name).to_string(),
        },
    }
}

fn option_row(state: &ConfigureState, source: &str, option: &BackendOption) -> ConfigureRow {
    let description = (!option.description.is_empty()).then(|| option.description.clone());
    let (hint, control) = if option.is_bool() {
        (
            description,
            ConfigureControl::Bool {
                on: option.bool_value(),
            },
        )
    } else if option.has_choices() {
        (
            description,
            ConfigureControl::Choice {
                choices: option.choices.clone(),
                selected: option.choice_index().map(|i| option.choices[i].clone()),
            },
        )
    } else {
        (
            text_hint(option),
            ConfigureControl::Text {
                input: state.option_input(source, &option.name).to_string(),
                value: option.value.clone(),
                overridden: option.value.as_deref() != option.default.as_deref(),
            },
        )
    };
    ConfigureRow {
        name: option.name.clone(),
        label: option.label.clone().unwrap_or_else(|| option.name.clone()),
        hint,
        required: option.required,
        control,
    }
}

/// A text option's description, with its default when it has one.
fn text_hint(option: &BackendOption) -> Option<String> {
    let default = option.default.as_deref().filter(|d| !d.is_empty());
    match (option.description.as_str(), default) {
        ("", None) => None,
        (hint, None) => Some(hint.to_string()),
        ("", Some(default)) => Some(crate::text_with(
            "setting-default",
            HashMap::from([("default".to_string(), default.to_string())]),
        )),
        (hint, Some(default)) => Some(crate::text_with(
            "setting-hint-default",
            HashMap::from([
                ("hint".to_string(), hint.to_string()),
                ("default".to_string(), default.to_string()),
            ]),
        )),
    }
}

/// The picker: follow the active context, no context, then each context; and
/// what the backend is sent now.
fn context_picker(held: &Held, source: &str) -> ContextPicker {
    let contexts = &held.contexts;
    let follow = match contexts.active.as_deref() {
        Some(id) => crate::text_with(
            "follow-active-context",
            HashMap::from([(
                "value0".to_string(),
                contexts
                    .contexts
                    .iter()
                    .find(|c| c.id == id)
                    .map_or_else(|| id.to_string(), DictationContext::display_name),
            )]),
        ),
        None => crate::text("follow-the-active-context-none-right-now"),
    };
    let mut options = vec![
        ContextOption {
            choice: ContextChoice::FollowActive,
            name: follow,
        },
        ContextOption {
            choice: ContextChoice::Nothing,
            name: crate::text("no-context"),
        },
    ];
    options.extend(contexts.contexts.iter().map(|c| ContextOption {
        choice: ContextChoice::Pinned { id: c.id.clone() },
        name: c.display_name(),
    }));
    let ids: Vec<String> = contexts.contexts.iter().map(|c| c.id.clone()).collect();
    let current = held.state.backend_contexts.get(source);
    ContextPicker {
        options,
        selected: pick_of(current, &ids).into(),
        hint: current.and_then(|c| c.context.as_ref()).map_or_else(
            || crate::text("nothing-is-sent-to-this-backend-right"),
            |context| {
                crate::text_with(
                    "sending-context",
                    HashMap::from([("value0".to_string(), context.display_name())]),
                )
            },
        ),
    }
}

/// What the sheet tells the user, in the user's language.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::CouldNotSave(e) => text_with("backend-setting-could-not-save", e),
        Notice::ContextNotChanged(e) => text_with("backend-context-change-failed", e),
    }
}

fn text_with(key: &str, error: String) -> String {
    crate::text_with(key, HashMap::from([("error".to_string(), error)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super_voice_app_core::daemon::client::{BackendContext, ContextMode};

    fn held() -> Held {
        let backend: BackendInfo = serde_json::from_value(serde_json::json!({
            "source": "openai",
            "name": "OpenAI",
            "models": [],
            "secrets": [{ "name": "api_key", "label": "API key", "required": true }],
            "options": [
                { "name": "stream", "type": "bool", "default": "true", "value": "false" },
                { "name": "speed", "choices": ["fast", "slow"], "default": "fast", "value": "slow" },
                { "name": "base_url", "label": "Base URL", "description": "Where to send it.",
                  "default": "https://a", "value": "https://b" },
            ],
            "supports": ["dictation_context"],
        }))
        .expect("a backend");
        let mut held = Held {
            backends: vec![backend],
            contexts: ContextCatalog {
                active: Some("coding".into()),
                contexts: vec![DictationContext {
                    id: "coding".into(),
                    name: "Coding".into(),
                    ..DictationContext::default()
                }],
            },
            loaded: true,
            ..Held::default()
        };
        held.state.open = Some("openai".into());
        held.state
            .option_inputs
            .insert(("openai".into(), "base_url".into()), "https://b".into());
        held
    }

    #[test]
    fn each_row_draws_its_own_control() {
        let sheet = snapshot(&held());
        assert_eq!(sheet.name, "OpenAI");
        assert_eq!(sheet.unmet, ["API key"]);
        let controls: Vec<_> = sheet.rows.iter().map(|r| r.control.clone()).collect();
        assert_eq!(
            controls,
            [
                ConfigureControl::Secret {
                    configured: false,
                    input: String::new()
                },
                ConfigureControl::Bool { on: false },
                ConfigureControl::Choice {
                    choices: vec!["fast".into(), "slow".into()],
                    selected: Some("slow".into())
                },
                ConfigureControl::Text {
                    input: "https://b".into(),
                    value: Some("https://b".into()),
                    overridden: true
                },
            ]
        );
        assert_eq!(
            sheet.rows[3].hint.as_deref(),
            Some("Where to send it. (default: https://a)")
        );
    }

    #[test]
    fn the_picker_names_the_active_context_and_what_is_sent() {
        let mut held = held();
        let picker = snapshot(&held).context.expect("a picker");
        assert_eq!(picker.options.len(), 3);
        assert_eq!(picker.options[0].name, "Follow the active context (Coding)");
        assert_eq!(picker.selected, ContextChoice::FollowActive);
        assert_eq!(picker.hint, "Nothing is sent to this backend right now.");

        held.state.backend_contexts.insert(
            "openai".into(),
            BackendContext {
                mode: ContextMode::Pinned,
                id: Some("coding".into()),
                context: held.contexts.contexts.first().cloned(),
            },
        );
        let picker = snapshot(&held).context.expect("a picker");
        assert_eq!(
            picker.selected,
            ContextChoice::Pinned {
                id: "coding".into()
            }
        );
        assert_eq!(picker.hint, "Sending Coding.");
    }

    #[test]
    fn notices_read_in_words() {
        assert_eq!(
            notice_text(Notice::CouldNotSave("down".into())),
            "Couldn't save that setting: down"
        );
    }
}
