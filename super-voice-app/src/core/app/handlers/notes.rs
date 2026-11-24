// SPDX-License-Identifier: GPL-3.0-only
//! The Notes tab: reading and writing the notes folder, and the takes that
//! record into it.
//!
//! A take is the Behavior tab's test recording put to use: the daemon records
//! until Stop and sends the transcript back here instead of typing it. A new
//! note's file is made when its transcript arrives; more dictated into an open
//! note goes at its end, as a paragraph of its own.

use std::time::Duration;

use cosmic::prelude::*;
use cosmic::widget::text_editor;
use futures_util::StreamExt;

use crate::core::app::AppModel;
use crate::daemon::client::{RecordEvent, record_command_stream, stop_record_command};
use crate::notes;
use crate::state::ErrorScope;
use crate::state::notes::{FileJob, NoteTake, OpenNote, TakeTarget};
use crate::ui::messages::{Message, NotesMessage};

/// How long the open note's edits stand still before they are saved.
const SAVE_AFTER: Duration = Duration::from_millis(500);

/// How long the app, asked to close mid-take, waits for the take's transcript
/// before it closes anyway.
const EXIT_WAIT: Duration = Duration::from_secs(20);

fn notes_action(message: NotesMessage) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Notes(message))
}

impl AppModel {
    pub(in crate::core::app) fn handle_notes_messages(
        &mut self,
        message: NotesMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            folder @ (NotesMessage::Reload
            | NotesMessage::Loaded(_)
            | NotesMessage::Written { .. }) => self.handle_notes_folder(folder),

            take @ (NotesMessage::Record
            | NotesMessage::DictateMore
            | NotesMessage::Stop
            | NotesMessage::Preview { .. }
            | NotesMessage::Tick
            | NotesMessage::Transcribed(_)
            | NotesMessage::Created(_)) => self.handle_note_take(take),

            exit @ (NotesMessage::FinishBeforeExit
            | NotesMessage::ExitNow
            | NotesMessage::SaveBeforeExit) => self.handle_notes_exit(&exit),

            open @ (NotesMessage::Open(_)
            | NotesMessage::Close
            | NotesMessage::Edit(_)
            | NotesMessage::SaveDue(_)
            | NotesMessage::Export
            | NotesMessage::Exported(_)
            | NotesMessage::DeleteRequested
            | NotesMessage::DeleteCancelled
            | NotesMessage::DeleteConfirmed) => self.handle_open_note(open),
        }
    }

    /// Reading the folder, and the writes to it finishing.
    fn handle_notes_folder(&mut self, message: NotesMessage) -> Task<cosmic::Action<Message>> {
        match message {
            NotesMessage::Reload => Task::perform(
                async { notes::list(&notes::dir()).await.map_err(|e| e.to_string()) },
                |result| notes_action(NotesMessage::Loaded(result)),
            ),
            NotesMessage::Loaded(result) => {
                self.notes.loaded = true;
                match result {
                    Ok(read) => {
                        let crate::state::notes::NotesState { items, queue, .. } = &mut self.notes;
                        *items = read;
                        // A write still waiting holds newer text than the
                        // file just read.
                        for job in queue.iter() {
                            if let FileJob::Save { id, text, .. } = job
                                && let Some(note) = items.iter_mut().find(|n| &n.id == id)
                            {
                                note.text.clone_from(text);
                            }
                        }
                    }
                    Err(e) => self.set_action_error(
                        ErrorScope::Notes,
                        crate::fl!("notes-could-not-be-read", error = e),
                    ),
                }
                Task::none()
            }

            NotesMessage::Written { job, result } => {
                match (&job, result) {
                    (FileJob::Save { id, edits, .. }, Ok(())) => {
                        if let Some(open) = self.notes.open.as_mut()
                            && &open.id == id
                        {
                            open.saved = open.saved.max(*edits);
                        }
                    }
                    (FileJob::Save { .. }, Err(e)) => self.set_action_error(
                        ErrorScope::Notes,
                        crate::fl!("the-note-could-not-be-saved", error = e),
                    ),
                    (FileJob::Delete { .. }, Ok(())) => {}
                    (FileJob::Delete { .. }, Err(e)) => {
                        self.set_action_error(
                            ErrorScope::Notes,
                            crate::fl!("the-note-could-not-be-deleted", error = e),
                        );
                        // The list dropped it already; read back what is
                        // really there.
                        return Task::batch([self.next_file_job(), reload()]);
                    }
                }
                self.next_file_job()
            }
            _ => Task::none(),
        }
    }

    /// Recording a note, or more of one.
    fn handle_note_take(&mut self, message: NotesMessage) -> Task<cosmic::Action<Message>> {
        match message {
            NotesMessage::Record => self.start_note_take(TakeTarget::New),
            NotesMessage::DictateMore => match &self.notes.open {
                Some(open) => {
                    let id = open.id.clone();
                    self.start_note_take(TakeTarget::Append(id))
                }
                None => Task::none(),
            },
            NotesMessage::Stop => {
                let Some(take) = self.notes.take.as_mut() else {
                    return Task::none();
                };
                take.stopping = true;
                Task::perform(stop_record_command(), |result| {
                    if let Err(e) = result {
                        log::warn!("Stopping the note's recording failed: {e}");
                    }
                    cosmic::Action::None
                })
            }
            NotesMessage::Preview { text, source } => {
                if let Some(take) = self.notes.take.as_mut() {
                    take.preview = text;
                    take.preview_source = source;
                }
                Task::none()
            }
            NotesMessage::Transcribed(result) => self.note_take_finished(result),
            NotesMessage::Created(result) => {
                match result {
                    Ok(note) => self.notes.put(note),
                    Err(e) => self.set_action_error(
                        ErrorScope::Notes,
                        crate::fl!("the-note-could-not-be-saved", error = e),
                    ),
                }
                Task::none()
            }

            // A tick changes nothing: the view reads the time itself.
            _ => Task::none(),
        }
    }

    /// The open note: opening and closing it, editing and saving it,
    /// exporting and deleting it.
    fn handle_open_note(&mut self, message: NotesMessage) -> Task<cosmic::Action<Message>> {
        match message {
            NotesMessage::Open(id) => {
                let flush = self.save_open_note();
                let Some(note) = self.notes.note(&id) else {
                    return flush;
                };
                self.notes.open = Some(OpenNote::new(note));
                self.notes.confirming_delete = false;
                self.notes.exported = None;
                self.clear_action_error(ErrorScope::Notes);
                flush
            }
            NotesMessage::Close => {
                let flush = self.save_open_note();
                self.notes.open = None;
                self.notes.confirming_delete = false;
                self.notes.exported = None;
                flush
            }
            NotesMessage::Edit(action) => {
                let Some(open) = self.notes.open.as_mut() else {
                    return Task::none();
                };
                let edit = action.is_edit();
                open.content.0.perform(action);
                if !edit {
                    return Task::none();
                }
                open.edits += 1;
                self.notes.exported = None;
                let edits = open.edits;
                Task::perform(tokio::time::sleep(SAVE_AFTER), move |()| {
                    notes_action(NotesMessage::SaveDue(edits))
                })
            }
            NotesMessage::SaveDue(edits) => {
                let current = self.notes.open.as_ref().is_some_and(|o| o.edits == edits);
                if current {
                    self.save_open_note()
                } else {
                    Task::none()
                }
            }
            NotesMessage::Export => {
                // A retry starts clean: an earlier failure's banner, or where
                // the last export went, would describe the wrong attempt.
                self.clear_action_error(ErrorScope::Notes);
                self.notes.exported = None;
                let Some(open) = self.notes.open.as_ref() else {
                    return Task::none();
                };
                let name = self
                    .notes
                    .note(&open.id)
                    .map_or_else(|| format!("{}.md", open.id), notes::export_name);
                let text = open.content.0.text();
                let labels = ExportLabels {
                    title: crate::fl!("export-note"),
                    kinds: crate::fl!("markdown-or-text"),
                };
                Task::perform(export(name, text, labels), |result| {
                    notes_action(NotesMessage::Exported(result))
                })
            }
            NotesMessage::Exported(result) => {
                match result {
                    Ok(Some(path)) => self.notes.exported = Some(path),
                    Ok(None) => {}
                    Err(e) => self.set_action_error(
                        ErrorScope::Notes,
                        crate::fl!("the-note-could-not-be-exported", error = e),
                    ),
                }
                Task::none()
            }

            NotesMessage::DeleteRequested => {
                self.notes.confirming_delete = true;
                Task::none()
            }
            NotesMessage::DeleteCancelled => {
                self.notes.confirming_delete = false;
                Task::none()
            }
            NotesMessage::DeleteConfirmed => {
                self.notes.confirming_delete = false;
                let Some(open) = self.notes.open.take() else {
                    return Task::none();
                };
                self.notes.exported = None;
                self.write_note_file(FileJob::Delete { id: open.id })
            }
            _ => Task::none(),
        }
    }

    /// The app closing: everything unwritten is written first, and a take
    /// still recording is finished and saved when the app can wait for it.
    fn handle_notes_exit(&mut self, message: &NotesMessage) -> Task<cosmic::Action<Message>> {
        match message {
            NotesMessage::FinishBeforeExit => {
                let Some(take) = self.notes.take.as_mut() else {
                    return Task::none();
                };
                take.closing = true;
                let stop = if take.stopping {
                    Task::none()
                } else {
                    take.stopping = true;
                    Task::perform(stop_record_command(), |result| {
                        if let Err(e) = result {
                            log::warn!("Stopping the note's recording failed: {e}");
                        }
                        cosmic::Action::None
                    })
                };
                // A daemon that never answers must not keep the app open.
                let give_up = Task::perform(tokio::time::sleep(EXIT_WAIT), |()| {
                    notes_action(NotesMessage::ExitNow)
                });
                Task::batch([stop, give_up])
            }
            NotesMessage::ExitNow => {
                if self.notes.take.is_some() {
                    log::warn!(
                        "Closing before the note's transcript came; its words are not saved"
                    );
                }
                self.write_notes_now(Vec::new(), None);
                cosmic::iced::exit()
            }
            NotesMessage::SaveBeforeExit => {
                self.write_notes_now(Vec::new(), None);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// Make every write not yet made, on this thread, before the app exits:
    /// `first`, then the queue and the open note's edits, then a new note of
    /// `new_note`'s words. Background writes not yet started are dropped,
    /// since these are newer.
    pub(in crate::core::app) fn write_notes_now(
        &mut self,
        first: Vec<FileJob>,
        new_note: Option<String>,
    ) {
        let last = notes::last_writes(&notes::dir());
        let mut jobs = first;
        jobs.extend(self.notes.drain_for_exit());
        for job in jobs {
            let written = match &job {
                FileJob::Save { id, text, .. } => last.save(id, text),
                FileJob::Delete { id } => last.delete(id),
            };
            if let Err(e) = written {
                log::error!("Could not write {job:?} before closing: {e}");
            }
        }
        if let Some(text) = new_note
            && let Err(e) = last.create(&text, chrono::Local::now().naive_local())
        {
            log::error!("Could not save the note {text:?} before closing: {e}");
        }
    }

    /// Save what the open note's editor holds, when the file is behind it.
    /// Done before the note is closed, another opened, or the tab left.
    pub(in crate::core::app) fn save_open_note(&mut self) -> Task<cosmic::Action<Message>> {
        match self.notes.open.as_ref().filter(|o| o.unsaved()) {
            Some(open) => {
                let job = open.save_job();
                self.write_note_file(job)
            }
            None => Task::none(),
        }
    }

    /// Queue a write to the folder; start it if nothing else is writing.
    fn write_note_file(&mut self, job: FileJob) -> Task<cosmic::Action<Message>> {
        self.notes.enqueue(job).map_or_else(Task::none, run)
    }

    fn next_file_job(&mut self) -> Task<cosmic::Action<Message>> {
        self.notes.next_job().map_or_else(Task::none, run)
    }

    fn start_note_take(&mut self, target: TakeTarget) -> Task<cosmic::Action<Message>> {
        if self.notes.take.is_some() {
            return Task::none();
        }
        self.clear_action_error(ErrorScope::Notes);
        self.notes.exported = None;
        self.notes.take = Some(NoteTake::new(target));
        cosmic::task::stream(record_command_stream().map(|event| {
            notes_action(match event {
                RecordEvent::Preview { text, source } => NotesMessage::Preview { text, source },
                RecordEvent::Final(result) => NotesMessage::Transcribed(result),
            })
        }))
    }

    /// The take ended: make its note, or add it to the end of the one it was
    /// for. A note deleted while more was dictated into it comes back as a
    /// new one, so no words are lost. When the app is closing, the words are
    /// written at once and the app exits.
    fn note_take_finished(
        &mut self,
        result: Result<String, String>,
    ) -> Task<cosmic::Action<Message>> {
        let Some(take) = self.notes.take.take() else {
            return Task::none();
        };
        self.audio_level = 0.0;
        let said = match result {
            Ok(text) if text.trim().is_empty() => {
                self.set_action_error(ErrorScope::Notes, crate::fl!("no-speech-was-heard"));
                None
            }
            Ok(text) => Some(text.trim().to_string()),
            // Another take holds the microphone: the Behavior tab's test, or
            // the dictation shortcut.
            Err(e) if e.contains("recording_in_progress") => {
                self.set_action_error(
                    ErrorScope::Notes,
                    crate::fl!("another-recording-is-running"),
                );
                None
            }
            Err(e) => {
                self.set_action_error(
                    ErrorScope::Notes,
                    crate::fl!("the-recording-failed", error = e),
                );
                None
            }
        };
        let (start, new_note) = match said {
            None => (None, None),
            Some(said) => self.place_words(take.target, said),
        };
        if take.closing {
            self.write_notes_now(start.into_iter().collect(), new_note);
            return cosmic::iced::exit();
        }
        Task::batch([
            start.map_or_else(Task::none, run),
            new_note.map_or_else(Task::none, create),
        ])
    }

    /// Put a take's words where they go. Returns the write to start now, if
    /// one is not already waiting its turn, and the words of a new note to
    /// make.
    fn place_words(
        &mut self,
        target: TakeTarget,
        said: String,
    ) -> (Option<FileJob>, Option<String>) {
        let TakeTarget::Append(id) = target else {
            return (None, Some(said));
        };
        if let Some(open) = self.notes.open.as_mut().filter(|o| o.id == id) {
            let text = notes::append(&open.content.0.text(), &said);
            open.content = crate::state::Editor(text_editor::Content::with_text(&text));
            open.content
                .0
                .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
            open.edits += 1;
            let job = open.save_job();
            return (self.notes.enqueue(job), None);
        }
        match self.notes.note(&id) {
            Some(note) => {
                let text = notes::append(&note.text, &said);
                (
                    self.notes.enqueue(FileJob::Save { id, text, edits: 0 }),
                    None,
                )
            }
            None => (None, Some(said)),
        }
    }
}

fn reload() -> Task<cosmic::Action<Message>> {
    Task::done(notes_action(NotesMessage::Reload))
}

/// Make a note that says `text`, named for now.
fn create(text: String) -> Task<cosmic::Action<Message>> {
    Task::perform(
        async move {
            let now = chrono::Local::now().naive_local();
            notes::create(&notes::dir(), &text, now)
                .await
                .map_err(|e| e.to_string())
        },
        |result| notes_action(NotesMessage::Created(result)),
    )
}

/// Carry out one write to the folder.
fn run(job: FileJob) -> Task<cosmic::Action<Message>> {
    Task::perform(
        async move {
            let dir = notes::dir();
            let result = match &job {
                FileJob::Save { id, text, .. } => notes::save(&dir, id, text).await,
                FileJob::Delete { id } => notes::delete(&dir, id).await,
            };
            (job, result.map_err(|e| e.to_string()))
        },
        |(job, result)| notes_action(NotesMessage::Written { job, result }),
    )
}

/// What the export dialog says, read before it opens.
struct ExportLabels {
    title: String,
    /// The one file type offered. One, not Markdown and text apart: the
    /// dialog does not change the name's extension to follow the type
    /// picked, so a second choice would only mislead. The name's extension
    /// is what decides; a note is plain text either way.
    kinds: String,
}

/// Ask where to export a note, then write it there. `None` when the dialog
/// was closed.
async fn export(
    name: String,
    text: String,
    labels: ExportLabels,
) -> Result<Option<String>, String> {
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title(labels.title)
        .set_file_name(name)
        .add_filter(labels.kinds, &["md", "txt"]);
    // Left alone, the dialog opens wherever the app was started from.
    if let Some(start) = dirs::document_dir()
        .filter(|d| d.is_dir())
        .or_else(dirs::home_dir)
    {
        dialog = dialog.set_directory(start);
    }
    let Some(handle) = dialog.save_file().await else {
        return Ok(None);
    };
    let path = notes::export_path(handle.path());
    notes::export(&text, &path)
        .await
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}
