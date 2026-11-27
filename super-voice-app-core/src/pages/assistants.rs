// SPDX-License-Identifier: GPL-3.0-only
//! The Assistants page: asking the daemon where each assistant stands,
//! connecting and disconnecting one, its switches, and the keys made by hand
//! for other MCP clients.
//!
//! The daemon answers every change with the whole list, so the page shows
//! what the configs now say. A switch moves at once, and the daemon's answer,
//! or the list read again after a failure, puts it where the daemon has it.

use chrono::{DateTime, Utc};
use super_voice_shared::models::assistants::{AssistantId, AssistantList, NewKey};

use crate::daemon::client::v1::assistants as client;
use crate::state::assistants::{AssistantsState, ConnectDialog, KeyDialog};

/// One of a connected assistant's switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    /// Show Talk above the prompt.
    ShowTalk,
    /// Send a dictated prompt on its own.
    AutosendDictatedPrompts,
    /// Say so when it needs the user.
    AnnounceWhenNeeded,
    /// Use the conversation as dictation's context.
    ConversationContext,
}

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Read the list again: the page was opened.
    Refresh,
    /// The daemon's list, or why not.
    Loaded(Result<AssistantList, String>),
    /// Connect was pressed on an assistant's row: ask first.
    AskConnect(AssistantId),
    /// The question was called off.
    CancelConnect,
    /// The user said Connect.
    Connect,
    /// The daemon connected it, or why not.
    Connected(AssistantId, Result<AssistantList, String>),
    /// Disconnect was pressed.
    Disconnect(AssistantId),
    /// The daemon disconnected it, or why not.
    Disconnected(AssistantId, Result<AssistantList, String>),
    /// A switch was flipped.
    SetFlag(AssistantId, Flag, bool),
    /// The daemon stored it, or why not.
    FlagSaved(AssistantId, Result<AssistantList, String>),
    /// A connected assistant's page was opened, in the list's place.
    Open(AssistantId),
    /// Back to the list.
    Close,
    /// Create key was pressed: ask for its name.
    AskKey,
    /// The key's name field changed.
    KeyNameChanged(String),
    /// The user said Create.
    CreateKey,
    /// The daemon made the key, or why not.
    KeyCreated(Result<NewKey, String>),
    /// The key dialog was closed.
    CloseKey,
    /// Remove was pressed on a key.
    RemoveKey(String),
    /// The daemon removed it, or why not.
    KeyRemoved(Result<AssistantList, String>),
}

/// What the app should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Read the list: [`run_list`].
    List,
    Connect(AssistantId),
    Disconnect(AssistantId),
    SetFlag(AssistantId, Flag, bool),
    CreateKey(String),
    RemoveKey(String),
    /// Put the cursor in the key's name field.
    FocusKeyName,
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    CouldNotLoad(String),
    CouldNotConnect(AssistantId, String),
    CouldNotDisconnect(AssistantId, String),
    CouldNotChange(AssistantId, String),
    CouldNotCreateKey(String),
    CouldNotRemoveKey(String),
}

impl Notice {
    /// Whether the notice is about what a dialog over the page is doing, and
    /// so is said in it: Connect, or making a key. Anything else, such as the
    /// list read every two seconds failing, is said in a toast and leaves the
    /// dialog as it is.
    #[must_use]
    pub const fn belongs_in_dialog(&self) -> bool {
        matches!(self, Self::CouldNotConnect(..) | Self::CouldNotCreateKey(_))
    }
}

/// What an input came to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub effects: Vec<Effect>,
    /// Take the page's notice down.
    pub clear_notice: bool,
    pub notice: Option<Notice>,
}

impl Outcome {
    fn effect(effect: Effect) -> Self {
        Self {
            effects: vec![effect],
            ..Self::default()
        }
    }

    /// Start `effect` with the notice down.
    fn fresh(effect: Effect) -> Self {
        Self {
            effects: vec![effect],
            clear_notice: true,
            notice: None,
        }
    }

    fn cleared() -> Self {
        Self {
            clear_notice: true,
            ..Self::default()
        }
    }

    /// Say `notice`, and read the list again: what failed may have changed
    /// something.
    fn failed(notice: Notice) -> Self {
        Self {
            effects: vec![Effect::List],
            clear_notice: false,
            notice: Some(notice),
        }
    }
}

/// Take in `input`, and say what to do.
pub fn update(state: &mut AssistantsState, input: Input) -> Outcome {
    match input {
        Input::Refresh => Outcome::effect(Effect::List),
        Input::Loaded(Ok(list)) | Input::FlagSaved(_, Ok(list)) | Input::KeyRemoved(Ok(list)) => {
            if state.list.as_ref() != Some(&list) {
                let connected = list
                    .assistants
                    .iter()
                    .filter(|a| {
                        a.state == super_voice_shared::models::assistants::AssistantState::Connected
                    })
                    .count();
                log::debug!(
                    "assistants: {connected} of {} connected, {} keys made by hand",
                    list.assistants.len(),
                    list.custom_keys.len()
                );
            }
            state.list = Some(list);
            Outcome::default()
        }
        Input::Loaded(Err(e)) => Outcome {
            notice: Some(Notice::CouldNotLoad(e)),
            ..Outcome::default()
        },
        Input::AskConnect(_)
        | Input::CancelConnect
        | Input::Connect
        | Input::Connected(..)
        | Input::Disconnect(_)
        | Input::Disconnected(..) => connection(state, input),
        Input::AskKey
        | Input::KeyNameChanged(_)
        | Input::CreateKey
        | Input::KeyCreated(_)
        | Input::CloseKey
        | Input::RemoveKey(_)
        | Input::KeyRemoved(Err(_)) => keys(state, input),
        Input::SetFlag(id, flag, on) => {
            if let Some(assistant) = state
                .list
                .as_mut()
                .and_then(|list| list.assistants.iter_mut().find(|a| a.id == id))
            {
                *match flag {
                    Flag::ShowTalk => &mut assistant.show_talk,
                    Flag::AutosendDictatedPrompts => &mut assistant.autosend_dictated_prompts,
                    Flag::AnnounceWhenNeeded => &mut assistant.announce_when_needed,
                    Flag::ConversationContext => &mut assistant.conversation_context,
                } = on;
            }
            Outcome::fresh(Effect::SetFlag(id, flag, on))
        }
        Input::FlagSaved(id, Err(e)) => Outcome::failed(Notice::CouldNotChange(id, e)),
        Input::Open(id) => {
            state.open = Some(id);
            Outcome::default()
        }
        Input::Close => {
            state.open = None;
            Outcome::default()
        }
    }
}

/// Connecting and disconnecting.
fn connection(state: &mut AssistantsState, input: Input) -> Outcome {
    match input {
        Input::AskConnect(id) => {
            state.connect = Some(ConnectDialog {
                id,
                connecting: false,
            });
            Outcome::cleared()
        }
        // Not while it connects: the answer has to land in the dialog.
        Input::CancelConnect => {
            if state.connect.is_some_and(|d| !d.connecting) {
                state.connect = None;
                return Outcome::cleared();
            }
            Outcome::default()
        }
        Input::Connect => match state.connect.as_mut().filter(|d| !d.connecting) {
            Some(dialog) => {
                dialog.connecting = true;
                Outcome::fresh(Effect::Connect(dialog.id))
            }
            None => Outcome::default(),
        },
        Input::Connected(id, Ok(list)) => {
            state.list = Some(list);
            state.connect = None;
            state.connected_here.insert(id);
            Outcome::default()
        }
        // The dialog stays open and says why, so the user can try again.
        Input::Connected(id, Err(e)) => {
            if let Some(dialog) = state.connect.as_mut() {
                dialog.connecting = false;
            }
            Outcome::failed(Notice::CouldNotConnect(id, e))
        }
        // Once at a time per assistant: its button waits.
        Input::Disconnect(id) => {
            if state.disconnecting.insert(id) {
                Outcome::fresh(Effect::Disconnect(id))
            } else {
                Outcome::default()
            }
        }
        Input::Disconnected(id, result) => {
            state.disconnecting.remove(&id);
            state.connected_here.remove(&id);
            match result {
                Ok(list) => {
                    state.list = Some(list);
                    // Its page has nothing left to show: back to the list.
                    if state.open == Some(id) {
                        state.open = None;
                    }
                    Outcome::default()
                }
                Err(e) => Outcome::failed(Notice::CouldNotDisconnect(id, e)),
            }
        }
        _ => Outcome::default(),
    }
}

/// The keys made by hand.
fn keys(state: &mut AssistantsState, input: Input) -> Outcome {
    match input {
        Input::AskKey => {
            state.key = Some(KeyDialog::new());
            Outcome::fresh(Effect::FocusKeyName)
        }
        Input::KeyNameChanged(name) => {
            if let Some(KeyDialog::Naming {
                name: field,
                creating: false,
            }) = state.key.as_mut()
            {
                *field = name;
            }
            Outcome::default()
        }
        Input::CreateKey => {
            let Some(name) = state
                .key
                .as_ref()
                .and_then(KeyDialog::name_to_create)
                .map(str::to_string)
            else {
                return Outcome::default();
            };
            if let Some(KeyDialog::Naming { creating, .. }) = state.key.as_mut() {
                *creating = true;
            }
            Outcome::fresh(Effect::CreateKey(name))
        }
        // The key, shown in the dialog this once, with the list read again
        // for its row; or why not, with the name kept.
        Input::KeyCreated(Ok(key)) => {
            state.key = Some(KeyDialog::Made(key));
            Outcome::effect(Effect::List)
        }
        Input::KeyCreated(Err(e)) => {
            if let Some(KeyDialog::Naming { creating, .. }) = state.key.as_mut() {
                *creating = false;
            }
            Outcome {
                notice: Some(Notice::CouldNotCreateKey(e)),
                ..Outcome::default()
            }
        }
        // Not while the key is being made: its answer has to land in the
        // dialog, or the key is lost.
        Input::CloseKey => {
            if matches!(state.key, Some(KeyDialog::Naming { creating: true, .. })) {
                return Outcome::default();
            }
            state.key = None;
            Outcome::cleared()
        }
        Input::RemoveKey(id) => Outcome::fresh(Effect::RemoveKey(id)),
        Input::KeyRemoved(Err(e)) => Outcome::failed(Notice::CouldNotRemoveKey(e)),
        _ => Outcome::default(),
    }
}

/// The switches an assistant has, in the order its page lists them: Talk is
/// only Claude Code's and `OpenCode`'s plugins'; only Claude Code's can send its
/// prompt box; the Antigravity CLI has no hook for when it needs the user.
#[must_use]
pub fn flags(id: AssistantId) -> Vec<Flag> {
    let mut flags = Vec::new();
    if matches!(id, AssistantId::ClaudeCode | AssistantId::OpenCode) {
        flags.push(Flag::ShowTalk);
    }
    if id == AssistantId::ClaudeCode {
        flags.push(Flag::AutosendDictatedPrompts);
    }
    if id != AssistantId::Antigravity {
        flags.push(Flag::AnnounceWhenNeeded);
    }
    flags.push(Flag::ConversationContext);
    flags
}

/// Whether an assistant reads every reply aloud, or only replies to what was
/// dictated to it.
#[must_use]
pub const fn reads_every_reply(id: AssistantId) -> bool {
    matches!(id, AssistantId::ClaudeCode | AssistantId::OpenCode)
}

/// Whether connecting installs a plugin, rather than writing Codex's config.
#[must_use]
pub const fn connects_with_a_plugin(id: AssistantId) -> bool {
    !matches!(id, AssistantId::Codex)
}

/// Whether `flag` is on for `assistant`.
#[must_use]
pub const fn flag_on(
    assistant: &super_voice_shared::models::assistants::Assistant,
    flag: Flag,
) -> bool {
    match flag {
        Flag::ShowTalk => assistant.show_talk,
        Flag::AutosendDictatedPrompts => assistant.autosend_dictated_prompts,
        Flag::AnnounceWhenNeeded => assistant.announce_when_needed,
        Flag::ConversationContext => assistant.conversation_context,
    }
}

/// The last part of a session's folder, which names it.
#[must_use]
pub fn folder_name(folder: &str) -> String {
    std::path::Path::new(folder).file_name().map_or_else(
        || folder.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// A folder as a terminal shows it: under the home folder, from `~`.
#[must_use]
pub fn shown_folder(folder: &str, home: Option<&std::path::Path>) -> String {
    home.and_then(|home| std::path::Path::new(folder).strip_prefix(home).ok())
        .map_or_else(
            || folder.to_string(),
            |rest| {
                if rest.as_os_str().is_empty() {
                    "~".to_string()
                } else {
                    format!("~/{}", rest.display())
                }
            },
        )
}

/// How long ago a session last spoke or listened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ago {
    /// Within the minute.
    Now,
    Minutes(i64),
    Hours(i64),
}

/// When a session last spoke or listened, as of `now`. Nothing before it
/// has, or for what is not a time. A clock a little ahead of the app's is
/// still now.
#[must_use]
pub fn used_ago(last_used_at: Option<&str>, now: DateTime<Utc>) -> Option<Ago> {
    let at = DateTime::parse_from_rfc3339(last_used_at?).ok()?;
    let minutes = (now - at.with_timezone(&Utc)).num_minutes().max(0);
    Some(match minutes {
        0 => Ago::Now,
        1..=59 => Ago::Minutes(minutes),
        _ => Ago::Hours(minutes / 60),
    })
}

/// [`used_ago`], as of now.
#[must_use]
pub fn used_ago_now(last_used_at: Option<&str>) -> Option<Ago> {
    used_ago(last_used_at, Utc::now())
}

/// When a key was made, as the page says it: today or yesterday at a time,
/// or the date and time for anything older, in the viewer's time zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MadeOn {
    /// Today, at this time (`09:42`).
    Today(String),
    /// Yesterday, at this time.
    Yesterday(String),
    /// Before then: `2026-11-16 11:03`.
    Earlier(String),
    /// Not a time the page can read: the daemon's words, as they came.
    Unread(String),
}

/// When a key made at `created_at` (RFC 3339) was made, as of `now`.
#[must_use]
pub fn made_on<Tz: chrono::TimeZone>(created_at: &str, now: &DateTime<Tz>) -> MadeOn {
    let Ok(at) = DateTime::parse_from_rfc3339(created_at) else {
        return MadeOn::Unread(created_at.to_string());
    };
    let at = at.with_timezone(&now.timezone()).naive_local();
    let today = now.naive_local().date();
    let time = at.format("%H:%M").to_string();
    if at.date() == today {
        MadeOn::Today(time)
    } else if today.pred_opt() == Some(at.date()) {
        MadeOn::Yesterday(time)
    } else {
        MadeOn::Earlier(at.format("%Y-%m-%d %H:%M").to_string())
    }
}

/// [`made_on`], as of now in the viewer's time zone.
#[must_use]
pub fn made_on_now(created_at: &str) -> MadeOn {
    made_on(created_at, &chrono::Local::now())
}

///
/// Said at trace, as the page reads it every 2 s while shown; a list that
/// changed is said at debug when it is taken in.
pub async fn run_list() -> Input {
    Input::Loaded(
        crate::daemon::client::quietly(client::list_assistants())
            .await
            .map_err(|e| e.to_string()),
    )
}

pub async fn run_connect(id: AssistantId) -> Input {
    Input::Connected(
        id,
        client::connect_assistant(id)
            .await
            .map_err(|e| e.to_string()),
    )
}

pub async fn run_disconnect(id: AssistantId) -> Input {
    Input::Disconnected(
        id,
        client::disconnect_assistant(id)
            .await
            .map_err(|e| e.to_string()),
    )
}

pub async fn run_set_flag(id: AssistantId, flag: Flag, on: bool) -> Input {
    let result = match flag {
        Flag::ShowTalk => client::set_show_talk(id, on).await,
        Flag::AutosendDictatedPrompts => client::set_autosend_dictated_prompts(id, on).await,
        Flag::AnnounceWhenNeeded => client::set_announce_when_needed(id, on).await,
        Flag::ConversationContext => client::set_conversation_context(id, on).await,
    };
    Input::FlagSaved(id, result.map_err(|e| e.to_string()))
}

pub async fn run_create_key(name: String) -> Input {
    Input::KeyCreated(client::create_key(name).await.map_err(|e| e.to_string()))
}

pub async fn run_remove_key(id: String) -> Input {
    Input::KeyRemoved(client::remove_key(id).await.map_err(|e| e.to_string()))
}

#[cfg(test)]
mod tests;
