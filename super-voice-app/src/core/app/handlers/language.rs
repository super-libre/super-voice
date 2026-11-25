// SPDX-License-Identifier: GPL-3.0-only
//! The language picker sheet, and a model's language, wired to its logic in
//! the core (`super_voice_app_core::pages::model_language`): the app runs the
//! work the logic asks for as iced tasks, closes the sheet on a pick, and says
//! the notices in its own words.
//!
//! The app keeps every answer rather than a focus: the transcription card can
//! show the loaded model and a staged one at once, each with its button.

use cosmic::prelude::*;
use super_voice_app_core::pages::model_language::{
    self as page, Effect, Input, ModelKey, Notice, Outcome,
};

use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::state::Stage;
use crate::state::models::ContextPage;
use crate::ui::messages::{LanguageMessage, Message};

pub(in crate::core::app) fn page_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Language(LanguageMessage::Model(input)))
}

impl AppModel {
    pub(in crate::core::app) fn handle_language_messages(
        &mut self,
        message: LanguageMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            LanguageMessage::OpenLanguagePicker { model } => {
                self.language.language_picker_target = model;
                self.language.language_picker_query.clear();
                self.context_page = ContextPage::LanguagePicker;
                self.core.window.show_context = true;
                Task::none()
            }
            LanguageMessage::CloseLanguagePicker => {
                self.core.window.show_context = false;
                Task::none()
            }
            LanguageMessage::LanguagePickerQueryChanged(q) => {
                self.language.language_picker_query = q;
                Task::none()
            }
            LanguageMessage::Model(input) => self.handle_model_language(input),
        }
    }

    /// Hand `input` to a model's language logic, and do what it asks.
    fn handle_model_language(&mut self, input: Input) -> Task<cosmic::Action<Message>> {
        // A pick closes the sheet it was made in.
        if matches!(input, Input::Picked(..)) {
            self.core.window.show_context = false;
        }
        let Outcome {
            effects,
            clear_notice,
            notice,
        } = page::update(&mut self.language.model_languages, None, input);
        // The language picker lives on the Settings page: its failures are
        // told on that page's banner rather than only logged.
        if clear_notice {
            self.clear_action_error(ErrorScope::Customization);
        }
        if let Some(notice) = notice {
            log::warn!("Language settings error: {notice:?}");
            self.set_action_error(ErrorScope::Customization, notice_text(notice));
        }
        Task::batch(effects.into_iter().map(run))
    }

    /// Read a model's language, and what it can be set to. Call when a model
    /// becomes selected (staged or loaded) so its card shows its language
    /// before Load.
    #[allow(clippy::unused_self)]
    pub(in crate::core::app) fn load_model_language(
        &self,
        stage: Stage,
        source: &str,
        model: String,
    ) -> Task<cosmic::Action<Message>> {
        Task::done(page_action(Input::Focus(ModelKey::new(
            stage, source, model,
        ))))
    }
}

/// What the logic asks for, as an iced task.
fn run(effect: Effect) -> Task<cosmic::Action<Message>> {
    match effect {
        Effect::Read(key) => Task::perform(page::run_read(key), page_action),
        Effect::ReadOffer(key) => Task::perform(page::run_read_offer(key), page_action),
        Effect::Save(key, tag) => Task::perform(page::run_save(key, tag), page_action),
    }
}

/// What the logic tells the user, in this app's words.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::CouldNotSaveLanguage(e) => crate::fl!("language-could-not-save", error = e),
    }
}
