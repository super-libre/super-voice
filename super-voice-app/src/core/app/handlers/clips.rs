// SPDX-License-Identifier: GPL-3.0-only
//! The Clips tab, wired to its logic in the core
//! (`super_voice_app_core::pages::clips`). The app applies the draft editor's
//! own actions, names the voice a clip is made in, runs the work the logic
//! asks for as iced tasks, and says its notices in its own words. The export
//! dialog is the app's too.

use cosmic::prelude::*;
use super_voice_app_core::pages::clips::{self as page, Effect, Input, Notice, Outcome};

use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::ui::messages::{ClipsMessage, Message};

fn page_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Clips(ClipsMessage::Page(input)))
}

impl AppModel {
    pub(in crate::core::app) fn handle_clips_messages(
        &mut self,
        message: ClipsMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            ClipsMessage::Page(input) => self.clips_input(input),
            ClipsMessage::DraftEdit(action) => {
                self.clips.draft.0.perform(action);
                Task::none()
            }
            ClipsMessage::Make => {
                let voice = self.clip_voice_label();
                self.clips_input(Input::Make { voice })
            }
        }
    }

    /// Hand `input` to the page's logic, and do what it asks.
    fn clips_input(&mut self, input: Input) -> Task<cosmic::Action<Message>> {
        let Outcome {
            effects,
            clear_notice,
            notice,
        } = page::update(&mut self.clips, input);
        if clear_notice {
            self.clear_action_error(ErrorScope::Clips);
        }
        if let Some(notice) = notice {
            self.set_action_error(ErrorScope::Clips, notice_text(notice));
        }
        Task::batch(effects.into_iter().map(run))
    }

    /// The voice a clip made now is spoken in, as the user knows it: the
    /// voice text to speech uses, by its label. Kept with each clip, and shown
    /// on its row.
    fn clip_voice_label(&self) -> Option<String> {
        let source = self.synthesis.source.as_ref()?;
        let model = self.synthesis.model.as_ref()?;
        if self.voice.target.as_ref() != Some(&(source.clone(), model.clone())) {
            return None;
        }
        let id = self.voice.resolution.as_ref()?.effective.as_ref()?;
        Some(
            self.voice
                .choices
                .iter()
                .find(|v| &v.id == id)
                .map_or_else(|| id.clone(), |v| v.label.clone()),
        )
    }

    /// The Clips tab came on screen: read the folder, and the voice text to
    /// speech uses, for the card's picker.
    pub(in crate::core::app) fn clips_shown(&self) -> Task<cosmic::Action<Message>> {
        let reload = Task::done(page_action(Input::Reload));
        let voice = match (&self.synthesis.source, &self.synthesis.model) {
            (Some(source), Some(model)) => super::voice::load_model_voice(source, model.clone()),
            _ => Task::none(),
        };
        Task::batch([reload, voice])
    }
}

/// What the page asks for, as an iced task.
fn run(effect: Effect) -> Task<cosmic::Action<Message>> {
    match effect {
        Effect::List => Task::perform(page::run_list(), page_action),
        Effect::Make { text, voice, abort } => {
            // An aborted clip sends nothing: Cancel has already let it go.
            Task::future(page::run_make(text, voice, abort))
                .and_then(|input| Task::done(page_action(input)))
        }
        Effect::ReadSamples(id) => Task::perform(page::run_samples(id), page_action),
        Effect::Export { id, name } => {
            let labels = (crate::fl!("export-clip"), crate::fl!("wav-audio"));
            Task::perform(
                async move {
                    let result = export(&id, name, labels).await;
                    (id, result)
                },
                |(id, result)| page_action(Input::Exported { id, result }),
            )
        }
        Effect::Delete(id) => Task::perform(page::run_delete(id), page_action),
    }
}

/// What the page tells the user, in this app's words.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::CouldNotRead(e) => crate::fl!("clips-could-not-be-read", error = e),
        Notice::CouldNotMake(e) => crate::fl!("the-clip-could-not-be-made", error = e),
        Notice::CouldNotPlay(e) => crate::fl!("the-clip-could-not-play", error = e),
        Notice::CouldNotExport(e) => crate::fl!("the-clip-could-not-be-exported", error = e),
        Notice::CouldNotDelete(e) => crate::fl!("the-clip-could-not-be-deleted", error = e),
    }
}

/// Ask where to export the clip `id`, then copy it there. `None` when the
/// dialog was closed. `labels` are the dialog's title and its file type.
async fn export(
    id: &str,
    name: String,
    labels: (String, String),
) -> Result<Option<String>, String> {
    let (title, kind) = labels;
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title(title)
        .set_file_name(name)
        .add_filter(kind, &["wav"]);
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
    page::export_to(id, handle.path()).await
}
