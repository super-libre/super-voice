// SPDX-License-Identifier: GPL-3.0-only
//! The Models page's stage cards, wired to their logic in the core
//! (`super_voice_app_core::pages::models`): the app runs the work the logic
//! asks for as iced tasks, closes the sheets a pick was made in, and says the
//! notices in its own words, most of them on the card they are about.

use cosmic::prelude::*;
use super_voice_app_core::pages::models::{self as page, Effect, Input, Notice, Outcome};

use crate::core::app::AppModel;
use crate::state::{DaemonStatus, ErrorScope, Stage};
use crate::ui::messages::{Message, ModelsMessage};

pub(in crate::core::app) fn models_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Models(ModelsMessage::Page(input)))
}

/// How long after a Load the app asks for the first download tick, rather
/// than waiting for the daemon to volunteer it.
const FIRST_TICK: std::time::Duration = std::time::Duration::from_millis(100);

impl AppModel {
    pub(in crate::core::app) fn handle_models_messages(
        &mut self,
        message: ModelsMessage,
    ) -> Task<cosmic::Action<Message>> {
        let ModelsMessage::Page(input) = message;
        self.models_input(input)
    }

    /// Hand `input` to the cards' logic, and do what it asks.
    pub(in crate::core::app) fn models_input(
        &mut self,
        input: Input,
    ) -> Task<cosmic::Action<Message>> {
        match &input {
            // The GPU poll asks only while the daemon answers.
            Input::RefreshGpus if self.connection.status != DaemonStatus::Connected => {
                return Task::none();
            }
            // A backend is picked in a sheet: it goes once the choice is made.
            // Emptying transcription closes its Configure sheet with it.
            Input::PickBackend(stage, source) => {
                self.core.window.show_context = false;
                if *stage == Stage::Transcription && source.is_none() {
                    self.configure.open = None;
                }
            }
            // A stage read, or a model picked, takes down the banner a failed
            // read left on its card.
            Input::ViewLoaded(stage, Ok(_)) | Input::PickModel(stage, _) => {
                if let Some(block) = stage.block() {
                    self.clear_action_error(ErrorScope::from(block));
                }
            }
            _ => {}
        }
        let Outcome {
            effects, notice, ..
        } = page::update(&mut self.models, input);
        if let Some(notice) = notice {
            self.models_notice(notice);
        }
        Task::batch(
            effects
                .into_iter()
                .map(|effect| self.run_models_effect(effect)),
        )
    }

    fn run_models_effect(&mut self, effect: Effect) -> Task<cosmic::Action<Message>> {
        match effect {
            Effect::ReadView(stage) => Task::perform(page::run_view(stage), models_action),
            Effect::ReadBackends(stage) => Task::perform(page::run_backends(stage), models_action),
            Effect::ReadSecrets(source) => Task::perform(page::run_secrets(source), models_action),
            Effect::ReadGpus => Task::perform(page::run_gpus(), models_action),
            Effect::ReadBackendDevices { stage, source } => {
                Task::perform(page::run_backend_devices(stage, source), models_action)
            }
            Effect::ReadDevices {
                stage,
                source,
                model,
            } => Task::perform(page::run_devices(stage, source, model), models_action),
            Effect::Poll(stage) => Task::perform(page::run_poll(stage), models_action),
            Effect::Focus {
                stage,
                source,
                model,
            } => {
                let voice = if stage == Stage::Synthesis {
                    super::voice::load_model_voice(&source, model.clone())
                } else {
                    Task::none()
                };
                Task::batch([self.load_model_language(stage, &source, model), voice])
            }
            Effect::VoicesChanged => Task::done(cosmic::Action::App(Message::Voices(
                crate::ui::messages::VoicesMessage::Page(
                    super_voice_app_core::pages::voices::Input::Refresh,
                ),
            ))),
            // A build already on disk installs without asking: that isn't a
            // download. Otherwise the dialog asks (`build_dialog`).
            Effect::OfferBuild {
                source, framework, ..
            } => {
                let on_disk = self
                    .registry
                    .build(&source, &framework)
                    .is_some_and(|build| build.on_disk);
                if on_disk {
                    self.models_input(Input::ConfirmBuild)
                } else {
                    Task::none()
                }
            }
            // Installed as the Library installs, so its progress shows on
            // both; its end comes back through the Library's logic.
            Effect::InstallBuild { source, framework } => {
                self.registry.install_errors.remove(&source);
                Task::perform(
                    page::run_install_build(source, framework),
                    super::models_page::library_action,
                )
            }
            load @ Effect::Load { stage, .. } => Task::batch([
                run_change(load),
                Task::perform(tokio::time::sleep(FIRST_TICK), move |()| {
                    models_action(Input::Changed(stage))
                }),
            ]),
            change => run_change(change),
        }
    }

    /// Say `notice`. A failure to change a stage shows on its card, where the
    /// user is looking; a stage that couldn't be read shows on its banner.
    fn models_notice(&mut self, notice: Notice) {
        log::warn!("Models page: {notice:?}");
        let (stage, message) = match notice {
            Notice::CouldNotRead(stage, error) => match stage.block() {
                Some(block) => {
                    let message = match block {
                        crate::state::BlockStage::PostProcessing => {
                            crate::fl!("post-processor-could-not-save", error = error)
                        }
                        crate::state::BlockStage::Synthesis => {
                            crate::fl!("speech-model-could-not-save", error = error)
                        }
                    };
                    self.set_action_error(ErrorScope::from(block), message);
                    return;
                }
                None => (
                    stage,
                    crate::fl!(
                        "stage-could-not-read",
                        stage = crate::ui::views::models::stage_name(stage),
                        error = error
                    ),
                ),
            },
            // The daemon's own words, as the card has always shown them.
            Notice::CouldNotChange(stage, error) => (stage, error),
            Notice::DeviceSwitchFailed(stage, error) => {
                (stage, crate::fl!("device-switch-failed", error = error))
            }
            Notice::BuildFailed {
                stage,
                framework,
                error,
            } => (
                stage,
                crate::fl!(
                    "build-download-failed",
                    framework = crate::ui::views::models::accel_name(&framework),
                    error = error
                ),
            ),
            Notice::Stalled(stage) => (stage, crate::fl!("model-load-stalled")),
        };
        page::show_failure(&mut self.models, stage, &message);
    }
}

/// A change the logic asked for, as an iced task: the stage is read again
/// when it is done.
fn run_change(effect: Effect) -> Task<cosmic::Action<Message>> {
    Task::perform(page::run_change(effect), |input| {
        input.map_or(cosmic::Action::None, models_action)
    })
}
