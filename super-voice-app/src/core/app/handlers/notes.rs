// SPDX-License-Identifier: GPL-3.0-only
//! The Notes tab, wired to its logic in the core
//! (`super_voice_app_core::pages::notes`). The app applies the editor's own
//! actions, runs the work the logic asks for as iced tasks, and says its
//! notices in its own words. The export dialog is the app's too.

use cosmic::prelude::*;
use futures_util::StreamExt;
use super_voice_app_core::pages::notes::{self as page, Effect, Input, Notice, Outcome};

use crate::core::app::AppModel;
use crate::notes;
use crate::state::ErrorScope;
use crate::ui::messages::{Message, NotesMessage};

fn page_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Notes(NotesMessage::Page(input)))
}

impl AppModel {
    pub(in crate::core::app) fn handle_notes_messages(
        &mut self,
        message: NotesMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            NotesMessage::Page(input) => self.notes_input(input),
            NotesMessage::Edit(action) => {
                let Some(open) = self.notes.open.as_mut() else {
                    return Task::none();
                };
                let edit = action.is_edit();
                open.content.0.perform(action);
                if edit {
                    self.notes_input(Input::Edited)
                } else {
                    Task::none()
                }
            }
            // A tick changes nothing: the view reads the time itself.
            NotesMessage::Tick => Task::none(),
        }
    }

    /// Hand `input` to the page's logic, and do what it asks.
    pub(in crate::core::app) fn notes_input(
        &mut self,
        input: Input,
    ) -> Task<cosmic::Action<Message>> {
        if matches!(input, Input::Transcribed(_)) {
            // The take is over, so its level meter goes quiet.
            self.microphone.level = 0.0;
        }
        let Outcome {
            effects,
            clear_notice,
            notice,
        } = page::update(&mut self.notes, input);
        if clear_notice {
            self.clear_action_error(ErrorScope::Notes);
        }
        if let Some(notice) = notice {
            self.set_action_error(ErrorScope::Notes, notice_text(notice));
        }
        Task::batch(effects.into_iter().map(run))
    }

    /// Write every note not yet written, now, on this thread: the app is
    /// closing.
    pub(in crate::core::app) fn write_notes_now(&mut self) {
        page::write_now(self.notes.drain_for_exit(), None);
    }
}

/// What the page asks for, as an iced task.
fn run(effect: Effect) -> Task<cosmic::Action<Message>> {
    match effect {
        Effect::List => Task::perform(page::run_list(), page_action),
        Effect::Write(job) => Task::perform(page::run_write(job), page_action),
        Effect::Create(text) => Task::perform(page::run_create(text), page_action),
        Effect::StartTake => cosmic::task::stream(page::take().map(page_action)),
        Effect::StopTake => Task::perform(page::run_stop(), |()| cosmic::Action::None),
        Effect::SaveAfter(edits) => Task::perform(page::run_save_after(edits), page_action),
        Effect::Export { name, text } => {
            let labels = ExportLabels {
                title: crate::fl!("export-note"),
                kinds: crate::fl!("markdown-or-text"),
            };
            Task::perform(export(name, text, labels), |result| {
                page_action(Input::Exported(result))
            })
        }
        Effect::GiveUpLater => Task::perform(page::run_give_up(), page_action),
        Effect::WriteNow { jobs, new_note } => {
            page::write_now(jobs, new_note);
            Task::none()
        }
        Effect::Exit => cosmic::iced::exit(),
    }
}

/// What the page tells the user, in this app's words.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::CouldNotRead(e) => crate::fl!("notes-could-not-be-read", error = e),
        Notice::CouldNotSave(e) => crate::fl!("the-note-could-not-be-saved", error = e),
        Notice::CouldNotDelete(e) => crate::fl!("the-note-could-not-be-deleted", error = e),
        Notice::CouldNotExport(e) => crate::fl!("the-note-could-not-be-exported", error = e),
        Notice::NoSpeech => crate::fl!("no-speech-was-heard"),
        Notice::AnotherRecording => crate::fl!("another-recording-is-running"),
        Notice::RecordingFailed(e) => crate::fl!("the-recording-failed", error = e),
    }
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
