// SPDX-License-Identifier: GPL-3.0-only

use crate::core::app::AppModel;
use crate::ui::messages::{Message, ShellMessage};
use crate::ui::views;
use cosmic::prelude::*;

impl AppModel {
    /// Handle template/shell messages: URL opening, context page toggles, and URL launching.
    pub(in crate::core::app) fn handle_shell_messages(
        &mut self,
        message: ShellMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            ShellMessage::OpenRepositoryUrl => {
                _ = open::that_detached(views::about::REPOSITORY);
                Task::none()
            }

            ShellMessage::ToggleContextPage(context_page) => {
                // A context sheet is opened from the Browse toolbar's overflow
                // menu, among other places; whichever it was, that menu has
                // served its purpose.
                self.models_page.browse_menu_open = false;
                if self.context_page == context_page {
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    self.context_page = context_page;
                    self.core.window.show_context = true;
                }
                Task::none()
            }

            ShellMessage::CopyText(text) => cosmic::iced::clipboard::write(text),

            ShellMessage::WindowWidth(id, width) => {
                if self.core.main_window_id() == Some(id) {
                    self.window_width = width;
                }
                Task::none()
            }

            ShellMessage::SelectTab(page) => {
                match crate::state::Section::of(page) {
                    crate::state::Section::SpeechToText => self.speech_to_text_tab = page,
                    crate::state::Section::TextToSpeech => self.text_to_speech_tab = page,
                    // One-page sections have no tabs to remember.
                    _ => {}
                }
                self.page_shown(page)
            }

            ShellMessage::BrowseFor(stage) => self.browse_for(stage),

            ShellMessage::SceneStep => self.scene_step(),
            ShellMessage::SceneTarget(bounds) => self.scene_target(bounds),
            ShellMessage::SceneStt(read) => self.scene_stt(read),

            #[cfg(target_os = "macos")]
            ShellMessage::WindowOpened(id) => self.window_opened(id),
            #[cfg(target_os = "macos")]
            ShellMessage::TitleBarMeasured(title_bar) => {
                self.title_bar = title_bar;
                Task::none()
            }
            #[cfg(target_os = "macos")]
            ShellMessage::WindowResized(id) => {
                cosmic::iced::window::run_with_handle(id, crate::core::app::macos::is_full_screen)
                    .map(|full_screen| {
                        cosmic::Action::App(Message::Shell(ShellMessage::FullScreenChecked(
                            full_screen,
                        )))
                    })
            }
            #[cfg(target_os = "macos")]
            ShellMessage::FullScreenChecked(full_screen) => self.full_screen_checked(full_screen),
            #[cfg(target_os = "macos")]
            ShellMessage::SystemLook(look) => self.follow_system_look(look),
            #[cfg(target_os = "macos")]
            ShellMessage::DragWindow => self
                .core
                .main_window_id()
                .map_or_else(Task::none, cosmic::iced::window::drag),
            #[cfg(target_os = "macos")]
            ShellMessage::ZoomWindow => self
                .core
                .main_window_id()
                .map_or_else(Task::none, cosmic::iced::window::toggle_maximize),
            #[cfg(target_os = "macos")]
            ShellMessage::ToggleSidebar => {
                // As libcosmic's own toggle does: a narrow window swaps the
                // sidebar for the content rather than showing both.
                if self.core.is_condensed() {
                    self.core.nav_bar_toggle_condensed();
                } else {
                    self.core.nav_bar_toggle();
                }
                Task::none()
            }

            ShellMessage::LaunchUrl(url) => {
                match open::that_detached(&url) {
                    Ok(()) => {}
                    Err(err) => {
                        eprintln!("failed to open {url:?}: {err}");
                    }
                }
                Task::none()
            }
        }
    }
}

impl AppModel {
    /// The Library's Browse tab, showing what can fill `stage`.
    ///
    /// The way out of an empty picker sheet: it says nothing installed can
    /// fill the stage, and this is where something can be installed. The sheet
    /// closes, since what it lists does not change until an install is done.
    fn browse_for(&mut self, stage: crate::state::Stage) -> Task<cosmic::Action<Message>> {
        use crate::state::ModelsTab;
        use crate::state::Stage;
        use crate::state::registry::{FeatureFilter, RoleFilter};
        use crate::ui::messages::ModelsPageMessage;

        self.core.window.show_context = false;
        let filters = &mut self.registry.filters;
        filters.search.clear();
        (filters.feature, filters.role) = match stage {
            Stage::Transcription => (FeatureFilter::SpeechToText, RoleFilter::Transcription),
            Stage::PostProcessing => (FeatureFilter::SpeechToText, RoleFilter::PostProcessing),
            Stage::Synthesis => (FeatureFilter::TextToSpeech, RoleFilter::All),
        };

        let mut tasks = Vec::new();
        let library = self.nav.iter().find(|&id| {
            self.nav.data::<crate::state::Section>(id) == Some(&crate::state::Section::Library)
        });
        if let Some(id) = library {
            tasks.push(cosmic::Application::on_nav_select(self, id));
        }
        let tabs = &self.models_page.models_tabs;
        if let Some(id) = tabs
            .iter()
            .find(|&id| tabs.data::<ModelsTab>(id) == Some(&ModelsTab::Download))
        {
            tasks.push(Task::done(cosmic::Action::App(Message::ModelsPage(
                ModelsPageMessage::ModelsTabActivated(id),
            ))));
        }
        Task::batch(tasks)
    }
}
