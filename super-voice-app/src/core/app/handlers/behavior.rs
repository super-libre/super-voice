// SPDX-License-Identifier: GPL-3.0-only
//! Speech to text's Behavior tab, wired to its logic in the core
//! (`super_voice_app_core::pages::behavior`): the app runs the work the logic
//! asks for as iced tasks, puts the cursor in its typing test's field before
//! that test types, and says the notices in its own words, on the banner of
//! the section each is about.

use cosmic::prelude::*;
use super_voice_app_core::pages::behavior::{
    self as page, Effect, Input, Notice, Outcome, Section,
};

use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::ui::messages::{BehaviorMessage, Message};

pub(in crate::core::app) fn page_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Behavior(BehaviorMessage::Page(input)))
}

impl AppModel {
    pub(in crate::core::app) fn handle_behavior_messages(
        &mut self,
        message: BehaviorMessage,
    ) -> Task<cosmic::Action<Message>> {
        let BehaviorMessage::Page(input) = message;
        let Outcome {
            effects,
            clear_notice,
            notice,
        } = page::update(&mut self.behavior, input);
        if let Some(section) = clear_notice {
            self.clear_action_error(scope(section));
        }
        if let Some(notice) = notice {
            let section = notice.section();
            self.set_action_error(scope(section), notice_text(notice));
        }
        Task::batch(effects.into_iter().map(run))
    }
}

/// What the tab asks for, as an iced task.
fn run(effect: Effect) -> Task<cosmic::Action<Message>> {
    match effect {
        Effect::SaveStopMode(mode) => Task::perform(page::run_save_stop_mode(mode), page_action),
        Effect::SavePreviewTyping(on) => {
            Task::perform(page::run_save_preview_typing(on), page_action)
        }
        Effect::SaveWriteMethod(method) => {
            Task::perform(page::run_save_write_method(method), page_action)
        }
        // The cursor goes into the test field first, and `chain` makes it:
        // `batch` would race the focus against the daemon typing.
        Effect::TestTyping { into_field: true } => {
            cosmic::widget::text_input::focus(crate::ui::views::input_simulation::test_field_id())
                .chain(Task::perform(page::run_test_typing(), page_action))
        }
        Effect::TestTyping { into_field: false } => {
            Task::perform(page::run_test_typing(), page_action)
        }
        Effect::Tick => Task::perform(page::run_tick(), page_action),
        Effect::SaveTheme {
            chosen,
            play,
            theme,
            sounding,
        } => Task::future(page::run_save_theme(chosen, play, theme, sounding))
            .and_then(|input| Task::done(page_action(input))),
        Effect::SaveVolume { volume, stored } => {
            Task::future(page::run_save_volume(volume, stored))
                .and_then(|input| Task::done(page_action(input)))
        }
    }
}

/// Where a notice goes: the recording section's banner, or typing's.
fn scope(section: Section) -> ErrorScope {
    match section {
        Section::Recording => ErrorScope::Recording,
        Section::Typing => ErrorScope::InputSimulation,
    }
}

/// What the tab tells the user, in this app's words.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::CouldNotSaveStopMode(e) => crate::fl!("stop-mode-could-not-save", error = e),
        Notice::CouldNotSavePreviewTyping(e) => {
            crate::fl!("preview-typing-could-not-save", error = e)
        }
        Notice::CouldNotSaveWriteMethod(e) => crate::fl!("write-method-could-not-save", error = e),
        Notice::TypingTestFailed(e) => crate::fl!("typing-test-failed", error = e),
        // The daemon's own words, as before: saving a sound has no lead-in.
        Notice::CouldNotSaveSounds(e) => e,
    }
}
