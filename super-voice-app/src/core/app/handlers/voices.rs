// SPDX-License-Identifier: GPL-3.0-only
//! The Voices tab, wired to its logic in the core
//! (`super_voice_app_core::pages::voices`). The app names the model a preview
//! speaks in and the language a script is read in, runs the work the logic
//! asks for as iced tasks, and says its notices in its own words. The file
//! picker is the app's too.

use cosmic::prelude::*;
use super_voice_app_core::pages::voices::{self as page, Effect, Input, Notice, Outcome};

use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::ui::messages::{Message, VoicesMessage};

fn page_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Voices(VoicesMessage::Page(input)))
}

impl AppModel {
    pub(in crate::core::app) fn handle_voices_messages(
        &mut self,
        message: VoicesMessage,
    ) -> Task<cosmic::Action<Message>> {
        // The global Primary Language picks the script suggested: a clone is
        // best made from speech in the language it will be asked to speak.
        let language = self.language.primary_language.clone();
        let input = match message {
            VoicesMessage::Page(input) => input,
            // Addressed to the model the Models card shows, like every other
            // write there: hearing a voice selects it for that model, because
            // a speak request carries no voice of its own.
            VoicesMessage::Preview(voice_id) => Input::Preview {
                voice_id,
                model: self.voice.target.clone().map(|(_, model)| model),
            },
            VoicesMessage::RecordingTick => Input::RecordingTick { language },
            VoicesMessage::StopRecording => Input::StopRecording { language },
        };
        self.voices_input(input)
    }

    /// Hand `input` to the page's logic, and do what it asks.
    fn voices_input(&mut self, input: Input) -> Task<cosmic::Action<Message>> {
        let Outcome {
            effects,
            clear_notice,
            notice,
        } = page::update(&mut self.voices, input);
        if clear_notice {
            self.clear_action_error(ErrorScope::Voices);
        }
        if let Some(notice) = notice {
            let text = notice_text(notice);
            log::warn!("voices: {text}");
            self.set_action_error(ErrorScope::Voices, text);
        }
        let tasks: Vec<_> = effects.into_iter().map(|e| self.run_voices(e)).collect();
        Task::batch(tasks)
    }

    /// What the page asks for, as an iced task.
    fn run_voices(&self, effect: Effect) -> Task<cosmic::Action<Message>> {
        match effect {
            Effect::List => Task::perform(page::run_list(), page_action),
            Effect::Rename { id, label } => Task::perform(page::run_rename(id, label), page_action),
            Effect::Delete(id) => Task::perform(page::run_delete(id), page_action),
            Effect::Preview { model, voice_id } => {
                Task::perform(page::run_preview(model, voice_id), page_action)
            }
            Effect::PickFile => {
                let labels = (
                    crate::fl!("choose-a-voice-recording"),
                    crate::fl!("wav-audio"),
                );
                Task::perform(pick_wav(labels), |picked| {
                    page_action(match picked {
                        Ok(picked) => Input::FileImported(picked),
                        Err(e) => Input::ImportFailed(e),
                    })
                })
            }
            Effect::Save {
                label,
                transcript,
                wav,
            } => Task::perform(page::run_save(label, transcript, wav), page_action),
            // The Models card lists the voices a model can be pinned to, the
            // library's among them: a voice added, renamed or deleted here
            // changes that list.
            Effect::LibraryChanged => self.reload_model_voices(),
        }
    }

    /// Re-read the loaded model's voice list, because the library it draws on
    /// just changed.
    ///
    /// The Models card offers the voices a model can be pinned to, and that
    /// list is the daemon's join of the model's own presets with the cloned
    /// library. It was read when a model became selected, staged or loaded, and
    /// at no other time — so a voice recorded here did not appear there until
    /// the app was restarted, and a deleted one was still offered. The
    /// daemon's `settings_changed` for `voice` and `voices` calls it too, for a
    /// change another client made.
    ///
    /// Addressed to the pair the card is showing rather than to "the active
    /// model", for the same reason every other write on that card is: a reply
    /// that lands after a model switch must not be applied to the model the
    /// user moved to.
    #[allow(clippy::unused_self)]
    pub(in crate::core::app) fn reload_model_voices(&self) -> Task<cosmic::Action<Message>> {
        Task::done(super::voice::page_action(
            super_voice_app_core::pages::model_voice::Input::Reload,
        ))
    }
}

/// What the page tells the user, in this app's words. The daemon's and the
/// device's own messages are shown as they come.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::CouldNotLoad(e)
        | Notice::CouldNotImport(e)
        | Notice::CouldNotRename(e)
        | Notice::CouldNotDelete(e)
        | Notice::CouldNotPreview(e)
        | Notice::CouldNotRecord(e)
        | Notice::CouldNotSave(e) => e,
        Notice::NeedsTranscript => crate::fl!("this-voice-needs-a-transcript"),
    }
}

/// Pick a WAV file and read it. `labels` are the dialog's title and its file
/// type.
///
/// The filter names WAV alone because that is what the daemon stores: offering
/// an mp3 the upload would refuse is worse than not offering it.
async fn pick_wav(labels: (String, String)) -> Result<Option<(String, Vec<u8>)>, String> {
    let (title, kind) = labels;
    let Some(handle) = rfd::AsyncFileDialog::new()
        .set_title(title)
        .add_filter(kind, &["wav"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let name = handle.file_name();
    let bytes = tokio::fs::read(handle.path())
        .await
        .map_err(|e| format!("could not read {name}: {e}"))?;
    Ok(Some((name, bytes)))
}
