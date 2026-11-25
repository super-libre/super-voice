// SPDX-License-Identifier: GPL-3.0-only
//! The Library's catalog and its installs, wired to their logic in the core
//! (`super_voice_app_core::pages::library`): the app runs the work the logic
//! asks for as iced tasks, closes the menus that asked, and acts on an
//! install's end for the add sheet and the Models page's framework builds.

use cosmic::prelude::*;
use super_voice_app_core::pages::library::{self as page, Effect, Input};

use crate::core::app::AppModel;
use crate::ui::messages::{BackendMessage, Message, ModelsPageMessage};

pub(in crate::core::app) fn library_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::ModelsPage(ModelsPageMessage::Library(input)))
}

impl AppModel {
    pub(in crate::core::app) fn library_input(
        &mut self,
        input: Input,
    ) -> Task<cosmic::Action<Message>> {
        match input {
            // The menu that asked goes: a menu left over the list it acted
            // on reads as though the click missed. The dialog takes its place.
            Input::AskUninstall(_) => self.models_page.installed_menu_open = None,
            Input::Refresh { index: true } => self.models_page.browse_menu_open = false,
            _ => {}
        }
        let effects = page::update(&mut self.registry, &mut self.library, input);
        Task::batch(
            effects
                .into_iter()
                .map(|effect| self.run_library_effect(effect)),
        )
    }

    fn run_library_effect(&mut self, effect: Effect) -> Task<cosmic::Action<Message>> {
        match effect {
            Effect::FetchCatalog { refresh_index } => {
                Task::perform(page::run_catalog(refresh_index), library_action)
            }
            Effect::Install(source) => Task::perform(page::run_install(source), library_action),
            Effect::Update(source) => Task::perform(page::run_update(source), library_action),
            Effect::Uninstall(source) => Task::perform(page::run_uninstall(source), library_action),
            Effect::ReloadBackends => Task::done(cosmic::Action::App(Message::Backend(
                BackendMessage::BackendsReload,
            ))),
            Effect::ReloadCatalogs => crate::core::app::handlers::tasks::reload_backend_catalogs(),
            Effect::Installed { source } => {
                // The add sheet's whole job is done once what it previewed is
                // installed: close it and empty it. Only for the install it
                // started: a Browse card's reports on the same channel.
                if self.core.window.show_context
                    && self.context_page == crate::state::models::ContextPage::AddBackend
                    && self.registry.add_key() == source
                {
                    self.core.window.show_context = false;
                    self.registry.clear_add_sheet();
                }
                // A build the user asked for moves the model onto it.
                self.finish_build(&source)
            }
            Effect::InstallFailed { source, message } => {
                self.fail_build(&source, &message);
                Task::none()
            }
        }
    }
}
