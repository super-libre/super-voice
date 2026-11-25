// SPDX-License-Identifier: GPL-3.0-only
//! Self-update: status load/check, the two settings toggles, and the apply
//! flow (installer download + spawn + JSON progress stream).
//!
//! Wired to its logic in the core (`super_voice_app_core::pages::updates`):
//! this module runs the work the logic asks for as iced tasks, and owns the
//! two actions that are this app's own, opening the page from the header's
//! badge and relaunching after an update.

use crate::core::app::AppModel;
use crate::ui::messages::{Message, UpdateMessage};
use cosmic::Application as _; // `on_nav_select`, reused by the header badge
use cosmic::prelude::*;
use futures_util::StreamExt;
use super_voice_app_core::pages::updates::{self as page, Effect, Input};

pub(in crate::core::app) fn page_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Update(UpdateMessage::Page(input)))
}

impl AppModel {
    pub(in crate::core::app) fn handle_update_messages(
        &mut self,
        message: UpdateMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            UpdateMessage::Page(input) => {
                let effects = page::update(&mut self.update, input);
                Task::batch(
                    effects
                        .into_iter()
                        .map(|effect| self.run_update_effect(effect)),
                )
            }
            UpdateMessage::OpenUpdatesPage => self.open_updates_page(),
            UpdateMessage::RestartApp => {
                // Only exit once the relaunch has actually been spawned — if
                // it fails, the user is left with no running app at all, so
                // surface the failure and let them retry (or start it
                // manually) instead.
                match std::process::Command::new("/usr/local/bin/super-voice-app").spawn() {
                    Ok(_) => std::process::exit(0),
                    Err(e) => {
                        log::warn!("failed to relaunch after update: {e}");
                        if let Some(run) = self.update.run.as_mut() {
                            run.error = Some(crate::fl!("relaunch-failed"));
                        }
                        Task::none()
                    }
                }
            }
        }
    }

    /// What the page asks for, as an iced task.
    fn run_update_effect(&mut self, effect: Effect) -> Task<cosmic::Action<Message>> {
        match effect {
            Effect::ReadStatus => Task::perform(page::run_status(), page_action),
            Effect::Check => Task::perform(page::run_check(), page_action),
            Effect::SaveAutoCheck(enabled) => {
                Task::perform(page::run_save_auto_check(enabled), page_action)
            }
            Effect::SaveBetaOptin(enabled) => {
                Task::perform(page::run_save_beta_optin(enabled), page_action)
            }
            // Dropping the stream kills the installer, so the run is aborted
            // through its handle, which the state keeps.
            Effect::StartRun { asset, tag } => {
                let (run, handle) = futures_util::stream::abortable(
                    super_voice_app_core::updater::run_update_stream(asset, tag),
                );
                self.update.run_abort = Some(handle);
                cosmic::task::stream(run.map(|event| page_action(Input::RunEvent(event))))
            }
        }
    }

    /// Navigate to the Updates page from the header bar's badge.
    ///
    /// Through `on_nav_select` rather than a bare `activate` so the badge
    /// lands on the page in exactly the state the sidebar entry leaves it in
    /// — status refetched, any open context sheet closed.
    fn open_updates_page(&mut self) -> Task<cosmic::Action<Message>> {
        let entity = self.nav.iter().find(|&id| {
            matches!(
                self.nav.data::<crate::state::Section>(id),
                Some(crate::state::Section::Updates)
            )
        });
        entity.map_or_else(Task::none, |id| self.on_nav_select(id))
    }
}
