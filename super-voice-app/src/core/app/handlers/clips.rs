// SPDX-License-Identifier: GPL-3.0-only
//! The Clips tab: making a clip from text through the daemon, keeping it in
//! the clips folder, and playing, exporting and deleting what is kept.

use std::time::Instant;

use cosmic::prelude::*;

use crate::audio::player::Player;
use crate::clips;
use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::state::clips::{Making, Playing};
use crate::ui::messages::{ClipsMessage, Message};

fn clips_action(message: ClipsMessage) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Clips(message))
}

impl AppModel {
    pub(in crate::core::app) fn handle_clips_messages(
        &mut self,
        message: ClipsMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            folder @ (ClipsMessage::Reload | ClipsMessage::Loaded(_)) => {
                self.handle_clips_folder(folder)
            }
            make @ (ClipsMessage::DraftEdit(_)
            | ClipsMessage::Make
            | ClipsMessage::Made(_)
            | ClipsMessage::Cancel
            | ClipsMessage::Tick) => self.handle_clip_making(make),
            play @ (ClipsMessage::Play(_) | ClipsMessage::Ready { .. } | ClipsMessage::Stop) => {
                self.handle_clip_playing(play)
            }
            kept @ (ClipsMessage::Export(_)
            | ClipsMessage::Exported { .. }
            | ClipsMessage::DeleteRequested(_)
            | ClipsMessage::DeleteCancelled
            | ClipsMessage::DeleteConfirmed(_)
            | ClipsMessage::Deleted { .. }) => self.handle_kept_clip(kept),
        }
    }

    /// Reading the folder.
    fn handle_clips_folder(&mut self, message: ClipsMessage) -> Task<cosmic::Action<Message>> {
        match message {
            ClipsMessage::Reload => Task::perform(
                async { clips::list(&clips::dir()).await.map_err(|e| e.to_string()) },
                |result| clips_action(ClipsMessage::Loaded(result)),
            ),
            ClipsMessage::Loaded(result) => {
                self.clips.loaded = true;
                match result {
                    Ok(items) => self.clips.items = items,
                    Err(e) => self.set_action_error(
                        ErrorScope::Clips,
                        crate::fl!("clips-could-not-be-read", error = e),
                    ),
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// The next clip: its text, making it, and stopping that.
    fn handle_clip_making(&mut self, message: ClipsMessage) -> Task<cosmic::Action<Message>> {
        match message {
            ClipsMessage::DraftEdit(action) => {
                self.clips.draft.0.perform(action);
                Task::none()
            }
            ClipsMessage::Make => {
                let text = self.clips.draft.0.text().trim().to_string();
                if text.is_empty() || self.clips.making.is_some() {
                    return Task::none();
                }
                self.clear_action_error(ErrorScope::Clips);
                self.clips.exported = None;
                let voice = self.clip_voice_label();
                let (make, handle) = futures_util::future::abortable(make(text, voice));
                // An aborted clip sends nothing: Cancel has already let it go.
                let task = Task::future(async move { make.await.ok() })
                    .and_then(|result| Task::done(clips_action(ClipsMessage::Made(result))));
                self.clips.making = Some(Making {
                    started: Instant::now(),
                    handle,
                });
                task
            }
            ClipsMessage::Made(result) => {
                self.clips.making = None;
                match result {
                    Ok(clip) => {
                        self.clips.put(clip);
                        self.clips.draft = crate::state::Editor::default();
                    }
                    Err(e) => self.set_action_error(
                        ErrorScope::Clips,
                        crate::fl!("the-clip-could-not-be-made", error = e),
                    ),
                }
                Task::none()
            }
            ClipsMessage::Cancel => {
                if let Some(making) = self.clips.making.take() {
                    making.handle.abort();
                }
                Task::none()
            }
            ClipsMessage::Tick => {
                // A clip that has played out is let go; one that could not
                // play says why.
                let ended = self
                    .clips
                    .playing
                    .as_ref()
                    .and_then(|p| p.player.as_ref())
                    .filter(|player| player.finished())
                    .map(Player::error);
                if let Some(error) = ended {
                    self.clips.playing = None;
                    if let Some(e) = error {
                        self.set_action_error(
                            ErrorScope::Clips,
                            crate::fl!("the-clip-could-not-play", error = e),
                        );
                    }
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// Playing a clip, and stopping it.
    fn handle_clip_playing(&mut self, message: ClipsMessage) -> Task<cosmic::Action<Message>> {
        match message {
            ClipsMessage::Play(id) => {
                self.clear_action_error(ErrorScope::Clips);
                // One at a time: the one playing stops when it is let go.
                self.clips.playing = Some(Playing {
                    id: id.clone(),
                    player: None,
                });
                Task::perform(
                    async move {
                        let result = clips::samples(&clips::dir(), &id)
                            .await
                            .map_err(|e| e.to_string());
                        (id, result)
                    },
                    |(id, result)| clips_action(ClipsMessage::Ready { id, result }),
                )
            }
            ClipsMessage::Ready { id, result } => {
                // Stopped, or another clip asked for, while this one was read.
                let Some(playing) = self.clips.playing.as_mut().filter(|p| p.id == id) else {
                    return Task::none();
                };
                match result {
                    Ok((samples, rate)) => playing.player = Some(Player::start(samples, rate)),
                    Err(e) => {
                        self.clips.playing = None;
                        self.set_action_error(
                            ErrorScope::Clips,
                            crate::fl!("the-clip-could-not-play", error = e),
                        );
                    }
                }
                Task::none()
            }
            ClipsMessage::Stop => {
                self.clips.playing = None;
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// Exporting and deleting a kept clip.
    fn handle_kept_clip(&mut self, message: ClipsMessage) -> Task<cosmic::Action<Message>> {
        match message {
            ClipsMessage::Export(id) => {
                // A retry starts clean, as the Notes tab's does.
                self.clear_action_error(ErrorScope::Clips);
                self.clips.exported = None;
                let Some(clip) = self.clips.clip(&id) else {
                    return Task::none();
                };
                let name = clips::export_name(clip);
                let labels = (crate::fl!("export-clip"), crate::fl!("wav-audio"));
                Task::perform(
                    async move {
                        let result = export(&id, name, labels).await;
                        (id, result)
                    },
                    |(id, result)| clips_action(ClipsMessage::Exported { id, result }),
                )
            }
            ClipsMessage::Exported { id, result } => {
                match result {
                    Ok(Some(path)) => self.clips.exported = Some((id, path)),
                    Ok(None) => {}
                    Err(e) => self.set_action_error(
                        ErrorScope::Clips,
                        crate::fl!("the-clip-could-not-be-exported", error = e),
                    ),
                }
                Task::none()
            }
            ClipsMessage::DeleteRequested(id) => {
                self.clips.confirming_delete = Some(id);
                Task::none()
            }
            ClipsMessage::DeleteCancelled => {
                self.clips.confirming_delete = None;
                Task::none()
            }
            ClipsMessage::DeleteConfirmed(id) => {
                self.clips.confirming_delete = None;
                if self.clips.is_playing(&id) {
                    self.clips.playing = None;
                }
                if self.clips.exported.as_ref().is_some_and(|(e, _)| *e == id) {
                    self.clips.exported = None;
                }
                self.clips.items.retain(|c| c.id != id);
                Task::perform(
                    async move {
                        let result = clips::delete(&clips::dir(), &id)
                            .await
                            .map_err(|e| e.to_string());
                        (id, result)
                    },
                    |(id, result)| clips_action(ClipsMessage::Deleted { id, result }),
                )
            }
            ClipsMessage::Deleted { id, result } => match result {
                Ok(()) => Task::none(),
                Err(e) => {
                    log::warn!("Could not delete the clip {id}: {e}");
                    self.set_action_error(
                        ErrorScope::Clips,
                        crate::fl!("the-clip-could-not-be-deleted", error = e),
                    );
                    // The list dropped it already; read back what is there.
                    Task::done(clips_action(ClipsMessage::Reload))
                }
            },
            _ => Task::none(),
        }
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
        let reload = Task::done(clips_action(ClipsMessage::Reload));
        let voice = match (&self.synthesis.source, &self.synthesis.model) {
            (Some(source), Some(model)) => super::voice::load_model_voice(source, model.clone()),
            _ => Task::none(),
        };
        Task::batch([reload, voice])
    }
}

/// Have the daemon speak `text`, and keep the audio as a clip made now.
async fn make(text: String, voice: Option<String>) -> Result<clips::Clip, String> {
    let wav = crate::daemon::client::v1::synthesize::synthesize(text.clone())
        .await
        .map_err(|e| e.to_string())?;
    let now = chrono::Local::now().naive_local();
    clips::create(&clips::dir(), wav, text, voice, now)
        .await
        .map_err(|e| e.to_string())
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
    let path = clips::export_path(handle.path());
    clips::export(&clips::dir(), id, &path)
        .await
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}
