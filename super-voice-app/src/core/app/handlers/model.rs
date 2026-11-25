// SPDX-License-Identifier: GPL-3.0-only

use crate::core::app::{AppModel, ModelOperationState};
use crate::daemon::client::{get_gpu_info, get_stage_model, get_stage_view};
use crate::state::Stage;
use crate::ui::messages::{DeviceMessage, Message, ModelMessage, ModelsPageMessage, StageMessage};
use cosmic::prelude::*;
use log::info;
use log::warn;
use super_voice_app_core::rules::load::sanitize_home;

impl AppModel {
    /// Handle model management messages
    pub(in crate::core::app) fn handle_model_messages(
        &mut self,
        message: ModelMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            ModelMessage::LoadInitialData => self.handle_model_load_commands(),
            ModelMessage::CurrentModelLoaded { .. }
            | ModelMessage::IdleRead { .. }
            | ModelMessage::ModelChanged { .. }
            | ModelMessage::CurrentModelFetchFailed { .. } => self.handle_model_results(message),
        }
    }

    /// Handle the `LoadInitialData` startup load: models + device info.
    fn handle_model_load_commands(&mut self) -> Task<cosmic::Action<Message>> {
        info!("LoadInitialData: Loading models and device info at startup");
        // One-time startup load: models + device info
        Task::batch([
            self.fetch_current_model(),
            // The accelerator stage 1 is actually on, which is a property of
            // the model it is running rather than of the backend filling the
            // stage — so it comes off the model slot, not off `/stt/pipeline/1`.
            Task::perform(
                get_stage_model(Stage::Transcription),
                |result| match result {
                    Ok(slot) => {
                        let device = slot.running_device().map(ToString::to_string);
                        info!("Initial device load successful: device={device:?}");
                        cosmic::Action::App(Message::Device(DeviceMessage::DeviceInfoLoaded(
                            device,
                        )))
                    }
                    Err(e) => {
                        warn!("Initial device load failed: {e}");
                        cosmic::Action::App(Message::Device(DeviceMessage::DeviceError(
                            e.to_string(),
                        )))
                    }
                },
            ),
            crate::core::app::handlers::tasks::reload_backends(),
            // Backend *and* the model it remembers, in one read: a stage keeps
            // its selection through an unload, so the card comes back offering
            // that model instead of empty.
            Task::perform(get_stage_view(Stage::Transcription), |result| {
                let stage = result.unwrap_or_default();
                cosmic::Action::App(Message::Stage(StageMessage::BackendSelected {
                    stage: Stage::Transcription,
                    source: stage.source,
                    model: stage.model,
                }))
            }),
            Task::perform(get_gpu_info(), |result| {
                cosmic::Action::App(Message::ModelsPage(ModelsPageMessage::GpuInfoLoaded(
                    result.unwrap_or_default(),
                )))
            }),
        ])
    }

    /// Handle model result messages: loads, changes, and errors.
    fn handle_model_results(&mut self, message: ModelMessage) -> Task<cosmic::Action<Message>> {
        match message {
            ModelMessage::CurrentModelLoaded {
                model,
                source,
                idle,
                epoch,
            } => {
                // Discard a stale snapshot: a live `model_switched` advanced the
                // epoch after this fetch was issued, so it is the fresher truth
                // and this point-in-time read must not overwrite it.
                if epoch != self.current_model_epoch {
                    return Task::none();
                }
                self.current_model.clone_from(&model);
                self.current_source.clone_from(&source);
                self.transcription_idle = idle;
                self.model_operations.set_ready(Stage::Transcription);
                // Fetch the per-model language block now that a model is loaded.
                // Wire point 1: model loaded (CurrentModelLoaded).
                self.load_model_language(Stage::Transcription, &source, model)
            }

            ModelMessage::IdleRead { idle, epoch } => {
                if epoch == self.current_model_epoch {
                    self.transcription_idle = idle;
                }
                Task::none()
            }

            ModelMessage::ModelChanged { model, source } => {
                // Authoritative result of a user-initiated switch — bump the
                // epoch so any in-flight reconnect snapshot is discarded rather
                // than reverting this.
                self.current_model_epoch = self.current_model_epoch.wrapping_add(1);
                self.current_model.clone_from(&model);
                self.current_source.clone_from(&source);
                self.transcription_idle = None;
                self.model_operations.set_ready(Stage::Transcription);
                // Fetch the per-model language block now that a model is loaded.
                // Wire point 1: model loaded (ModelChanged).
                self.load_model_language(Stage::Transcription, &source, model)
            }

            ModelMessage::CurrentModelFetchFailed { epoch, error } => {
                // A stale snapshot fetch that failed must not clobber live state:
                // if a `model_switched` advanced the epoch after this fetch was
                // issued, the fresher truth already arrived — log and keep it,
                // never `clear_loaded_model()` on a superseded fetch error
                // (audit 2 Tier 1 #8).
                if epoch != self.current_model_epoch {
                    info!(
                        "Ignoring stale get_current_model failure (epoch {epoch} != {}); \
                         keeping live model state: {error}",
                        self.current_model_epoch
                    );
                    return Task::none();
                }
                self.set_model_error(&error);
                Task::none()
            }

            // Startup load is driven by the sibling `handle_model_load_commands`
            // arm; nothing to do here.
            ModelMessage::LoadInitialData => Task::none(),
        }
    }

    /// Surface a failed model operation on the Models card and drop the loaded
    /// model, sanitizing any `$HOME` path out of the message. Shared by the
    /// optimistic-rollback handlers (audit Tier 3 #37).
    pub(in crate::core::app) fn set_model_error(&mut self, err: &str) {
        warn!("Model operation failed: {err}");
        let home = std::env::var("HOME").unwrap_or_default();
        let sanitized = sanitize_home(err, &home);
        self.model_operations.set(
            Stage::Transcription,
            ModelOperationState::Error { message: sanitized },
        );
        // A failed switch leaves stage 1 idle (no model) — the backend stays
        // selected, but no model is loaded.
        self.clear_loaded_model();
    }

    /// Snapshot the daemon's current model, tagging the result with the
    /// `current_model_epoch` captured now. The `CurrentModelLoaded` handler
    /// applies it only if the epoch is still current — so a slow query that
    /// resolves after a live `model_switched` is discarded instead of reverting
    /// the model identity. Used at initial load and on every event-stream
    /// (re)subscribe to resync robustly against reconnect/restart ordering.
    pub(in crate::core::app) fn fetch_current_model(&self) -> Task<cosmic::Action<Message>> {
        let epoch = self.current_model_epoch;
        Task::perform(
            get_stage_view(Stage::Transcription),
            move |result| match result {
                Ok(stage) => {
                    // An idle daemon is a valid state, not an error: report an
                    // empty selection rather than a backend with no model, which is
                    // what the identity handler reads as "nothing loaded".
                    //
                    // The *loaded* model, not the selection: a stage remembers what
                    // it was pointed at through an unload, and reporting that as
                    // the current model would tell the Recording page a model is up
                    // when none is.
                    let idle = idle_selection(&stage);
                    let (model, source) = match stage.model.filter(|_| stage.loaded) {
                        Some(model) => (model, stage.source.unwrap_or_default()),
                        None => (String::new(), String::new()),
                    };
                    cosmic::Action::App(Message::Model(ModelMessage::CurrentModelLoaded {
                        model,
                        source,
                        idle,
                        epoch,
                    }))
                }
                Err(e) => {
                    cosmic::Action::App(Message::Model(ModelMessage::CurrentModelFetchFailed {
                        epoch,
                        error: e.to_string(),
                    }))
                }
            },
        )
    }
}

impl AppModel {
    /// Whether stage 1's model is idle, from the `ready` that says it left
    /// memory: `was` is the model it had, when the app knew it. An idle model
    /// is the one that was loaded, so only one the app did not know (a load
    /// that failed, say) is read back.
    pub(in crate::core::app) fn take_transcription_idle(
        &mut self,
        state: crate::daemon::client::ModelState,
        was: Option<(String, String)>,
    ) -> Option<Task<cosmic::Action<Message>>> {
        match idle_after_ready(state, was) {
            IdleAfterReady::Known(idle) => {
                self.transcription_idle = idle;
                None
            }
            IdleAfterReady::Unknown => Some(self.read_transcription_idle()),
        }
    }

    /// Re-read whether the daemon reports stage 1's model idle: when idle
    /// unload is turned on or off, and when a `ready` says it is idle without
    /// the app knowing which model.
    ///
    /// Tagged with the epoch, as [`Self::fetch_current_model`] is, so a load
    /// that lands first wins over this read.
    pub(in crate::core::app) fn read_transcription_idle(&self) -> Task<cosmic::Action<Message>> {
        let epoch = self.current_model_epoch;
        Task::perform(
            get_stage_view(Stage::Transcription),
            move |result| match result {
                Ok(stage) => cosmic::Action::App(Message::Model(ModelMessage::IdleRead {
                    idle: idle_selection(&stage),
                    epoch,
                })),
                Err(e) => {
                    warn!("Could not read whether stage 1's model is idle: {e}");
                    cosmic::Action::None
                }
            },
        )
    }
}

/// Stage 1's idle model after a `ready` saying its model left memory.
#[derive(Debug, PartialEq, Eq)]
enum IdleAfterReady {
    /// The idle model, or `None` when the model is not idle.
    Known(Option<(String, String)>),
    /// Idle, but the app did not know which model was loaded: read it back.
    Unknown,
}

/// [`AppModel::take_transcription_idle`]'s rule: an idle model is the one
/// that was loaded, `was`, when the app knew it.
fn idle_after_ready(
    state: crate::daemon::client::ModelState,
    was: Option<(String, String)>,
) -> IdleAfterReady {
    if state != crate::daemon::client::ModelState::Idle {
        return IdleAfterReady::Known(None);
    }
    was.map_or(IdleAfterReady::Unknown, |model| {
        IdleAfterReady::Known(Some(model))
    })
}

/// Stage 1's `(model, source)` when the daemon reports that model idle.
fn idle_selection(stage: &crate::daemon::client::StageState) -> Option<(String, String)> {
    if stage.state != crate::daemon::client::ModelState::Idle {
        return None;
    }
    Some((stage.model.clone()?, stage.source.clone()?))
}

#[cfg(test)]
mod tests {
    use super::{IdleAfterReady, idle_after_ready};
    use crate::daemon::client::ModelState;

    fn whisper() -> Option<(String, String)> {
        Some((
            "whisper-tiny".to_string(),
            "github.com/x/whisper".to_string(),
        ))
    }

    /// An idle unload leaves idle the model that was loaded, which the app
    /// knew, so nothing is read back.
    #[test]
    fn an_idle_unload_keeps_the_model_that_was_loaded() {
        assert_eq!(
            idle_after_ready(ModelState::Idle, whisper()),
            IdleAfterReady::Known(whisper())
        );
    }

    /// An unload leaves nothing idle, whatever was loaded.
    #[test]
    fn an_unload_leaves_nothing_idle() {
        assert_eq!(
            idle_after_ready(ModelState::NotLoaded, whisper()),
            IdleAfterReady::Known(None)
        );
    }

    /// Idle with no model the app knew of, as after a load on demand that
    /// failed: the stage is read back for which.
    #[test]
    fn an_idle_model_the_app_did_not_know_is_read_back() {
        assert_eq!(
            idle_after_ready(ModelState::Idle, None),
            IdleAfterReady::Unknown
        );
    }
}
