// SPDX-License-Identifier: GPL-3.0-only
//! The sessions of the AI assistants talking to Super Voice. A session is one
//! conversation of one assistant: one Claude Code session, one Codex thread.
//!
//! A request names its session by what its client says of itself (the MCP
//! server's `identity`). A session is kept from its first request until its
//! plugin says it ended, or it goes quiet for too long. The speech and the
//! recording a session starts are its own, so stopping acts on them alone.

use super::calls::Caller;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a session its plugin opened may go without a request. The plugin
/// checks in every minute while the session is open.
pub const OPENED_IDLE: Duration = Duration::from_mins(3);

/// How long a session nothing opened may go without a request: nothing says
/// when it ends.
pub const SEEN_IDLE: Duration = Duration::from_mins(30);

/// What a request says of the session it comes from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Named {
    /// The session's id, as its client names it. `None` when it names none:
    /// such requests are one unnamed session of their caller.
    pub id: Option<String>,
    /// The agent within the session that made the request, when it is not
    /// the session's own, such as a subagent.
    pub agent: Option<String>,
    /// The folder the session works in.
    pub folder: Option<String>,
}

/// The session a request comes from: its caller, what it says of itself,
/// and the connection it came over, if the client keeps one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Who {
    pub caller: Caller,
    pub named: Named,
    pub connection: Option<String>,
}

/// A session as the assistants list shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionView {
    pub caller: Caller,
    pub id: Option<String>,
    pub folder: Option<String>,
    /// How many agents besides its own have made requests.
    pub agents: usize,
    /// The utterance it started last, whether or not it still plays.
    pub utterance: Option<String>,
    /// Whether one of its `listen` calls is recording.
    pub listening: bool,
    pub started_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
}

struct Entry {
    caller: Caller,
    id: Option<String>,
    /// The connection the session's calls last came over, which names it for
    /// a call that names nothing else.
    connection: Option<String>,
    folder: Option<String>,
    agents: BTreeSet<String>,
    started_at: DateTime<Utc>,
    last_active_at: DateTime<Utc>,
    last_active: Instant,
    /// Its plugin said it started, so it checks in while it is open.
    opened: bool,
    utterance: Option<String>,
    recordings: usize,
}

impl Entry {
    fn is(&self, who: &Who) -> bool {
        self.caller == who.caller && self.id == who.named.id
    }

    fn view(&self) -> SessionView {
        SessionView {
            caller: self.caller.clone(),
            id: self.id.clone(),
            folder: self.folder.clone(),
            agents: self.agents.len(),
            utterance: self.utterance.clone(),
            listening: self.recordings > 0,
            started_at: self.started_at,
            last_active_at: self.last_active_at,
        }
    }
}

/// Every session the daemon knows of.
#[derive(Default)]
pub struct Sessions {
    entries: Mutex<Vec<Entry>>,
}

/// A recording one session's `listen` holds. Dropped when the take ends.
#[must_use = "dropping it ends the session's recording at once"]
pub struct Recording {
    sessions: Arc<Sessions>,
    who: Who,
}

impl Drop for Recording {
    fn drop(&mut self) {
        let mut entries = self.sessions.entries.lock();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.is(&self.who)) {
            entry.recordings = entry.recordings.saturating_sub(1);
        }
    }
}

impl Sessions {
    /// Name the session of a call that came over `connection`, the
    /// `Mcp-Session-Id` the client sent, and said `named` of itself.
    ///
    /// A call that names its session ties the connection to it, so a later
    /// call over the same connection that names nothing, such as a model's
    /// whose client dropped what its plugin added, is the same session's. A
    /// call over a connection nothing named is that connection's own session.
    #[must_use]
    pub fn resolve(&self, caller: &Caller, mut named: Named, connection: Option<&str>) -> Named {
        let Some(connection) = connection else {
            return named;
        };
        let mut entries = self.entries.lock();
        if named.id.is_some() {
            // A newer session on the connection, as after Claude Code's
            // `/clear`, takes it over: `touch` ties it to this one.
            for entry in entries.iter_mut().filter(|entry| {
                entry.caller == *caller
                    && entry.id != named.id
                    && entry.connection.as_deref() == Some(connection)
            }) {
                entry.connection = None;
            }
            return named;
        }
        named.id = entries
            .iter()
            .find(|entry| {
                entry.caller == *caller && entry.connection.as_deref() == Some(connection)
            })
            .and_then(|entry| entry.id.clone())
            .or_else(|| Some(connection.to_string()));
        named
    }

    /// A request from `who`: it is active now. A session is kept from its
    /// first request, and learns its folder and agents as requests name them.
    pub fn touch(&self, who: &Who) {
        self.with(who, |_| {});
    }

    /// `who`'s plugin says the session started. It is kept until it says the
    /// session ended, or the session goes quiet for [`OPENED_IDLE`].
    pub fn open(&self, who: &Who) {
        self.with(who, |entry| entry.opened = true);
    }

    /// `who`'s plugin says the session ended.
    pub fn close(&self, who: &Who) {
        self.entries.lock().retain(|entry| !entry.is(who));
    }

    /// `who` started `utterance`.
    pub fn spoke(&self, who: &Who, utterance: String) {
        self.with(who, |entry| entry.utterance = Some(utterance));
    }

    /// The utterance `who` started last, whether or not it still plays.
    #[must_use]
    pub fn utterance(&self, who: &Who) -> Option<String> {
        self.entries
            .lock()
            .iter()
            .find(|entry| entry.is(who))
            .and_then(|entry| entry.utterance.clone())
    }

    /// `who`'s `listen` is recording until the returned guard is dropped.
    pub fn recording(self: &Arc<Self>, who: &Who) -> Recording {
        self.with(who, |entry| entry.recordings += 1);
        Recording {
            sessions: Arc::clone(self),
            who: who.clone(),
        }
    }

    /// Whether one of `who`'s `listen` calls is recording.
    #[must_use]
    pub fn is_recording(&self, who: &Who) -> bool {
        self.entries
            .lock()
            .iter()
            .any(|entry| entry.is(who) && entry.recordings > 0)
    }

    /// Forget every session of `caller`, whose key was revoked.
    pub fn forget(&self, caller: &Caller) {
        self.entries.lock().retain(|entry| entry.caller != *caller);
    }

    /// Forget the sessions gone quiet by `now`. A session still recording is
    /// kept: its `listen` may wait minutes for the user. How many went.
    pub fn sweep(&self, now: Instant) -> usize {
        let mut entries = self.entries.lock();
        let before = entries.len();
        entries.retain(|entry| {
            let idle = if entry.opened { OPENED_IDLE } else { SEEN_IDLE };
            entry.recordings > 0 || now.saturating_duration_since(entry.last_active) < idle
        });
        before - entries.len()
    }

    /// Every session, oldest first.
    #[must_use]
    pub fn list(&self) -> Vec<SessionView> {
        self.entries.lock().iter().map(Entry::view).collect()
    }

    /// Run `change` on `who`'s session, made now if it is new, and mark it
    /// active.
    fn with(&self, who: &Who, change: impl FnOnce(&mut Entry)) {
        let mut entries = self.entries.lock();
        let index = if let Some(index) = entries.iter().position(|entry| entry.is(who)) {
            index
        } else {
            let now = Utc::now();
            entries.push(Entry {
                caller: who.caller.clone(),
                id: who.named.id.clone(),
                connection: None,
                folder: None,
                agents: BTreeSet::new(),
                started_at: now,
                last_active_at: now,
                last_active: Instant::now(),
                opened: false,
                utterance: None,
                recordings: 0,
            });
            entries.len() - 1
        };
        let entry = &mut entries[index];
        entry.last_active = Instant::now();
        entry.last_active_at = Utc::now();
        if let Some(connection) = &who.connection {
            entry.connection = Some(connection.clone());
        }
        if let Some(folder) = &who.named.folder {
            entry.folder = Some(folder.clone());
        }
        if let Some(agent) = &who.named.agent {
            entry.agents.insert(agent.clone());
        }
        change(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(key: &str, id: Option<&str>) -> Who {
        Who {
            caller: Caller::Key(key.to_string()),
            named: Named {
                id: id.map(str::to_string),
                ..Named::default()
            },
            connection: None,
        }
    }

    #[test]
    fn a_session_is_kept_from_its_first_request_and_learns_its_folder() {
        let sessions = Sessions::default();
        let mut one = who("a", Some("s1"));
        sessions.touch(&one);
        one.named.folder = Some("/home/me/project".to_string());
        one.named.agent = Some("sub".to_string());
        sessions.touch(&one);
        sessions.touch(&who("a", Some("s2")));
        sessions.touch(&who("b", Some("s1")));
        let list = sessions.list();
        assert_eq!(list.len(), 3, "{list:?}");
        assert_eq!(list[0].folder.as_deref(), Some("/home/me/project"));
        assert_eq!(list[0].agents, 1);
    }

    /// A connection a session named itself on names it for a call that names
    /// nothing; a newer session on it takes it over; an unnamed connection is
    /// its own session.
    #[test]
    fn a_connection_names_the_session_that_last_named_itself_on_it() {
        let sessions = Sessions::default();
        let caller = Caller::Key("a".to_string());
        let bare = Named::default();
        let tie = |id: &str| {
            let named = sessions.resolve(&caller, who("a", Some(id)).named, Some("conn"));
            sessions.touch(&Who {
                caller: caller.clone(),
                named,
                connection: Some("conn".to_string()),
            });
        };
        tie("s1");
        assert_eq!(
            sessions
                .resolve(&caller, bare.clone(), Some("conn"))
                .id
                .as_deref(),
            Some("s1")
        );
        tie("s2");
        assert_eq!(
            sessions
                .resolve(&caller, bare.clone(), Some("conn"))
                .id
                .as_deref(),
            Some("s2")
        );
        assert_eq!(
            sessions
                .resolve(&caller, bare.clone(), Some("other"))
                .id
                .as_deref(),
            Some("other")
        );
        assert_eq!(sessions.resolve(&caller, bare, None).id, None);
    }

    /// Each session's speech and recording are its own.
    #[test]
    fn speech_and_recording_belong_to_their_session() {
        let sessions = Arc::new(Sessions::default());
        let one = who("a", Some("s1"));
        let two = who("a", Some("s2"));
        sessions.spoke(&one, "utt_1".to_string());
        assert_eq!(sessions.utterance(&one).as_deref(), Some("utt_1"));
        assert_eq!(sessions.utterance(&two), None);

        let take = sessions.recording(&two);
        assert!(sessions.is_recording(&two));
        assert!(!sessions.is_recording(&one));
        drop(take);
        assert!(!sessions.is_recording(&two));
    }

    #[test]
    fn a_session_ends_when_its_plugin_says_so_or_its_key_is_revoked() {
        let sessions = Sessions::default();
        let one = who("a", Some("s1"));
        sessions.open(&one);
        sessions.touch(&who("a", Some("s2")));
        sessions.touch(&who("b", None));
        sessions.close(&one);
        assert_eq!(sessions.list().len(), 2);
        sessions.forget(&Caller::Key("a".to_string()));
        let list = sessions.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, None);
    }

    /// One its plugin opened goes after a few quiet minutes, one nothing
    /// opened after half an hour, and one recording stays.
    #[test]
    fn quiet_sessions_are_swept() {
        let sessions = Arc::new(Sessions::default());
        let opened = who("a", Some("opened"));
        let seen = who("a", Some("seen"));
        let listening = who("a", Some("listening"));
        sessions.open(&opened);
        sessions.touch(&seen);
        let take = sessions.recording(&listening);

        let now = Instant::now();
        assert_eq!(sessions.sweep(now + OPENED_IDLE / 2), 0);
        assert_eq!(
            sessions.sweep(now + OPENED_IDLE + Duration::from_secs(1)),
            1
        );
        assert_eq!(sessions.sweep(now + SEEN_IDLE + Duration::from_secs(1)), 1);
        let left = sessions.list();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id.as_deref(), Some("listening"));
        drop(take);
        assert_eq!(sessions.sweep(now + SEEN_IDLE + Duration::from_secs(1)), 1);
    }
}
