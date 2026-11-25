// SPDX-License-Identifier: GPL-3.0-only
//! The "Install manually" sheet, wired to its logic in the core
//! (`super_voice_app_core::pages::add_backend`): the app runs the work the
//! logic asks for as iced tasks, and keeps the native folder picker and the
//! drawer itself.

use cosmic::prelude::*;
use super_voice_app_core::pages::add_backend::{self as page, Effect, Input};

use crate::core::app::AppModel;
use crate::ui::messages::{Message, ModelsPageMessage};

fn add_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::ModelsPage(ModelsPageMessage::AddBackend(input)))
}

/// Open the folder picker. Shared by the drawer's Choose button and by Check
/// pressed with no folder chosen yet, which mean the same thing.
fn pick_folder_task() -> Task<cosmic::Action<Message>> {
    Task::perform(
        async {
            rfd::AsyncFileDialog::new()
                .set_title(crate::fl!("choose-backend-folder"))
                .pick_folder()
                .await
                .map(|h| h.path().to_string_lossy().into_owned())
        },
        |picked| add_action(Input::Picked(picked)),
    )
}

impl AppModel {
    /// The Add-a-backend drawer: pick a source, resolve it, commit to it.
    pub(in crate::core::app) fn add_backend_input(
        &mut self,
        input: Input,
    ) -> Task<cosmic::Action<Message>> {
        let effects = page::update(&mut self.registry, input);
        Task::batch(
            effects
                .into_iter()
                .map(|effect| self.run_add_backend_effect(effect)),
        )
    }

    fn run_add_backend_effect(&mut self, effect: Effect) -> Task<cosmic::Action<Message>> {
        match effect {
            Effect::PickFolder => pick_folder_task(),
            Effect::Preview(target) => Task::perform(page::run_preview(target), add_action),
            Effect::Install(target) => {
                Task::perform(page::run_install(target), super::library_action)
            }
            Effect::Close => {
                self.core.window.show_context = false;
                Task::none()
            }
        }
    }
}
