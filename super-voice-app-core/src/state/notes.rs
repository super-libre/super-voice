// SPDX-License-Identifier: GPL-3.0-only
//! The Notes tab: the notes the app keeps, the one open, and a take recording
//! into one.
//!
//! The files are the truth (see [`crate::notes`]). The list here holds what
//! was last read, plus every write queued since: a note's text in the list is
//! its newest, even while its file is still being written.
//!
//! Writes to the files go one at a time, in the order they were asked for
//! ([`NotesState::enqueue`]). Two saves of one note running at once could land
//! in either order and leave the older text on disk.

use std::collections::VecDeque;
use std::time::Instant;

use super_voice_shared::models::protocol::PreviewSource;

use crate::notes::Note;
use crate::state::TextBuffer;

/// Everything the Notes tab holds. `B` is the open note's editor buffer, the
/// app's toolkit's own (see [`TextBuffer`]).
#[derive(Debug, Default)]
pub struct NotesState<B = String> {
    /// The notes, newest first.
    pub items: Vec<Note>,
    /// Whether the first read has landed. Until it has, no notes means "not
    /// read yet", not "none".
    pub loaded: bool,
    /// The note shown in place of the list.
    pub open: Option<OpenNote<B>>,
    /// A take recording a note, or more of one.
    pub take: Option<NoteTake>,
    /// The words of a new note's take, while its file is made: the note that
    /// says them opens once it is, unless the user opened or closed one in
    /// the meantime.
    pub opening: Option<String>,
    /// The open note's Delete was pressed once and waits for the second.
    pub confirming_delete: bool,
    /// Where the open note was last exported to, until it is closed.
    pub exported: Option<String>,
    /// A write to the folder is running.
    pub busy: bool,
    /// The writes waiting for it, oldest first.
    pub queue: VecDeque<FileJob>,
}

/// One write to the notes folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileJob {
    /// Write `text` as the note `id`. `edits` is the open note's edit count
    /// the text holds; `0` for a note not open.
    Save {
        id: String,
        text: String,
        edits: u64,
    },
    Delete {
        id: String,
    },
}

impl FileJob {
    fn id(&self) -> &str {
        match self {
            Self::Save { id, .. } | Self::Delete { id } => id,
        }
    }
}

impl<B: TextBuffer> NotesState<B> {
    /// The note `id`, if the list has it.
    #[must_use]
    pub fn note(&self, id: &str) -> Option<&Note> {
        self.items.iter().find(|n| n.id == id)
    }

    /// Whether a take is running.
    #[must_use]
    pub const fn is_recording(&self) -> bool {
        self.take.is_some()
    }

    /// Put `note` in the list, in place of the one with its id, newest first.
    pub fn put(&mut self, note: Note) {
        self.items.retain(|n| n.id != note.id);
        self.items.push(note);
        crate::notes::sort_newest_first(&mut self.items);
    }

    /// Ask for `job`, and show its result in the list at once. Returns the job
    /// to start now when no write is running; otherwise it waits its turn.
    ///
    /// A save waiting for the same note is replaced, since only the newest
    /// text matters. A delete drops the saves of its note still waiting.
    pub fn enqueue(&mut self, job: FileJob) -> Option<FileJob> {
        match &job {
            FileJob::Save { id, text, .. } => {
                if let Some(note) = self.items.iter_mut().find(|n| &n.id == id) {
                    note.text.clone_from(text);
                }
                let waiting = self
                    .queue
                    .iter_mut()
                    .find(|j| matches!(j, FileJob::Save { .. }) && j.id() == id);
                if let Some(waiting) = waiting {
                    *waiting = job;
                    return None;
                }
            }
            FileJob::Delete { id } => {
                self.items.retain(|n| &n.id != id);
                self.queue
                    .retain(|j| !(matches!(j, FileJob::Save { .. }) && j.id() == id));
            }
        }
        if self.busy {
            self.queue.push_back(job);
            None
        } else {
            self.busy = true;
            Some(job)
        }
    }

    /// The running write is done. Returns the next to start, if any waits.
    pub fn next_job(&mut self) -> Option<FileJob> {
        let next = self.queue.pop_front();
        self.busy = next.is_some();
        next
    }

    /// Every write not yet made, oldest first, the open note's unsaved edits
    /// included: the app is closing, and makes them itself, now.
    pub fn drain_for_exit(&mut self) -> Vec<FileJob> {
        let mut jobs = Vec::new();
        if let Some(job) = self
            .open
            .as_ref()
            .filter(|o| o.unsaved())
            .map(OpenNote::save_job)
        {
            jobs.extend(self.enqueue(job));
        }
        if let Some(open) = self.open.as_mut() {
            open.saved = open.edits;
        }
        jobs.extend(self.queue.drain(..));
        self.busy = false;
        jobs
    }
}

/// The note open in place of the list, and how far its file is behind it.
#[derive(Debug)]
pub struct OpenNote<B = String> {
    pub id: String,
    /// The editor's buffer.
    pub content: B,
    /// Counts the edits, so a save knows which it wrote.
    pub edits: u64,
    /// The count of edits the file holds.
    pub saved: u64,
}

impl<B: TextBuffer> OpenNote<B> {
    #[must_use]
    pub fn new(note: &Note) -> Self {
        Self {
            id: note.id.clone(),
            content: B::from_text(&note.text),
            edits: 0,
            saved: 0,
        }
    }

    /// Whether the editor holds edits the file does not.
    #[must_use]
    pub const fn unsaved(&self) -> bool {
        self.saved < self.edits
    }

    /// A save of everything in the editor.
    #[must_use]
    pub fn save_job(&self) -> FileJob {
        FileJob::Save {
            id: self.id.clone(),
            text: self.content.text(),
            edits: self.edits,
        }
    }
}

/// What a take records into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TakeTarget {
    /// A note of its own.
    New,
    /// The end of the note with this id.
    Append(String),
}

/// A take recording into a note.
#[derive(Debug)]
pub struct NoteTake {
    pub target: TakeTarget,
    pub started: Instant,
    /// The newest preview of what was said.
    pub preview: String,
    /// What the preview spans: everything so far, or the last few seconds.
    pub preview_source: Option<PreviewSource>,
    /// Stop was pressed; the transcript is on its way.
    pub stopping: bool,
    /// The window's close button was pressed: the app closes once this
    /// take's words are saved.
    pub closing: bool,
}

impl NoteTake {
    #[must_use]
    pub fn new(target: TakeTarget) -> Self {
        Self {
            target,
            started: Instant::now(),
            preview: String::new(),
            preview_source: None,
            stopping: false,
            closing: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    type NotesState = super::NotesState<String>;
    type OpenNote = super::OpenNote<String>;

    fn note(id: &str, created: &str) -> Note {
        Note {
            id: id.to_string(),
            created: NaiveDateTime::parse_from_str(created, "%Y-%m-%d %H:%M").expect("a time"),
            text: id.to_string(),
        }
    }

    fn save(id: &str, text: &str) -> FileJob {
        FileJob::Save {
            id: id.to_string(),
            text: text.to_string(),
            edits: 0,
        }
    }

    #[test]
    fn a_note_put_in_the_list_replaces_its_old_self_and_keeps_the_order() {
        let mut state = NotesState::default();
        state.put(note("a", "2026-11-14 10:00"));
        state.put(note("c", "2026-11-18 10:00"));
        state.put(note("b", "2026-11-16 10:00"));
        let mut edited = note("b", "2026-11-16 10:00");
        edited.text = "edited".to_string();

        state.put(edited);

        let ids: Vec<&str> = state.items.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["c", "b", "a"]);
        assert_eq!(state.note("b").map(|n| n.text.as_str()), Some("edited"));
    }

    #[test]
    fn writes_run_one_at_a_time_in_the_order_asked() {
        let mut state = NotesState::default();

        assert_eq!(state.enqueue(save("a", "1")), Some(save("a", "1")));
        assert_eq!(state.enqueue(save("b", "1")), None);
        assert_eq!(state.enqueue(FileJob::Delete { id: "c".into() }), None);

        assert_eq!(state.next_job(), Some(save("b", "1")));
        assert_eq!(state.next_job(), Some(FileJob::Delete { id: "c".into() }));
        assert_eq!(state.next_job(), None);
        assert!(!state.busy);
        assert_eq!(state.enqueue(save("a", "2")), Some(save("a", "2")));
    }

    #[test]
    fn only_the_newest_waiting_save_of_a_note_is_written() {
        let mut state = NotesState::default();
        state.enqueue(save("a", "1"));
        state.enqueue(save("a", "2"));
        state.enqueue(save("b", "1"));
        state.enqueue(save("a", "3"));

        assert_eq!(state.next_job(), Some(save("a", "3")));
        assert_eq!(state.next_job(), Some(save("b", "1")));
        assert_eq!(state.next_job(), None);
    }

    #[test]
    fn a_delete_drops_the_saves_of_its_note_still_waiting() {
        let mut state = NotesState::default();
        state.put(note("a", "2026-11-14 10:00"));
        state.enqueue(save("b", "running"));
        state.enqueue(save("a", "too late"));

        state.enqueue(FileJob::Delete { id: "a".into() });

        assert!(state.note("a").is_none());
        assert_eq!(state.next_job(), Some(FileJob::Delete { id: "a".into() }));
        assert_eq!(state.next_job(), None);
    }

    #[test]
    fn the_list_shows_a_save_before_it_lands() {
        let mut state = NotesState::default();
        state.put(note("a", "2026-11-14 10:00"));
        state.enqueue(save("b", "running"));

        state.enqueue(save("a", "newest words"));

        assert_eq!(
            state.note("a").map(|n| n.text.as_str()),
            Some("newest words")
        );
    }

    #[test]
    fn closing_collects_every_write_not_made_with_the_open_notes_edits() {
        let mut state = NotesState::default();
        state.put(note("a", "2026-11-14 10:00"));
        state.enqueue(save("b", "running"));
        state.enqueue(save("c", "waiting"));
        let mut open = OpenNote::new(&note("a", "2026-11-14 10:00"));
        open.content = "typed just now".to_string();
        open.edits = 1;
        state.open = Some(open);

        let jobs = state.drain_for_exit();

        assert_eq!(
            jobs,
            [
                save("c", "waiting"),
                FileJob::Save {
                    id: "a".into(),
                    text: "typed just now".into(),
                    edits: 1,
                },
            ]
        );
        assert!(!state.busy);
        assert!(state.open.as_ref().is_some_and(|o| !o.unsaved()));
    }

    #[test]
    fn an_opened_note_has_nothing_unsaved() {
        let open = OpenNote::new(&note("a", "2026-11-14 10:00"));
        assert!(!open.unsaved());
        assert_eq!(open.content.text(), "a");
    }
}
