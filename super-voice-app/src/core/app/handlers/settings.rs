// SPDX-License-Identifier: GPL-3.0-only
//! The Read aloud settings, wired to their logic in the core
//! (`super_voice_app_core::pages::read_aloud`): the app runs the work the
//! logic asks for as iced tasks, and says its notices in its own words.

use cosmic::prelude::*;
use super_voice_app_core::pages::read_aloud::{self as page, Effect, Input, Notice, Outcome};

use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::ui::messages::{Message, ReadMethodMessage};

impl AppModel {
    /// The Read aloud settings: where the shortcut reads from, and its test.
    pub(in crate::core::app) fn handle_read_method_messages(
        &mut self,
        message: ReadMethodMessage,
    ) -> Task<cosmic::Action<Message>> {
        let ReadMethodMessage::Page(input) = message;
        let Outcome {
            effects,
            clear_notice,
            notice,
        } = page::update(&mut self.read_method, input);
        if clear_notice {
            self.clear_action_error(ErrorScope::ReadAloud);
        }
        if let Some(notice) = notice {
            self.set_action_error(ErrorScope::ReadAloud, notice_text(notice));
        }
        Task::batch(effects.into_iter().map(|effect| match effect {
            Effect::Load => Task::perform(page::run_load(), page_action),
            Effect::Save(method) => Task::perform(page::run_save(method), page_action),
            Effect::Test => Task::perform(page::run_test(), |input| {
                input.map_or(cosmic::Action::None, page_action)
            }),
        }))
    }
}

/// `input` as a cosmic action.
pub(in crate::core::app) fn page_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::ReadMethod(ReadMethodMessage::Page(input)))
}

/// What the page tells the user, in this app's words.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::CouldNotSave(e) => crate::fl!("read-method-could-not-save", error = e),
        Notice::NothingSelected => crate::fl!("read-nothing-selection"),
        Notice::NothingInClipboard => crate::fl!("read-nothing-clipboard"),
        Notice::TooLong => crate::fl!("read-too-much"),
        Notice::CouldNotRead(e) => crate::fl!("read-failed", error = e),
    }
}
