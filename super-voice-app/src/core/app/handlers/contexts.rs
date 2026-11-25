// SPDX-License-Identifier: GPL-3.0-only
//! The Contexts tab, wired to its logic in the core
//! (`super_voice_app_core::pages::contexts`): the app applies the prompt
//! editor's own actions, runs the work the logic asks for as iced tasks, shows
//! and hides its editor sheet, and says its notices in its own words.
//!
//! Which context one backend uses is set from its Configure sheet
//! (`handlers/backend.rs`).

use cosmic::prelude::*;
use cosmic::widget::text_input;
use super_voice_app_core::pages::contexts::{self as page, Effect, Input, Notice, Outcome};

use crate::core::app::AppModel;
use crate::state::models::ContextPage;
use crate::ui::messages::{ContextsMessage, Message};

impl AppModel {
    pub(in crate::core::app) fn handle_contexts_messages(
        &mut self,
        message: ContextsMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            ContextsMessage::Page(input) => self.contexts_input(input),
            ContextsMessage::PromptAction(action) => {
                let Some(draft) = self.contexts.draft.as_mut() else {
                    return Task::none();
                };
                draft.prompt.0.perform(action);
                self.contexts_input(Input::PromptEdited)
            }
        }
    }

    /// Hand `input` to the page's logic, and do what it asks.
    fn contexts_input(&mut self, input: Input) -> Task<cosmic::Action<Message>> {
        let Outcome {
            effects,
            clear_notice,
            notice,
        } = page::update(&mut self.contexts, input);
        if clear_notice {
            self.clear_action_error(crate::state::ErrorScope::Contexts);
        }
        if let Some(notice) = notice {
            self.set_action_error(crate::state::ErrorScope::Contexts, notice_text(notice));
        }
        let tasks: Vec<_> = effects.into_iter().map(|e| self.run_contexts(e)).collect();
        Task::batch(tasks)
    }

    /// What the page asks for, as an iced task, or done to the window.
    fn run_contexts(&mut self, effect: Effect) -> Task<cosmic::Action<Message>> {
        match effect {
            Effect::List => {
                let list = Task::perform(page::run_list(), page_action);
                // The open Configure sheet shows what its backend is pointed
                // at, which the contexts changing may have changed too.
                let sheet = self.configure_input(
                    super_voice_app_core::pages::configure::Input::ContextsChanged,
                );
                Task::batch([list, sheet])
            }
            Effect::Activate(id) => Task::perform(page::run_activate(id), page_action),
            Effect::Delete(id) => Task::perform(page::run_delete(id), page_action),
            Effect::Save(context) => Task::perform(page::run_save(context), page_action),
            Effect::FocusTerm(row) => text_input::focus(crate::ui::views::contexts::term_id(row)),
            Effect::OpenEditor => {
                self.context_page = ContextPage::EditContext;
                self.core.window.show_context = true;
                Task::none()
            }
            Effect::CloseEditor => {
                self.core.window.show_context = false;
                Task::none()
            }
        }
    }
}

fn page_action(input: Input) -> cosmic::Action<Message> {
    app(ContextsMessage::Page(input))
}

/// What the page tells the user, in this app's words.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::CouldNotLoad(e) => crate::fl!("contexts-could-not-load", error = e),
        Notice::CouldNotSave(e) => crate::fl!("context-could-not-save", error = e),
    }
}

/// `ContextsMessage` as a cosmic action.
fn app(message: ContextsMessage) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Contexts(message))
}
