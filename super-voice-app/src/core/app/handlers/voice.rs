// SPDX-License-Identifier: GPL-3.0-only
//! The synthesis model's voice, wired to its logic in the core
//! (`super_voice_app_core::pages::model_voice`): the app runs the work the
//! logic asks for as iced tasks, and says the notices in its own words.

use cosmic::prelude::*;
use super_voice_app_core::pages::model_voice::{self as page, Effect, Input, Notice, Outcome};

use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::ui::messages::{Message, VoiceMessage};

pub(in crate::core::app) fn page_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Voice(VoiceMessage::Page(input)))
}

impl AppModel {
    /// Route every voice message.
    pub(in crate::core::app) fn handle_voice_messages(
        &mut self,
        message: VoiceMessage,
    ) -> Task<cosmic::Action<Message>> {
        let VoiceMessage::Page(input) = message;
        let Outcome {
            effects,
            clear_notice,
            notice,
        } = page::update(&mut self.voice, input);
        if clear_notice {
            self.clear_action_error(ErrorScope::Synthesis);
        }
        if let Some(notice) = notice {
            log::warn!("Voice settings error: {notice:?}");
            self.set_action_error(ErrorScope::Synthesis, notice_text(notice));
        }
        Task::batch(effects.into_iter().map(run))
    }
}

/// Read a model's voice. Called when a synthesis model is staged or selected,
/// so the card can show its voice before Load.
pub(in crate::core::app) fn load_model_voice(
    source: &str,
    model: String,
) -> Task<cosmic::Action<Message>> {
    Task::done(page_action(Input::Focus {
        source: source.to_string(),
        model,
    }))
}

/// What the logic asks for, as an iced task.
fn run(effect: Effect) -> Task<cosmic::Action<Message>> {
    match effect {
        Effect::Read { source, model } => Task::perform(page::run_read(source, model), page_action),
        Effect::List { source, model } => Task::perform(page::run_list(source, model), page_action),
        Effect::Save {
            source,
            model,
            voice,
        } => Task::perform(page::run_save(source, model, voice), page_action),
    }
}

/// What the logic tells the user, in this app's words.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::CouldNotSaveVoice(e) => crate::fl!("voice-could-not-save", error = e),
    }
}
