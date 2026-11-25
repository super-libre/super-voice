// SPDX-License-Identifier: GPL-3.0-only
//! The Assistants page, wired to its logic in the core
//! (`super_voice_app_core::pages::assistants`): the app runs the work the
//! logic asks for as iced tasks, puts the cursor in the key's name field, and
//! says the notices in its own words.

use cosmic::prelude::*;
use super_voice_app_core::pages::assistants::{self as page, Effect, Input, Notice, Outcome};

use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::ui::messages::{AssistantsMessage, Message};

fn page_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Assistants(AssistantsMessage::Page(input)))
}

impl AppModel {
    pub(in crate::core::app) fn handle_assistants_messages(
        &mut self,
        message: AssistantsMessage,
    ) -> Task<cosmic::Action<Message>> {
        let AssistantsMessage::Page(input) = message;
        let Outcome {
            effects,
            clear_notice,
            notice,
        } = page::update(&mut self.assistants, input);
        if clear_notice {
            self.clear_action_error(ErrorScope::Assistants);
        }
        if let Some(notice) = notice {
            log::warn!("Assistants: {notice:?}");
            self.set_action_error(ErrorScope::Assistants, notice_text(notice));
        }
        Task::batch(effects.into_iter().map(run))
    }
}

/// What the page asks for, as an iced task.
fn run(effect: Effect) -> Task<cosmic::Action<Message>> {
    match effect {
        Effect::List => Task::perform(page::run_list(), page_action),
        Effect::Connect(id) => Task::perform(page::run_connect(id), page_action),
        Effect::Disconnect(id) => Task::perform(page::run_disconnect(id), page_action),
        Effect::SetFlag(id, flag, on) => {
            Task::perform(page::run_set_flag(id, flag, on), page_action)
        }
        Effect::CreateKey(name) => Task::perform(page::run_create_key(name), page_action),
        Effect::RemoveKey(id) => Task::perform(page::run_remove_key(id), page_action),
        Effect::FocusKeyName => {
            cosmic::widget::text_input::focus(crate::ui::views::assistants::key_name_id())
        }
    }
}

/// What the page tells the user, in this app's words.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::CouldNotLoad(e) => crate::fl!("assistants-load-failed", error = e),
        Notice::CouldNotConnect(id, e) => {
            crate::fl!(
                "assistant-connect-failed",
                name = id.display_name(),
                error = e
            )
        }
        Notice::CouldNotDisconnect(id, e) => {
            crate::fl!(
                "assistant-disconnect-failed",
                name = id.display_name(),
                error = e
            )
        }
        Notice::CouldNotChange(id, e) => {
            crate::fl!(
                "assistant-change-failed",
                name = id.display_name(),
                error = e
            )
        }
        Notice::CouldNotCreateKey(e) => crate::fl!("key-create-failed", error = e),
        Notice::CouldNotRemoveKey(e) => crate::fl!("key-remove-failed", error = e),
    }
}
