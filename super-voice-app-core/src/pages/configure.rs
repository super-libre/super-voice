// SPDX-License-Identifier: GPL-3.0-only
//! A backend's Configure sheet: its secrets (an API key), its options (a base
//! URL, a switch, a pick from a list), and, for a backend that takes one,
//! which dictation context it is sent.
//!
//! Secrets never read back: the daemon only says whether each is stored. An
//! option's value comes with the backend list, so every write is followed by
//! reading that list again ([`Effect::ReloadBackends`]), and the row shows
//! what the daemon stored. Which context a backend uses is read when the sheet
//! opens, and again when the contexts change.
//!
//! The backend list is the app's: [`update`] takes it where it needs an
//! option's default or what a backend supports.

use std::collections::HashMap;

use super_voice_registry_types::index::supports;

use crate::daemon::backends::{BackendInfo, BackendOption};
use crate::daemon::client::BackendContext;
use crate::daemon::client::v1::backends::options::{clear_backend_option, set_backend_option};
use crate::daemon::client::v1::backends::secrets::{
    clear_backend_secret, list_backend_secrets, set_backend_secret,
};
use crate::daemon::client::v1::contexts::backend::{
    clear_backend_context, get_backend_context, set_backend_context,
};

/// A secret or option, by `(source, name)`.
pub type Key = (String, String);

/// What the sheet holds beside the backend list.
#[derive(Debug, Clone, Default)]
pub struct ConfigureState {
    /// The backend whose sheet is open, by `source`.
    pub open: Option<String>,
    /// What is typed in each secret's field. Never read back from the daemon.
    pub secret_inputs: HashMap<Key, String>,
    /// Whether each declared secret is stored, as the daemon reported it.
    /// Read for every backend, since a required one missing keeps it from
    /// being used.
    pub secrets_configured: HashMap<Key, bool>,
    /// What is typed in each text option's field, seeded from its value each
    /// time the backend list is read.
    pub option_inputs: HashMap<Key, String>,
    /// What each backend uses for a dictation context, as the daemon last
    /// said, by `source`.
    ///
    /// Read when a sheet opens rather than with the backend list: it is one
    /// line of one sheet, and the list is read on every visit to two pages
    /// that never show it.
    pub backend_contexts: HashMap<String, BackendContext>,
}

impl ConfigureState {
    /// Whether `source`'s secret `name` is stored.
    #[must_use]
    pub fn secret_configured(&self, source: &str, name: &str) -> bool {
        self.secrets_configured
            .get(&key(source, name))
            .copied()
            .unwrap_or(false)
    }

    /// What is typed in `source`'s secret `name`.
    #[must_use]
    pub fn secret_input(&self, source: &str, name: &str) -> &str {
        self.secret_inputs
            .get(&key(source, name))
            .map_or("", String::as_str)
    }

    /// What is typed in `source`'s option `name`.
    #[must_use]
    pub fn option_input(&self, source: &str, name: &str) -> &str {
        self.option_inputs
            .get(&key(source, name))
            .map_or("", String::as_str)
    }
}

/// Which dictation context a backend is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextPick {
    /// Whichever is active. The default.
    FollowActive,
    /// None at all.
    Nothing,
    /// This one, whatever is active.
    Pinned(String),
}

/// What happened.
#[derive(Debug, Clone)]
pub enum Input {
    /// The sheet opened over this backend.
    Open(String),
    /// The sheet closed.
    Close,
    /// The backend list was read again: seed the option fields from it, and
    /// read which secrets each backend has stored.
    BackendsLoaded,
    /// Which of a backend's secrets are stored, as `(name, configured)`, or
    /// why that couldn't be read.
    SecretsListed {
        source: String,
        result: Result<Vec<(String, bool)>, String>,
    },
    /// A secret's field now holds this.
    SecretTyped {
        source: String,
        name: String,
        value: String,
    },
    /// Save was pressed on a secret.
    SaveSecret { source: String, name: String },
    /// The daemon stored the secret, or why not.
    SecretSaved {
        source: String,
        name: String,
        result: Result<(), String>,
    },
    /// Remove was pressed on a stored secret.
    RemoveSecret { source: String, name: String },
    /// A text option's field now holds this.
    OptionTyped {
        source: String,
        name: String,
        value: String,
    },
    /// Save was pressed on a text option. An empty field clears it.
    SaveOption { source: String, name: String },
    /// A switch was flipped: the flip is the save.
    ToggleOption {
        source: String,
        name: String,
        value: bool,
    },
    /// A value was picked from an option's list: picking is the save.
    ChooseOption {
        source: String,
        name: String,
        value: String,
    },
    /// Reset was pressed on a text option: back to the backend's default.
    ResetOption { source: String, name: String },
    /// A secret was removed or an option written, or why not.
    Written(Result<(), String>),
    /// Pick which context a backend is sent.
    PickContext { source: String, pick: ContextPick },
    /// What a backend uses, as read when the sheet opened, or why not.
    ContextRead {
        source: String,
        result: Result<BackendContext, String>,
    },
    /// What a backend uses after a pick, or why the pick failed.
    ContextPicked {
        source: String,
        result: Result<BackendContext, String>,
    },
    /// The contexts changed, here or in another app: what the open sheet's
    /// backend resolves to may have too.
    ContextsChanged,
}

/// Work [`update`] asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Read which of a backend's secrets are stored ([`run_list_secrets`]).
    ListSecrets(String),
    /// Store a secret ([`run_save_secret`]).
    SaveSecret {
        source: String,
        name: String,
        value: String,
    },
    /// Remove a secret ([`run_remove_secret`]).
    RemoveSecret { source: String, name: String },
    /// Set an option ([`run_set_option`]).
    SetOption {
        source: String,
        name: String,
        value: String,
    },
    /// Clear an option back to its default ([`run_clear_option`]).
    ClearOption { source: String, name: String },
    /// Read what a backend uses for a context ([`run_read_context`]).
    ReadContext(String),
    /// Pick a backend's context ([`run_pick_context`]).
    PickContext { source: String, pick: ContextPick },
    /// Read the backend list again. The app's to do: it keeps the list.
    ReloadBackends,
    /// Read the contexts list, for the picker. The app's to do: the Contexts
    /// page keeps it.
    ReloadContexts,
}

/// Something to tell the user on the sheet. Each app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A secret or option couldn't be written: the daemon's words.
    CouldNotSave(String),
    /// The backend's context couldn't be changed.
    ContextNotChanged(String),
}

/// What an [`update`] asks of the app.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The work to start, in order.
    pub effects: Vec<Effect>,
    /// Take the sheet's notice down. Applied before [`notice`](Self::notice).
    pub clear_notice: bool,
    pub notice: Option<Notice>,
}

impl Outcome {
    fn work(effects: Vec<Effect>) -> Self {
        Self {
            effects,
            ..Self::default()
        }
    }

    /// A write the user started: the sheet's old notice goes.
    fn write(effect: Effect) -> Self {
        Self {
            effects: vec![effect],
            clear_notice: true,
            notice: None,
        }
    }

    fn notice(notice: Notice) -> Self {
        Self {
            notice: Some(notice),
            ..Self::default()
        }
    }
}

/// Take in `input`, and say what to do. `backends` is the backend list as the
/// app last read it.
pub fn update(state: &mut ConfigureState, backends: &[BackendInfo], input: Input) -> Outcome {
    match input {
        Input::Open(source) => open(state, backends, source),
        Input::Close => {
            state.open = None;
            Outcome {
                clear_notice: true,
                ..Outcome::default()
            }
        }
        Input::BackendsLoaded => backends_loaded(state, backends),
        Input::SecretsListed { source, result } => {
            match result {
                Ok(items) => {
                    for (name, configured) in items {
                        state
                            .secrets_configured
                            .insert((source.clone(), name), configured);
                    }
                }
                // Read as nothing stored, as it was before the read.
                Err(e) => log::warn!("Couldn't read {source}'s secrets: {e}"),
            }
            Outcome::default()
        }
        Input::SecretTyped {
            source,
            name,
            value,
        } => {
            state.secret_inputs.insert((source, name), value);
            Outcome::default()
        }
        Input::OptionTyped {
            source,
            name,
            value,
        } => {
            state.option_inputs.insert((source, name), value);
            Outcome::default()
        }
        Input::PickContext { source, pick } => Outcome::write(Effect::PickContext { source, pick }),
        Input::ContextRead { source, result } => {
            match result {
                Ok(context) => {
                    state.backend_contexts.insert(source, context);
                }
                // Reads as following the active context, which is the default
                // and the truth for almost every backend. Not worth a notice
                // in a sheet that is about secrets and options.
                Err(e) => log::warn!("Couldn't read this backend's context: {e}"),
            }
            Outcome::default()
        }
        Input::ContextPicked { source, result } => match result {
            Ok(context) => {
                state.backend_contexts.insert(source, context);
                Outcome::default()
            }
            Err(e) => Outcome::notice(Notice::ContextNotChanged(e)),
        },
        // The open sheet shows what its backend is pointed at. A backend
        // pinned or unpinned elsewhere would otherwise leave it saying the
        // opposite until it was opened again.
        Input::ContextsChanged => match &state.open {
            Some(source) if state.backend_contexts.contains_key(source) => {
                Outcome::work(vec![Effect::ReadContext(source.clone())])
            }
            _ => Outcome::default(),
        },
        writes => write(state, backends, writes),
    }
}

/// The sheet opens over `source`, without the last sheet's notice.
fn open(state: &mut ConfigureState, backends: &[BackendInfo], source: String) -> Outcome {
    // The picker needs two things the backend list doesn't carry: what this
    // backend is pointed at, and the contexts to offer.
    let mut effects = vec![Effect::ReloadContexts];
    if takes_context(backends, &source) {
        effects.push(Effect::ReadContext(source.clone()));
    }
    state.open = Some(source);
    Outcome {
        effects,
        clear_notice: true,
        notice: None,
    }
}

/// The backend list was read again.
fn backends_loaded(state: &mut ConfigureState, backends: &[BackendInfo]) -> Outcome {
    state.option_inputs.clear();
    state.secrets_configured.clear();
    for backend in backends {
        for option in &backend.options {
            state.option_inputs.insert(
                key(&backend.source, &option.name),
                option.value.clone().unwrap_or_default(),
            );
        }
    }
    Outcome::work(
        backends
            .iter()
            .map(|b| Effect::ListSecrets(b.source.clone()))
            .collect(),
    )
}

/// A secret or option the user saved, removed, flipped, picked or reset; and
/// the daemon's answer.
fn write(state: &mut ConfigureState, backends: &[BackendInfo], input: Input) -> Outcome {
    match input {
        // The field keeps what was typed until the daemon has stored it, so
        // a failure leaves it there to try again.
        Input::SaveSecret { source, name } => {
            let value = state.secret_inputs.get(&key(&source, &name)).cloned();
            match value.filter(|v| !v.is_empty()) {
                Some(value) => Outcome::write(Effect::SaveSecret {
                    source,
                    name,
                    value,
                }),
                None => Outcome {
                    clear_notice: true,
                    ..Outcome::default()
                },
            }
        }
        Input::SecretSaved {
            source,
            name,
            result,
        } => match result {
            Ok(()) => {
                state.secret_inputs.remove(&(source, name));
                Outcome::work(vec![Effect::ReloadBackends])
            }
            Err(e) => Outcome::notice(Notice::CouldNotSave(e)),
        },
        Input::RemoveSecret { source, name } => {
            state.secret_inputs.remove(&key(&source, &name));
            Outcome::write(Effect::RemoveSecret { source, name })
        }
        // The daemon hands an option to the running stages itself: it is a
        // request header, so nothing reloads and the change is live on the
        // next request.
        Input::SaveOption { source, name } => {
            let value = state
                .option_inputs
                .get(&key(&source, &name))
                .cloned()
                .unwrap_or_default();
            if value.is_empty() {
                Outcome::write(Effect::ClearOption { source, name })
            } else {
                Outcome::write(Effect::SetOption {
                    source,
                    name,
                    value,
                })
            }
        }
        // Flipping back to the default clears the override rather than
        // storing a copy of it, so the daemon's config records only real
        // changes, and the backend's own default can move later.
        Input::ToggleOption {
            source,
            name,
            value,
        } => {
            let default = option(backends, &source, &name).and_then(BackendOption::bool_default);
            if default == Some(value) {
                Outcome::write(Effect::ClearOption { source, name })
            } else {
                Outcome::write(Effect::SetOption {
                    source,
                    name,
                    value: value.to_string(),
                })
            }
        }
        // The list's twin of the switch: picking the default clears.
        Input::ChooseOption {
            source,
            name,
            value,
        } => {
            let default = option(backends, &source, &name).and_then(|o| o.default.as_ref());
            if default == Some(&value) {
                Outcome::write(Effect::ClearOption { source, name })
            } else {
                Outcome::write(Effect::SetOption {
                    source,
                    name,
                    value,
                })
            }
        }
        Input::ResetOption { source, name } => Outcome::write(Effect::ClearOption { source, name }),
        Input::Written(Ok(())) => Outcome::work(vec![Effect::ReloadBackends]),
        Input::Written(Err(e)) => Outcome::notice(Notice::CouldNotSave(e)),
        other => {
            log::error!("configure: {other:?} isn't a write");
            Outcome::default()
        }
    }
}

fn key(source: &str, name: &str) -> Key {
    (source.to_string(), name.to_string())
}

/// `source`'s option `name`, from the backend list.
fn option<'a>(backends: &'a [BackendInfo], source: &str, name: &str) -> Option<&'a BackendOption> {
    backends
        .iter()
        .find(|b| b.source == source)
        .and_then(|b| b.options.iter().find(|o| o.name == name))
}

/// Whether `source` takes a dictation context, so its sheet offers to pick
/// one. On any other, the row would offer a choice with no effect.
#[must_use]
pub fn takes_context(backends: &[BackendInfo], source: &str) -> bool {
    backends
        .iter()
        .any(|b| b.source == source && supports::has(&b.supports, supports::DICTATION_CONTEXT))
}

/// What `context` reads as on the picker. A pin to a context since deleted
/// reads as no context: the daemon keeps the pin, and the backend is sent
/// nothing, so showing "follow" would be wrong about what it receives.
#[must_use]
pub fn pick_of(context: Option<&BackendContext>, ids: &[String]) -> ContextPick {
    use crate::daemon::client::ContextMode;
    match context.map(|c| (c.mode, c.id.as_ref())) {
        None | Some((ContextMode::Active, _)) => ContextPick::FollowActive,
        Some((ContextMode::Pinned, Some(id))) if ids.contains(id) => {
            ContextPick::Pinned(id.clone())
        }
        Some((ContextMode::Nothing | ContextMode::Pinned, _)) => ContextPick::Nothing,
    }
}

/// Read which of `source`'s secrets are stored.
pub async fn run_list_secrets(source: String) -> Input {
    let result = list_backend_secrets(source.clone())
        .await
        .map_err(|e| e.to_string());
    Input::SecretsListed { source, result }
}

/// Store `value` as `source`'s secret `name`.
pub async fn run_save_secret(source: String, name: String, value: String) -> Input {
    let result = set_backend_secret(source.clone(), name.clone(), value)
        .await
        .map_err(|e| e.to_string());
    Input::SecretSaved {
        source,
        name,
        result,
    }
}

/// Remove `source`'s secret `name`.
pub async fn run_remove_secret(source: String, name: String) -> Input {
    Input::Written(
        clear_backend_secret(source, name)
            .await
            .map_err(|e| e.to_string()),
    )
}

/// Set `source`'s option `name` to `value`.
pub async fn run_set_option(source: String, name: String, value: String) -> Input {
    Input::Written(
        set_backend_option(source, name, value)
            .await
            .map_err(|e| e.to_string()),
    )
}

/// Clear `source`'s option `name` back to its default.
pub async fn run_clear_option(source: String, name: String) -> Input {
    Input::Written(
        clear_backend_option(source, name)
            .await
            .map_err(|e| e.to_string()),
    )
}

/// Read what `source` uses for a context.
pub async fn run_read_context(source: String) -> Input {
    let result = get_backend_context(source.clone())
        .await
        .map_err(|e| e.to_string());
    Input::ContextRead { source, result }
}

/// Send `source` the context `pick` names.
pub async fn run_pick_context(source: String, pick: ContextPick) -> Input {
    let result = match pick {
        ContextPick::FollowActive => clear_backend_context(source.clone()).await,
        ContextPick::Nothing => set_backend_context(source.clone(), None).await,
        ContextPick::Pinned(id) => set_backend_context(source.clone(), Some(id)).await,
    }
    .map_err(|e| e.to_string());
    Input::ContextPicked { source, result }
}

#[cfg(test)]
mod tests;
