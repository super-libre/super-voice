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
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::AbortHandle;

/// How long a session its plugin opened may go without a request. The plugin
/// checks in at least once a minute while the session is open.
pub const OPENED_IDLE: Duration = Duration::from_mins(3);

/// How long a session nothing opened may go without a request: nothing says
/// when it ends.
pub const SEEN_IDLE: Duration = Duration::from_mins(30);

/// How long a session is remembered once it has left the list: its turn,
/// spoken or typed, and its replies. A turn may run long with nothing said
/// to Super Voice, as a Codex task working on its own does, and is still the
/// turn it was when it ends.
pub const REMEMBERED: Duration = Duration::from_hours(24);

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
    /// When it last spoke or listened. A request that only checks in, like
    /// its plugin's heartbeat, is not a use.
    pub last_used_at: Option<DateTime<Utc>>,
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
    /// When it last made any request, which keeps it listed.
    last_active: Instant,
    /// When it last spoke or listened.
    used_at: Option<DateTime<Utc>>,
    /// Its plugin said it started, so it checks in while it is open.
    opened: bool,
    utterance: Option<String>,
    /// The task making its last speech, from the moment it was asked for:
    /// its speech while it waits for the model and is made, before any
    /// utterance plays.
    speaking: Option<AbortHandle>,
    /// How many times its speech was stopped, so a speech of its own waiting
    /// its turn knows the user asked for quiet.
    stops: u64,
    recordings: usize,
    /// Whether the turn under way was spoken, once its plugin has said.
    spoken: Option<bool>,
    /// The prompt that started it, as its plugin last said.
    prompt: Option<String>,
    /// Its last [`READ_BACK`] replies, as its plugin said them, the last one
    /// first.
    replies: VecDeque<String>,
}

impl Entry {
    fn is(&self, who: &Who) -> bool {
        self.caller == who.caller && self.id == who.named.id
    }

    /// Whether it is listed at `now`: it records, or has not been quiet for
    /// longer than a session like it may be.
    fn is_listed(&self, now: Instant) -> bool {
        let idle = if self.opened { OPENED_IDLE } else { SEEN_IDLE };
        self.recordings > 0 || now.saturating_duration_since(self.last_active) < idle
    }

    /// What names it to people: the last part of the folder it works in.
    fn label(&self) -> Option<String> {
        self.folder.as_deref().map(folder_name)
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
            last_used_at: self.used_at,
        }
    }
}

/// The last part of `folder`, or all of it when it has none, as `/` does.
fn folder_name(folder: &str) -> String {
    std::path::Path::new(folder).file_name().map_or_else(
        || folder.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// How many of a session's replies are kept to read aloud again.
pub const READ_BACK: usize = 10;

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
        self.with(who, |entry| {
            entry.utterance = Some(utterance);
            entry.used_at = Some(Utc::now());
        });
    }

    /// What names `who`'s session to people, as the app's Assistants page
    /// names it. `None` when nothing does.
    #[must_use]
    pub fn label(&self, who: &Who) -> Option<String> {
        self.read(who, Entry::label).flatten()
    }

    /// `who` asked to speak, on `task`.
    pub fn asked_to_speak(&self, who: &Who, task: AbortHandle) {
        self.with(who, |entry| entry.speaking = Some(task));
    }

    /// `who`'s speech was stopped: a speech of its own waiting its turn is
    /// not said.
    pub fn speech_stopped(&self, who: &Who) {
        self.with(who, |entry| entry.stops += 1);
    }

    /// How many times `who`'s speech was stopped.
    #[must_use]
    pub fn stops(&self, who: &Who) -> u64 {
        self.read(who, |entry| entry.stops).unwrap_or(0)
    }

    /// The task making `who`'s last speech, while it is still at it.
    #[must_use]
    pub fn speaking(&self, who: &Who) -> Option<AbortHandle> {
        self.read(who, |entry| entry.speaking.clone())
            .flatten()
            .filter(|task| !task.is_finished())
    }

    /// The utterance `who` started last, whether or not it still plays.
    #[must_use]
    pub fn utterance(&self, who: &Who) -> Option<String> {
        self.read(who, |entry| entry.utterance.clone()).flatten()
    }

    /// `who`'s `listen` is recording until the returned guard is dropped.
    pub fn recording(self: &Arc<Self>, who: &Who) -> Recording {
        self.with(who, |entry| {
            entry.recordings += 1;
            entry.used_at = Some(Utc::now());
        });
        Recording {
            sessions: Arc::clone(self),
            who: who.clone(),
        }
    }

    /// Whether one of `who`'s `listen` calls is recording.
    #[must_use]
    pub fn is_recording(&self, who: &Who) -> bool {
        self.read(who, |entry| entry.recordings > 0) == Some(true)
    }

    /// `who`'s turn under way was `spoken`, or typed.
    pub fn set_spoken(&self, who: &Who, spoken: bool) {
        self.with(who, |entry| entry.spoken = Some(spoken));
    }

    /// `who`'s plugin says the user sent `prompt`. Said `again`, as a hook
    /// that runs before each model call of a turn says it, and the prompt the
    /// turn under way started with: what that turn was. Otherwise `None`: a
    /// new turn, whose prompt it keeps. The same text sent in a new turn is a
    /// new turn.
    pub fn prompted(&self, who: &Who, prompt: &str, again: bool) -> Option<bool> {
        let mut said = None;
        self.with(who, |entry| {
            if again && entry.prompt.as_deref() == Some(prompt) {
                said = entry.spoken;
            } else {
                entry.prompt = Some(prompt.to_string());
            }
        });
        said
    }

    /// `who`'s plugin says the assistant answered its turn with `reply`. The
    /// same reply said again, as a turn that ended before the model ran
    /// says, is kept once.
    pub fn replied(&self, who: &Who, reply: &str) {
        let reply = reply.trim();
        if reply.is_empty() {
            return;
        }
        self.with(who, |entry| {
            if entry.replies.front().map(String::as_str) != Some(reply) {
                entry.replies.push_front(reply.to_string());
                entry.replies.truncate(READ_BACK);
            }
        });
    }

    /// `who`'s reply `back` replies before its last one.
    ///
    /// # Errors
    ///
    /// How many replies it has kept, when there is none that far back.
    pub fn reply(&self, who: &Who, back: usize) -> Result<String, usize> {
        self.read(who, |entry| {
            entry.replies.get(back).cloned().ok_or(entry.replies.len())
        })
        .unwrap_or(Err(0))
    }

    /// Whether `who`'s turn under way was spoken, once its plugin has said.
    #[must_use]
    pub fn spoken(&self, who: &Who) -> Option<bool> {
        self.read(who, |entry| entry.spoken).flatten()
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
            entry.recordings > 0 || now.saturating_duration_since(entry.last_active) < REMEMBERED
        });
        before - entries.len()
    }

    /// Every session listed, oldest first.
    #[must_use]
    pub fn list(&self) -> Vec<SessionView> {
        self.list_at(Instant::now())
    }

    /// Every session listed at `now`, oldest first.
    fn list_at(&self, now: Instant) -> Vec<SessionView> {
        self.entries
            .lock()
            .iter()
            .filter(|entry| entry.is_listed(now))
            .map(Entry::view)
            .collect()
    }

    /// What `look` finds in `who`'s session, when there is one.
    fn read<T>(&self, who: &Who, look: impl FnOnce(&Entry) -> T) -> Option<T> {
        self.entries
            .lock()
            .iter()
            .find(|entry| entry.is(who))
            .map(look)
    }

    /// Run `change` on `who`'s session, made now if it is new, and mark it
    /// active.
    fn with(&self, who: &Who, change: impl FnOnce(&mut Entry)) {
        let mut entries = self.entries.lock();
        let index = if let Some(index) = entries.iter().position(|entry| entry.is(who)) {
            index
        } else {
            entries.push(Entry {
                caller: who.caller.clone(),
                id: who.named.id.clone(),
                connection: None,
                folder: None,
                agents: BTreeSet::new(),
                started_at: Utc::now(),
                last_active: Instant::now(),
                used_at: None,
                opened: false,
                utterance: None,
                speaking: None,
                stops: 0,
                recordings: 0,
                spoken: None,
                prompt: None,
                replies: VecDeque::new(),
            });
            entries.len() - 1
        };
        let entry = &mut entries[index];
        entry.last_active = Instant::now();
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

    /// A session is named by its folder's last part, once a request says it.
    #[test]
    fn a_session_is_named_by_its_folder() {
        let sessions = Sessions::default();
        let mut one = who("a", Some("s1"));
        sessions.touch(&one);
        assert_eq!(sessions.label(&one), None);
        one.named.folder = Some("/home/me/super-voice".to_string());
        sessions.touch(&one);
        assert_eq!(sessions.label(&one).as_deref(), Some("super-voice"));
        assert_eq!(sessions.label(&who("a", Some("s2"))), None);
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

    /// A session is used when it speaks or listens, not when it only checks
    /// in, as a plugin's heartbeat does.
    #[test]
    fn a_session_is_used_when_it_speaks_or_listens() {
        let sessions = Arc::new(Sessions::default());
        let speaks = who("a", Some("s1"));
        let listens = who("a", Some("s2"));
        sessions.open(&speaks);
        sessions.touch(&speaks);
        sessions.touch(&listens);
        assert!(sessions.list().iter().all(|s| s.last_used_at.is_none()));

        sessions.spoke(&speaks, "utt_1".to_string());
        drop(sessions.recording(&listens));
        assert!(sessions.list().iter().all(|s| s.last_used_at.is_some()));
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

    /// One its plugin opened leaves the list after a few quiet minutes, one
    /// nothing opened after half an hour, and one recording stays. Off the
    /// list, a session still holds its turn, until a day has gone by.
    #[test]
    fn quiet_sessions_leave_the_list_and_are_forgotten_later() {
        let sessions = Arc::new(Sessions::default());
        let opened = who("a", Some("opened"));
        let seen = who("a", Some("seen"));
        let listening = who("a", Some("listening"));
        sessions.open(&opened);
        sessions.touch(&seen);
        sessions.set_spoken(&seen, false);
        let take = sessions.recording(&listening);
        let listed = |at| {
            sessions
                .list_at(at)
                .into_iter()
                .filter_map(|session| session.id)
                .collect::<Vec<_>>()
        };

        let now = Instant::now();
        assert_eq!(listed(now + OPENED_IDLE / 2).len(), 3);
        assert_eq!(
            listed(now + OPENED_IDLE + Duration::from_secs(1)),
            ["seen", "listening"]
        );
        let after_half_an_hour = now + SEEN_IDLE + Duration::from_secs(1);
        assert_eq!(listed(after_half_an_hour), ["listening"]);
        // A typed turn stays typed off the list: a long task's speech at
        // its end is still refused.
        assert_eq!(sessions.sweep(after_half_an_hour), 0);
        assert_eq!(sessions.spoken(&seen), Some(false));

        drop(take);
        let after_a_day = now + REMEMBERED + Duration::from_secs(1);
        assert_eq!(sessions.sweep(after_a_day), 3);
        assert_eq!(sessions.spoken(&seen), None);
    }

    /// A session keeps its last replies, the last one first, each once, and
    /// says how many it has when asked for one further back.
    #[test]
    fn a_session_keeps_its_last_replies() {
        let sessions = Sessions::default();
        let one = who("a", Some("s1"));
        assert_eq!(sessions.reply(&one, 0), Err(0));
        sessions.replied(&one, " First. ");
        sessions.replied(&one, "Second.");
        // A turn that ended before the model ran says its last reply again.
        sessions.replied(&one, "Second.");
        sessions.replied(&one, "  ");
        assert_eq!(sessions.reply(&one, 0).as_deref(), Ok("Second."));
        assert_eq!(sessions.reply(&one, 1).as_deref(), Ok("First."));
        assert_eq!(sessions.reply(&one, 2), Err(2));
        assert_eq!(sessions.reply(&who("a", Some("s2")), 0), Err(0));

        for reply in 0..READ_BACK + 2 {
            sessions.replied(&one, &format!("Reply {reply}."));
        }
        assert_eq!(sessions.reply(&one, 0).as_deref(), Ok("Reply 11."));
        assert_eq!(
            sessions.reply(&one, READ_BACK - 1).as_deref(),
            Ok("Reply 2.")
        );
        assert_eq!(sessions.reply(&one, READ_BACK), Err(READ_BACK));
    }

    /// A session's turn is spoken or typed once its plugin says, and the next
    /// prompt says again.
    #[test]
    fn a_turn_is_spoken_or_typed() {
        let sessions = Sessions::default();
        let one = who("a", Some("s1"));
        assert_eq!(sessions.spoken(&one), None);
        sessions.set_spoken(&one, true);
        assert_eq!(sessions.spoken(&one), Some(true));
        sessions.set_spoken(&one, false);
        assert_eq!(sessions.spoken(&one), Some(false));
        assert_eq!(sessions.spoken(&who("a", Some("s2"))), None);
    }
}
