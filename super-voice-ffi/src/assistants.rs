// SPDX-License-Identifier: GPL-3.0-only
//! The Assistants page for an app written in another language: a store that
//! holds the page's state, runs the core's logic for it (`pages::assistants`)
//! on the shared runtime, and tells the app whenever the page changed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use chrono::{DateTime, Utc};
use super_voice_app_core::events::Event;
use super_voice_app_core::pages::assistants::{
    self as page, Ago, Effect, Flag, Input, Notice, flag_on, flags, folder_name, shown_folder,
};
use super_voice_app_core::state::assistants::{AssistantsState, KeyDialog};
use super_voice_shared::models::assistants::{AssistantId, AssistantSession, AssistantState};
use tokio::sync::broadcast::error::RecvError;

use crate::RUNTIME;

/// What the app is told. Called from the store's own threads.
#[uniffi::export(with_foreign)]
pub trait AssistantsObserver: Send + Sync {
    /// The page changed; draw this.
    fn changed(&self, page: AssistantsPage);
}

/// One of a connected assistant's switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AssistantFlag {
    ShowTalk,
    AutosendDictatedPrompts,
    AnnounceWhenNeeded,
    ConversationContext,
}

impl From<Flag> for AssistantFlag {
    fn from(flag: Flag) -> Self {
        match flag {
            Flag::ShowTalk => Self::ShowTalk,
            Flag::AutosendDictatedPrompts => Self::AutosendDictatedPrompts,
            Flag::AnnounceWhenNeeded => Self::AnnounceWhenNeeded,
            Flag::ConversationContext => Self::ConversationContext,
        }
    }
}

impl From<AssistantFlag> for Flag {
    fn from(flag: AssistantFlag) -> Self {
        match flag {
            AssistantFlag::ShowTalk => Self::ShowTalk,
            AssistantFlag::AutosendDictatedPrompts => Self::AutosendDictatedPrompts,
            AssistantFlag::AnnounceWhenNeeded => Self::AnnounceWhenNeeded,
            AssistantFlag::ConversationContext => Self::ConversationContext,
        }
    }
}

/// What the user did. An assistant goes by its wire id (`claude-code`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum AssistantsAction {
    Refresh,
    AskConnect {
        id: String,
    },
    CancelConnect,
    Connect,
    Disconnect {
        id: String,
    },
    SetFlag {
        id: String,
        flag: AssistantFlag,
        on: bool,
    },
    Open {
        id: String,
    },
    Close,
    AskKey,
    KeyNameChanged {
        name: String,
    },
    CreateKey,
    CloseKey,
    RemoveKey {
        id: String,
    },
}

/// Where an assistant stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AssistantStanding {
    /// Not on this computer.
    NotFound,
    /// On this computer, not connected.
    Found,
    Connected,
}

/// How long ago a session last spoke or listened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SessionAgo {
    Now,
    Minutes { n: i64 },
    Hours { n: i64 },
}

/// One open session.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SessionRow {
    /// Its folder's last part; `None` when it has no folder.
    pub name: Option<String>,
    /// The folder, from `~` under the home folder.
    pub folder: Option<String>,
    pub agents: u32,
    pub speaking: bool,
    pub listening: bool,
    pub ago: Option<SessionAgo>,
}

/// One switch, and whether it is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct FlagRow {
    pub flag: AssistantFlag,
    pub on: bool,
}

/// One assistant.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AssistantRow {
    pub id: String,
    pub name: String,
    pub standing: AssistantStanding,
    /// Whether it reads every reply aloud, or only replies to what was
    /// dictated to it.
    pub reads_every_reply: bool,
    /// Connected since the app started: it reads its config when it starts,
    /// so it needs a restart.
    pub connected_here: bool,
    /// Its Disconnect is running.
    pub disconnecting: bool,
    pub flags: Vec<FlagRow>,
    pub sessions: Vec<SessionRow>,
}

/// One key made by hand.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct KeyRow {
    pub id: String,
    pub name: String,
    /// When it was made, as seconds since 1970; `None` when the daemon's
    /// words are not a time, which [`Self::made`] then holds.
    pub created: Option<i64>,
    pub made: String,
    pub sessions: Vec<SessionRow>,
}

/// The question Connect asks first.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConnectQuestion {
    pub id: String,
    pub name: String,
    /// Connecting installs a plugin, rather than writing Codex's config.
    pub with_plugin: bool,
    /// The user said Connect and the daemon has not answered yet.
    pub connecting: bool,
}

/// The dialog that makes a key.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum KeyQuestion {
    Naming {
        name: String,
        creating: bool,
        /// There is a name to send.
        can_create: bool,
    },
    /// The key, shown this once.
    Made {
        name: String,
        key: String,
        address: String,
    },
}

/// Everything the page shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AssistantsPage {
    pub loaded: bool,
    pub assistants: Vec<AssistantRow>,
    pub keys: Vec<KeyRow>,
    /// The connected assistant whose page is open, in the list's place.
    pub open: Option<String>,
    pub connect: Option<ConnectQuestion>,
    pub key: Option<KeyQuestion>,
    /// What to tell the user, in the user's language.
    pub notice: Option<String>,
}

#[derive(Default)]
struct Held {
    state: AssistantsState,
    notice: Option<String>,
}

/// The Assistants page's store.
#[derive(uniffi::Object)]
pub struct AssistantsStore {
    held: Mutex<Held>,
    observer: Arc<dyn AssistantsObserver>,
}

#[uniffi::export]
impl AssistantsStore {
    /// A store that tells `observer` what changes. Nothing is read until the
    /// app sends [`AssistantsAction::Refresh`].
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn AssistantsObserver>) -> Arc<Self> {
        let store = Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
        });
        follow(Arc::downgrade(&store));
        store
    }

    /// What the page shows now.
    pub fn page(&self) -> AssistantsPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: AssistantsAction) {
        let id = |id: &str| id.parse::<AssistantId>().ok();
        let input = match action {
            AssistantsAction::Refresh => Input::Refresh,
            AssistantsAction::AskConnect { id: i } => match id(&i) {
                Some(id) => Input::AskConnect(id),
                None => return,
            },
            AssistantsAction::CancelConnect => Input::CancelConnect,
            AssistantsAction::Connect => Input::Connect,
            AssistantsAction::Disconnect { id: i } => match id(&i) {
                Some(id) => Input::Disconnect(id),
                None => return,
            },
            AssistantsAction::SetFlag { id: i, flag, on } => match id(&i) {
                Some(id) => Input::SetFlag(id, flag.into(), on),
                None => return,
            },
            AssistantsAction::Open { id: i } => match id(&i) {
                Some(id) => Input::Open(id),
                None => return,
            },
            AssistantsAction::Close => Input::Close,
            AssistantsAction::AskKey => Input::AskKey,
            AssistantsAction::KeyNameChanged { name } => Input::KeyNameChanged(name),
            AssistantsAction::CreateKey => Input::CreateKey,
            AssistantsAction::CloseKey => Input::CloseKey,
            AssistantsAction::RemoveKey { id } => Input::RemoveKey(id),
        };
        self.dispatch(input);
    }
}

impl AssistantsStore {
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
            Arc::clone(&self).run(effect);
        }
    }

    fn run(self: Arc<Self>, effect: Effect) {
        RUNTIME.spawn(async move {
            let input = match effect {
                Effect::List => page::run_list().await,
                Effect::Connect(id) => page::run_connect(id).await,
                Effect::Disconnect(id) => page::run_disconnect(id).await,
                Effect::SetFlag(id, flag, on) => page::run_set_flag(id, flag, on).await,
                Effect::CreateKey(name) => page::run_create_key(name).await,
                Effect::RemoveKey(id) => page::run_remove_key(id).await,
                // The app puts the cursor in the field as the dialog opens.
                Effect::FocusKeyName => return,
            };
            self.dispatch(input);
        });
    }
}

/// Read the list again when the daemon is back, until `store` is dropped.
fn follow(store: Weak<AssistantsStore>) {
    let mut events = crate::events::listen();
    RUNTIME.spawn(async move {
        loop {
            match events.recv().await {
                Ok(Event::Connected) => {}
                Ok(_) | Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            }
            let Some(store) = store.upgrade() else {
                return;
            };
            if store.lock().state.list.is_some() {
                store.dispatch(Input::Refresh);
            }
        }
    });
}

fn snapshot(held: &Held) -> AssistantsPage {
    let state = &held.state;
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let now = Utc::now();
    let sessions = |sessions: &[AssistantSession]| {
        sessions
            .iter()
            .map(|s| session_row(s, home.as_deref(), now))
            .collect()
    };
    let list = state.list.as_ref();
    AssistantsPage {
        loaded: list.is_some(),
        assistants: AssistantId::ALL
            .iter()
            .filter_map(|id| list?.assistants.iter().find(|a| a.id == *id))
            .map(|assistant| AssistantRow {
                id: assistant.id.to_string(),
                name: assistant.id.display_name().to_string(),
                standing: match assistant.state {
                    AssistantState::NotFound => AssistantStanding::NotFound,
                    AssistantState::Found => AssistantStanding::Found,
                    AssistantState::Connected => AssistantStanding::Connected,
                },
                reads_every_reply: page::reads_every_reply(assistant.id),
                connected_here: state.connected_here.contains(&assistant.id),
                disconnecting: state.disconnecting.contains(&assistant.id),
                flags: flags(assistant.id)
                    .into_iter()
                    .map(|flag| FlagRow {
                        flag: flag.into(),
                        on: flag_on(assistant, flag),
                    })
                    .collect(),
                sessions: sessions(&assistant.sessions),
            })
            .collect(),
        keys: list.map_or_default(|list| {
            list.custom_keys
                .iter()
                .map(|key| KeyRow {
                    id: key.id.clone(),
                    name: key.name.clone(),
                    created: DateTime::parse_from_rfc3339(&key.created_at)
                        .ok()
                        .map(|at| at.timestamp()),
                    made: key.created_at.clone(),
                    sessions: sessions(&key.sessions),
                })
                .collect()
        }),
        open: state.open.map(|id| id.to_string()),
        connect: state.connect.map(|dialog| ConnectQuestion {
            id: dialog.id.to_string(),
            name: dialog.id.display_name().to_string(),
            with_plugin: page::connects_with_a_plugin(dialog.id),
            connecting: dialog.connecting,
        }),
        key: state.key.as_ref().map(|dialog| match dialog {
            KeyDialog::Naming { name, creating } => KeyQuestion::Naming {
                name: name.clone(),
                creating: *creating,
                can_create: dialog.name_to_create().is_some(),
            },
            KeyDialog::Made(key) => KeyQuestion::Made {
                name: key.name.clone(),
                key: key.key.clone(),
                address: key.address.clone(),
            },
        }),
        notice: held.notice.clone(),
    }
}

fn session_row(
    session: &AssistantSession,
    home: Option<&std::path::Path>,
    now: DateTime<Utc>,
) -> SessionRow {
    SessionRow {
        name: session.folder.as_deref().map(folder_name),
        folder: session.folder.as_deref().map(|f| shown_folder(f, home)),
        agents: u32::try_from(session.agents).unwrap_or(u32::MAX),
        speaking: session.speaking,
        listening: session.listening,
        ago: page::used_ago(session.last_used_at.as_deref(), now).map(|ago| match ago {
            Ago::Now => SessionAgo::Now,
            Ago::Minutes(n) => SessionAgo::Minutes { n },
            Ago::Hours(n) => SessionAgo::Hours { n },
        }),
    }
}

/// What the page tells the user, in the settings app's words.
fn notice_text(notice: Notice) -> String {
    let (key, name, error) = match notice {
        Notice::CouldNotLoad(e) => ("assistants-load-failed", None, e),
        Notice::CouldNotConnect(id, e) => ("assistant-connect-failed", Some(id), e),
        Notice::CouldNotDisconnect(id, e) => ("assistant-disconnect-failed", Some(id), e),
        Notice::CouldNotChange(id, e) => ("assistant-change-failed", Some(id), e),
        Notice::CouldNotCreateKey(e) => ("key-create-failed", None, e),
        Notice::CouldNotRemoveKey(e) => ("key-remove-failed", None, e),
    };
    let mut args = HashMap::from([("error".to_string(), error)]);
    if let Some(id) = name {
        args.insert("name".to_string(), id.display_name().to_string());
    }
    crate::text_with(key, args)
}
