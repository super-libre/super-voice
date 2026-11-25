// SPDX-License-Identifier: GPL-3.0-only
//! The Notes page for an app written in another language: a store that holds
//! the page's state, runs the core's logic for it (`pages::notes`) on a tokio
//! runtime of its own, and tells the app whenever the page changed.
//!
//! The app sends what the user did ([`NotesAction`], and the editor's text
//! through [`NotesStore::set_text`]) and draws the [`NotesPage`] it is handed.
//! Everything else, the writes, the save delay, a take's stream, happens here,
//! as it does in the settings app.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use futures_util::StreamExt;
use super_voice_app_core::notes::{self, Note};
use super_voice_app_core::pages::notes::{self as page, Effect, Input, Notice};
use super_voice_app_core::state::TextBuffer;
use super_voice_app_core::state::notes::NotesState;

use crate::RUNTIME;

/// What the app is told. Called from the store's own threads: the app hops to
/// its main thread before it draws.
#[uniffi::export(with_foreign)]
pub trait NotesObserver: Send + Sync {
    /// The page changed; draw this.
    fn changed(&self, page: NotesPage);

    /// Ask the user where to export the open note, suggesting `name`, then
    /// call [`NotesStore::export_to`] with the answer and this `text`.
    fn export_requested(&self, name: String, text: String);
}

/// What the user did on the page.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum NotesAction {
    /// Read the folder again: the page was opened.
    Reload,
    /// Record a new note.
    Record,
    /// Record more into the open note.
    DictateMore,
    /// Stop the take.
    Stop,
    /// Open a note in place of the list.
    Open { id: String },
    /// Back to the list.
    Close,
    /// Save the open note's edits now: the user left the page.
    SaveNow,
    /// Export the open note.
    Export,
    /// Delete was pressed once.
    DeleteRequested,
    /// The delete was called off.
    DeleteCancelled,
    /// Delete the open note.
    DeleteConfirmed,
}

impl From<NotesAction> for Input {
    fn from(action: NotesAction) -> Self {
        match action {
            NotesAction::Reload => Self::Reload,
            NotesAction::Record => Self::Record,
            NotesAction::DictateMore => Self::DictateMore,
            NotesAction::Stop => Self::Stop,
            NotesAction::Open { id } => Self::Open(id),
            NotesAction::Close => Self::Close,
            NotesAction::SaveNow => Self::SaveNow,
            NotesAction::Export => Self::Export,
            NotesAction::DeleteRequested => Self::DeleteRequested,
            NotesAction::DeleteCancelled => Self::DeleteCancelled,
            NotesAction::DeleteConfirmed => Self::DeleteConfirmed,
        }
    }
}

/// One note in the list.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NoteSummary {
    /// The note's id: its file name without the extension.
    pub id: String,
    /// When the note was made, as seconds since 1970, for the app to say in
    /// its own words ("Today, 08:02").
    pub created: i64,
    /// What the note says.
    pub text: String,
    /// How many words it has.
    pub words: u32,
}

impl From<&Note> for NoteSummary {
    fn from(note: &Note) -> Self {
        Self {
            id: note.id.clone(),
            created: unix_seconds(note),
            text: note.text.clone(),
            words: count(note.words()),
        }
    }
}

/// The note open in place of the list.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OpenNotePage {
    pub id: String,
    /// When it was made, as seconds since 1970.
    pub created: i64,
    /// What its editor holds.
    pub text: String,
    /// How many words that is.
    pub words: u32,
    /// Whether the file holds everything the editor does.
    pub saved: bool,
    /// How many times the store replaced the text (words dictated into the
    /// note). When it goes up, the app's editor takes [`text`](Self::text).
    pub replaced: u64,
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum NotesNotice {
    CouldNotRead { message: String },
    CouldNotSave { message: String },
    CouldNotDelete { message: String },
    CouldNotExport { message: String },
    NoSpeech,
    AnotherRecording,
    RecordingFailed { message: String },
}

impl From<Notice> for NotesNotice {
    fn from(notice: Notice) -> Self {
        match notice {
            Notice::CouldNotRead(message) => Self::CouldNotRead { message },
            Notice::CouldNotSave(message) => Self::CouldNotSave { message },
            Notice::CouldNotDelete(message) => Self::CouldNotDelete { message },
            Notice::CouldNotExport(message) => Self::CouldNotExport { message },
            Notice::NoSpeech => Self::NoSpeech,
            Notice::AnotherRecording => Self::AnotherRecording,
            Notice::RecordingFailed(message) => Self::RecordingFailed { message },
        }
    }
}

/// Everything the page shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NotesPage {
    /// Whether the folder has been read. Until it has, no notes means "not
    /// read yet", not "none".
    pub loaded: bool,
    /// The notes, newest first.
    pub notes: Vec<NoteSummary>,
    /// The note open in place of the list.
    pub open: Option<OpenNotePage>,
    /// Whether a take is recording.
    pub recording: bool,
    /// What the take has heard so far.
    pub preview: String,
    /// Whether Delete was pressed once and waits for the second.
    pub confirming_delete: bool,
    /// Where the open note was last exported to.
    pub exported: Option<String>,
    /// What to tell the user.
    pub notice: Option<NotesNotice>,
    /// The folder the notes are kept in.
    pub folder: String,
}

/// The open note's text as the store holds it.
///
/// The app keeps its own editor. It hands its text over with
/// [`NotesStore::set_text`], and takes the store's back only when the store
/// replaced it (words dictated into the note), which `replaced` counts. Taking
/// it back on every change would undo keystrokes still on their way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Buffer {
    text: String,
    replaced: u64,
}

impl TextBuffer for Buffer {
    fn from_text(text: &str) -> Self {
        Self {
            text: text.to_string(),
            replaced: 0,
        }
    }

    fn text(&self) -> String {
        self.text.clone()
    }

    fn replace_text(&mut self, text: &str) {
        text.clone_into(&mut self.text);
        self.replaced += 1;
    }
}

/// The page's state, and the notice it shows.
#[derive(Default)]
struct Held {
    state: NotesState<Buffer>,
    notice: Option<NotesNotice>,
}

/// The Notes page's store.
#[derive(uniffi::Object)]
pub struct NotesStore {
    held: Mutex<Held>,
    observer: Arc<dyn NotesObserver>,
    /// The effects asked for, kept instead of run: the tests' store, which
    /// must never write to the real notes folder.
    #[cfg(test)]
    recorded: Option<Mutex<Vec<Effect>>>,
}

#[uniffi::export]
impl NotesStore {
    /// A store that tells `observer` what changes. Nothing is read until the
    /// app sends [`NotesAction::Reload`].
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn NotesObserver>) -> Arc<Self> {
        crate::identify();
        Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
            #[cfg(test)]
            recorded: None,
        })
    }

    /// What the page shows now.
    pub fn page(&self) -> NotesPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: NotesAction) {
        self.dispatch(action.into());
    }

    /// The open note's editor now holds `text`.
    pub fn set_text(self: Arc<Self>, text: String) {
        let changed = {
            let mut held = self.lock();
            match held.state.open.as_mut() {
                Some(open) if open.content.text != text => {
                    open.content.text = text;
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.dispatch(Input::Edited);
        }
    }

    /// The export dialog was answered: write `text` to `path`, or nothing
    /// when the user closed it (`None`).
    pub fn export_to(self: Arc<Self>, path: Option<String>, text: String) {
        let store = Arc::clone(&self);
        RUNTIME.spawn(async move {
            let result = match path {
                None => Ok(None),
                Some(chosen) => {
                    let to = notes::export_path(&PathBuf::from(chosen));
                    notes::export(&text, &to)
                        .await
                        .map(|()| Some(to.display().to_string()))
                        .map_err(|e| format!("{}: {e}", to.display()))
                }
            };
            store.dispatch(Input::Exported(result));
        });
    }

    /// The app is quitting: write everything not yet written, now.
    pub fn save_before_exit(self: Arc<Self>) {
        self.dispatch(Input::SaveBeforeExit);
    }
}

impl NotesStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hand `input` to the page's logic, tell the app what changed, and start
    /// the work it asked for.
    fn dispatch(self: Arc<Self>, input: Input) {
        let (page, effects) = {
            let mut held = self.lock();
            let outcome = page::update(&mut held.state, input);
            if outcome.clear_notice {
                held.notice = None;
            }
            if let Some(notice) = outcome.notice {
                held.notice = Some(notice.into());
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
        #[cfg(test)]
        if let Some(recorded) = &self.recorded {
            recorded.lock().expect("recorded").push(effect);
            return;
        }
        match effect {
            Effect::List => self.answer(page::run_list()),
            Effect::Write(job) => self.answer(page::run_write(job)),
            Effect::Create(text) => self.answer(page::run_create(text)),
            Effect::SaveAfter(edits) => self.answer(page::run_save_after(edits)),
            Effect::GiveUpLater => self.answer(page::run_give_up()),
            Effect::StopTake => {
                RUNTIME.spawn(page::run_stop());
            }
            Effect::StartTake => {
                RUNTIME.spawn(async move {
                    let mut take = std::pin::pin!(page::take());
                    while let Some(input) = take.next().await {
                        Arc::clone(&self).dispatch(input);
                    }
                });
            }
            Effect::Export { name, text } => self.observer.export_requested(name, text),
            Effect::WriteNow { jobs, new_note } => page::write_now(jobs, new_note),
            // The app quits on its own terms; the store has nothing left to do.
            Effect::Exit => {}
        }
    }

    /// Run `work` and hand its answer back to the page.
    fn answer(self: Arc<Self>, work: impl Future<Output = Input> + Send + 'static) {
        RUNTIME.spawn(async move {
            let input = work.await;
            self.dispatch(input);
        });
    }
}

fn snapshot(held: &Held) -> NotesPage {
    let state = &held.state;
    NotesPage {
        loaded: state.loaded,
        notes: state.items.iter().map(NoteSummary::from).collect(),
        open: state.open.as_ref().map(|open| OpenNotePage {
            id: open.id.clone(),
            created: state.note(&open.id).map_or(0, unix_seconds),
            words: count(notes::words(&open.content.text)),
            text: open.content.text.clone(),
            saved: !open.unsaved(),
            replaced: open.content.replaced,
        }),
        recording: state.take.is_some(),
        preview: state
            .take
            .as_ref()
            .map_or_default(|take| take.preview.clone()),
        confirming_delete: state.confirming_delete,
        exported: state.exported.clone(),
        notice: held.notice.clone(),
        folder: notes::dir().display().to_string(),
    }
}

/// When `note` was made, as seconds since 1970, read in local time.
fn unix_seconds(note: &Note) -> i64 {
    note.created
        .and_local_timezone(chrono::Local)
        .earliest()
        .map_or(0, |at| at.timestamp())
}

fn count(words: usize) -> u32 {
    u32::try_from(words).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records what the store says, for the tests to read.
    #[derive(Default)]
    struct Seen {
        pages: Mutex<Vec<NotesPage>>,
    }

    impl NotesObserver for Seen {
        fn changed(&self, page: NotesPage) {
            self.pages.lock().expect("pages").push(page);
        }

        fn export_requested(&self, _name: String, _text: String) {}
    }

    /// A store holding one note, whose effects are recorded, never run.
    fn store_with(note_text: &str) -> (Arc<NotesStore>, Arc<Seen>) {
        let seen = Arc::new(Seen::default());
        let store = Arc::new(NotesStore {
            held: Mutex::new(Held::default()),
            observer: Arc::clone(&seen) as Arc<dyn NotesObserver>,
            recorded: Some(Mutex::new(Vec::new())),
        });
        {
            let mut held = store.lock();
            held.state.put(Note {
                id: "2026-11-20_10-00-00".to_string(),
                created: chrono::NaiveDateTime::parse_from_str(
                    "2026-11-20 10:00:00",
                    "%Y-%m-%d %H:%M:%S",
                )
                .expect("a time"),
                text: note_text.to_string(),
            });
        }
        (store, seen)
    }

    #[test]
    fn opening_a_note_shows_it_saved_and_counted() {
        let (store, seen) = store_with("one two three");
        Arc::clone(&store).send(NotesAction::Open {
            id: "2026-11-20_10-00-00".into(),
        });
        let page = seen
            .pages
            .lock()
            .expect("pages")
            .last()
            .cloned()
            .expect("a page");
        let open = page.open.expect("open");
        assert_eq!(open.text, "one two three");
        assert_eq!(open.words, 3);
        assert!(open.saved);
    }

    #[test]
    fn a_change_in_the_editor_is_unsaved_until_written() {
        let (store, _seen) = store_with("draft");
        Arc::clone(&store).send(NotesAction::Open {
            id: "2026-11-20_10-00-00".into(),
        });
        Arc::clone(&store).set_text("draft, longer".into());
        let open = store.page().open.expect("open");
        assert_eq!(open.text, "draft, longer");
        assert!(!open.saved);
        // The same text again is no edit.
        Arc::clone(&store).set_text("draft, longer".into());
        assert_eq!(store.lock().state.open.as_ref().expect("open").edits, 1);
        // The save waits for the edits to stand still.
        let recorded = store.recorded.as_ref().expect("recorded");
        assert_eq!(*recorded.lock().expect("recorded"), [Effect::SaveAfter(1)]);
    }

    #[test]
    fn export_asks_the_app_where_with_what_the_editor_holds() {
        let (store, _seen) = store_with("saved");
        Arc::clone(&store).send(NotesAction::Open {
            id: "2026-11-20_10-00-00".into(),
        });
        Arc::clone(&store).set_text("typed".into());
        Arc::clone(&store).send(NotesAction::Export);
        let recorded = store.recorded.as_ref().expect("recorded");
        let last = recorded.lock().expect("recorded").last().cloned();
        assert!(matches!(last, Some(Effect::Export { text, .. }) if text == "typed"));
    }
}
