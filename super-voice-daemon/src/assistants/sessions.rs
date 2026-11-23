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

/// How long after a dictation a prompt that holds it counts as spoken: time
/// to look it over and send it. Kept short, as the dictation may have gone
/// to any window: a prompt typed long after is the user's own.
pub const DICTATION_FRESH: Duration = Duration::from_secs(90);

/// How many of a session's replies are kept to read aloud again.
pub const READ_BACK: usize = 10;

/// Every session the daemon knows of.
#[derive(Default)]
pub struct Sessions {
    entries: Mutex<Vec<Entry>>,
    /// What the user last dictated into a window, and when: a prompt that
    /// holds it was spoken.
    dictated: Mutex<Option<(Vec<String>, Instant)>>,
}

/// The words of `text` as they are compared: lower case, letters and
/// digits. In a script written without spaces between words, as Chinese,
/// Japanese and Thai are, each character counts as a word, so a corrected
/// word there costs a character or two rather than its whole clause.
fn words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if is_written_without_spaces(c) {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            words.push(c.to_string());
        } else if c.is_alphanumeric() {
            word.push(c);
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Whether `c` belongs to a script written without spaces between words:
/// Thai, Lao, Burmese, Khmer, Japanese kana, and the Chinese characters
/// Chinese and Japanese share. Korean puts spaces between its words.
fn is_written_without_spaces(c: char) -> bool {
    matches!(c,
        '\u{0E00}'..='\u{0EFF}' // Thai, Lao
        | '\u{1000}'..='\u{109F}' // Burmese
        | '\u{1780}'..='\u{17FF}' // Khmer
        | '\u{3040}'..='\u{30FF}' // Hiragana, Katakana
        | '\u{31F0}'..='\u{31FF}' // Katakana for Ainu
        | '\u{3400}'..='\u{4DBF}' // Chinese characters, extension A
        | '\u{4E00}'..='\u{9FFF}' // Chinese characters
        | '\u{F900}'..='\u{FAFF}' // Chinese characters for compatibility
        | '\u{FF66}'..='\u{FF9F}' // Halfwidth Katakana
        | '\u{20000}'..='\u{3FFFF}' // Chinese characters, later extensions
    )
}

/// Whether a prompt of `prompt`'s words holds a dictation of `dictated`'s:
/// all of it, in order, but for about a word in four the user corrected by
/// hand, with anything they typed around it; or the prompt is the dictation
/// cut down, keeping at least half of it. A dictation of three words or
/// fewer is matched whole: cut down, its few words are too common to tell
/// the user's typing from it.
fn holds(prompt: &[String], dictated: &[String]) -> bool {
    /// The longest dictation that must be held whole.
    const SHORT: usize = 3;
    if prompt.is_empty() || dictated.is_empty() {
        return false;
    }
    let kept = words_in_common(prompt, dictated);
    let nearly_all = |of: usize| kept >= of - of / 4;
    let cut_down =
        dictated.len() > SHORT && 2 * prompt.len() >= dictated.len() && nearly_all(prompt.len());
    nearly_all(dictated.len()) || cut_down
}

/// How many words `a` and `b` share in the same order: the length of their
/// longest common subsequence.
fn words_in_common(a: &[String], b: &[String]) -> usize {
    let mut row = vec![0; b.len() + 1];
    for word in a {
        let mut diagonal = 0;
        for (j, other) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if word == other {
                diagonal + 1
            } else {
                above.max(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
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

    /// `who` asked to speak, on `task`.
    pub fn asked_to_speak(&self, who: &Who, task: AbortHandle) {
        self.with(who, |entry| entry.speaking = Some(task));
    }

    /// The task making `who`'s last speech, while it is still at it.
    #[must_use]
    pub fn speaking(&self, who: &Who) -> Option<AbortHandle> {
        self.entries
            .lock()
            .iter()
            .find(|entry| entry.is(who))
            .and_then(|entry| entry.speaking.clone())
            .filter(|task| !task.is_finished())
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
        self.entries
            .lock()
            .iter()
            .any(|entry| entry.is(who) && entry.recordings > 0)
    }

    /// The user dictated `text` into a window, as a prompt may be.
    pub fn dictated(&self, text: &str) {
        let text = words(text);
        if !text.is_empty() {
            *self.dictated.lock() = Some((text, Instant::now()));
        }
    }

    /// Whether `prompt` holds the user's last dictation, made within
    /// [`DICTATION_FRESH`] of `now`, as [`holds`] says: a word or two the
    /// user corrected by hand still counts. A match uses the dictation up,
    /// so the next prompt is not spoken for it too.
    pub fn was_dictated(&self, prompt: &str, now: Instant) -> bool {
        let prompt = words(prompt);
        let mut dictated = self.dictated.lock();
        let matches = dictated.as_ref().is_some_and(|(text, at)| {
            now.saturating_duration_since(*at) < DICTATION_FRESH && holds(&prompt, text)
        });
        if matches {
            *dictated = None;
        }
        matches
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
        let entries = self.entries.lock();
        let replies = entries
            .iter()
            .find(|entry| entry.is(who))
            .map(|entry| &entry.replies);
        match replies {
            Some(replies) => replies.get(back).cloned().ok_or(replies.len()),
            None => Err(0),
        }
    }

    /// Whether `who`'s turn under way was spoken, once its plugin has said.
    #[must_use]
    pub fn spoken(&self, who: &Who) -> Option<bool> {
        self.entries
            .lock()
            .iter()
            .find(|entry| entry.is(who))
            .and_then(|entry| entry.spoken)
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

    /// A prompt that holds the user's fresh dictation was spoken, once; one
    /// typed, or sent long after, was not.
    #[test]
    fn a_prompt_holding_a_fresh_dictation_was_spoken() {
        let sessions = Sessions::default();
        let now = Instant::now();
        assert!(!sessions.was_dictated("Open the release notes.", now));
        sessions.dictated("Open the release notes");
        assert!(sessions.was_dictated("  open the RELEASE notes! ", now));
        assert!(
            !sessions.was_dictated("Open the release notes.", now),
            "used up"
        );

        sessions.dictated("open the notes");
        assert!(!sessions.was_dictated("something else", now));
        assert!(!sessions.was_dictated(
            "open the notes",
            now + DICTATION_FRESH + Duration::from_secs(1)
        ));
        // A minute to look it over is enough.
        sessions.dictated("open the notes");
        assert!(sessions.was_dictated("open the notes", now + Duration::from_secs(60)));
        sessions.dictated("   ");
        assert!(!sessions.was_dictated("", now));
    }

    /// A dictation the user touched up by hand is still theirs: a word or
    /// two corrected, text typed around it, or the end cut off. A prompt that
    /// only shares a few of its words, or a short one, is not.
    #[test]
    fn a_dictation_corrected_by_hand_still_counts() {
        let dictation = "Rename the super boys plugin folder to super voice please";
        let spoken = |prompt: &str| {
            let sessions = Sessions::default();
            sessions.dictated(dictation);
            sessions.was_dictated(prompt, Instant::now())
        };
        // Ten words: two may be corrected.
        assert!(spoken(
            "Rename the Super Voice plugin folder to super-voice, please."
        ));
        assert!(spoken(
            "In this repo: rename the super boys plugin folder to super voice please. Thanks!"
        ));
        assert!(spoken("Rename the super boys plugin folder to super voice"));
        assert!(!spoken(
            "Rename the voice plugin directory to voice, thanks"
        ));
        assert!(!spoken("please"));
        assert!(!spoken("Delete the plugin folder"));

        // Chinese and Japanese are written without spaces: a corrected word
        // costs its characters, not the whole clause.
        let said = |dictation: &str, prompt: &str| {
            let sessions = Sessions::default();
            sessions.dictated(dictation);
            sessions.was_dictated(prompt, Instant::now())
        };
        let misheard = "把插件文件夹重命名为超级鱼音";
        assert!(said(misheard, "把插件文件夹重命名为超级语音"));
        assert!(said(misheard, "请把插件文件夹重命名为超级鱼音谢谢"));
        assert!(!said(misheard, "删除插件文件夹"));
        assert!(said(
            "スーパーボーイズの設定を開いて",
            "スーパーボイスの設定を開いて"
        ));
        assert_eq!(
            words("打开 README 文件。"),
            ["打", "开", "readme", "文", "件"]
        );

        // Three words or fewer: whole, or not at all. Cut down, they would
        // be a word or two the user may well have typed.
        let sessions = Sessions::default();
        sessions.dictated("Open the notes");
        assert!(!sessions.was_dictated("Open my notes", Instant::now()));
        assert!(!sessions.was_dictated("open notes", Instant::now()));
        assert!(sessions.was_dictated("open the notes!", Instant::now()));
        assert!(!said("Yes please", "yes"));
        assert!(said("Yes please", "Yes, please."));
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
