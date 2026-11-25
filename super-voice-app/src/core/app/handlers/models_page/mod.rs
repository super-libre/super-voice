// SPDX-License-Identifier: GPL-3.0-only

mod install;
mod registry;

pub(in crate::core::app) use install::library_action;

use crate::core::app::AppModel;
use crate::daemon::client::get_gpu_info;
use crate::state::{ContextPage, DaemonStatus, ModelsTab};
use crate::ui::messages::{Message, ModelsPageMessage};
use cosmic::prelude::*;
use super_voice_app_core::pages::configure::Input;

impl AppModel {
    /// Models-page UI: tab switch, per-backend dropdown / GPU / select, the
    /// configuration sub-view, and the (UI-only) download actions.
    pub(in crate::core::app) fn handle_models_page_messages(
        &mut self,
        message: ModelsPageMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            ModelsPageMessage::ModelsTabActivated(entity) => self.activate_models_tab(entity),

            ModelsPageMessage::SelectPipelineStep(stage) => {
                self.models_page.pipeline_step = stage;
                Task::none()
            }

            ModelsPageMessage::OpenBackendConfig(_)
            | ModelsPageMessage::CloseBackendConfig
            | ModelsPageMessage::RefreshGpuInfo
            | ModelsPageMessage::GpuInfoLoaded(_)
            | ModelsPageMessage::ToggleInstalledMenu(_)
            | ModelsPageMessage::CloseInstalledMenu => self.handle_models_backend_config(message),

            ModelsPageMessage::Library(input) => self.library_input(input),

            ModelsPageMessage::AddBackend(input) => self.add_backend_input(input),

            ModelsPageMessage::ToggleBrowseMenu => {
                self.models_page.browse_menu_open = !self.models_page.browse_menu_open;
                Task::none()
            }

            ModelsPageMessage::CloseBrowseMenu => {
                self.models_page.browse_menu_open = false;
                Task::none()
            }
        }
    }

    fn activate_models_tab(
        &mut self,
        entity: cosmic::widget::segmented_button::Entity,
    ) -> Task<cosmic::Action<Message>> {
        self.models_page.models_tabs.activate(entity);
        // Trigger initial registry fetch when the Download tab is opened for
        // the first time (backends empty and no prior refresh attempt).
        let switched_to_download = self
            .models_page
            .models_tabs
            .data::<ModelsTab>(entity)
            .is_some_and(|t| *t == ModelsTab::Download);
        if switched_to_download
            && self.registry.backends.is_empty()
            && self.registry.last_refresh.is_none()
        {
            return crate::core::app::handlers::tasks::fetch_registry_catalog(false);
        }
        Task::none()
    }

    fn handle_models_backend_config(
        &mut self,
        message: ModelsPageMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            ModelsPageMessage::OpenBackendConfig(source) => {
                // Open the per-backend configuration as a right-side sheet over
                // the current list (active card or Installed tab), instead of a
                // full-page takeover. Also closes the card's overflow menu.
                self.context_page = ContextPage::ConfigureBackend;
                self.core.window.show_context = true;
                self.models_page.installed_menu_open = None;
                self.configure_input(Input::Open(source))
            }

            ModelsPageMessage::CloseBackendConfig => {
                self.core.window.show_context = false;
                self.configure_input(Input::Close)
            }

            ModelsPageMessage::RefreshGpuInfo => {
                // Periodic poll — only query when connected so the disconnected
                // state doesn't spam failing requests.
                if self.connection.status == DaemonStatus::Connected {
                    Task::perform(get_gpu_info(), |result| {
                        cosmic::Action::App(Message::ModelsPage(ModelsPageMessage::GpuInfoLoaded(
                            result.unwrap_or_default(),
                        )))
                    })
                } else {
                    Task::none()
                }
            }

            ModelsPageMessage::GpuInfoLoaded(gpus) => {
                // Every GPU poll lands here; see `get_gpu_info`.
                log::trace!("GpuInfoLoaded: storing {} GPU(s) in app state", gpus.len());
                self.gpu_info = gpus;
                Task::none()
            }

            ModelsPageMessage::ToggleInstalledMenu(source) => {
                // Toggle this card's overflow menu; opening one closes any other.
                if self.models_page.installed_menu_open.as_deref() == Some(source.as_str()) {
                    self.models_page.installed_menu_open = None;
                } else {
                    self.models_page.installed_menu_open = Some(source);
                }
                Task::none()
            }

            ModelsPageMessage::CloseInstalledMenu => {
                self.models_page.installed_menu_open = None;
                Task::none()
            }

            _ => Task::none(),
        }
    }
}
