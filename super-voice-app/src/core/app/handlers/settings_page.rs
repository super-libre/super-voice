// SPDX-License-Identifier: GPL-3.0-only
//! The Settings page, wired to its logic in the core
//! (`super_voice_app_core::pages::settings`): the app runs the work the logic
//! asks for as iced tasks, closes the language picker on a pick, and says the
//! notices in its own words.

use cosmic::prelude::*;
use super_voice_app_core::pages::settings::{self as page, Effect, Input, Notice, Outcome};

use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::ui::messages::{DaemonMessage, Message, SettingsMessage};

pub(in crate::core::app) fn page_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Settings(SettingsMessage::Page(input)))
}

/// Read how failures are reported.
pub(in crate::core::app) fn load_notification_method() -> Task<cosmic::Action<Message>> {
    Task::perform(page::run_load_notification_method(), page_action)
}

/// Read how long an unused model stays in memory.
pub(in crate::core::app) fn load_idle_unload() -> Task<cosmic::Action<Message>> {
    Task::perform(page::run_load_idle_unload(), page_action)
}

/// Read the primary language and the tags it can be: the picker needs both.
pub(in crate::core::app) fn load_language() -> Task<cosmic::Action<Message>> {
    Task::batch([
        Task::perform(page::run_load_primary_language(), page_action),
        Task::perform(page::run_load_languages(), page_action),
    ])
}

impl AppModel {
    pub(in crate::core::app) fn handle_settings_messages(
        &mut self,
        message: SettingsMessage,
    ) -> Task<cosmic::Action<Message>> {
        let SettingsMessage::Page(input) = message;
        // A pick closes the language picker it was made in.
        if matches!(input, Input::PrimaryLanguagePicked(_)) {
            self.core.window.show_context = false;
        }
        let Outcome {
            effects,
            clear_notice,
            notice,
        } = page::update(&mut self.settings, &mut self.language, input);
        if clear_notice {
            self.clear_action_error(ErrorScope::Customization);
        }
        if let Some(notice) = notice {
            log::warn!("Settings: {notice:?}");
            self.set_action_error(ErrorScope::Customization, notice_text(notice));
        }
        Task::batch(effects.into_iter().map(run))
    }
}

/// What the page asks for, as an iced task.
fn run(effect: Effect) -> Task<cosmic::Action<Message>> {
    match effect {
        Effect::SaveNotificationMethod(method) => {
            Task::perform(page::run_save_notification_method(method), page_action)
        }
        Effect::SaveIdleUnload(after_secs) => {
            Task::perform(page::run_save_idle_unload(after_secs), page_action)
        }
        Effect::SavePrimaryLanguage(tag) => {
            Task::perform(page::run_save_primary_language(tag), page_action)
        }
        // A model whose language follows the primary one reads differently
        // now, and the daemon's status says so.
        Effect::LanguageChanged => Task::done(cosmic::Action::App(Message::Daemon(
            DaemonMessage::RefreshDaemonStatus,
        ))),
    }
}

/// What the page tells the user, in this app's words.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::CouldNotSaveNotificationMethod(e) => {
            crate::fl!("notification-method-could-not-save", error = e)
        }
        Notice::CouldNotSaveIdleUnload(e) => crate::fl!("idle-unload-could-not-save", error = e),
        Notice::CouldNotSaveLanguage(e) => crate::fl!("language-could-not-save", error = e),
    }
}
