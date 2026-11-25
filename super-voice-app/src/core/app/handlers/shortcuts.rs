// SPDX-License-Identifier: GPL-3.0-only
//! The Shortcuts page, wired to its logic in the core
//! (`super_voice_app_core::pages::shortcuts`): the app runs the work the logic
//! asks for as iced tasks, and says the notices in its own words.

use cosmic::prelude::*;
use super_voice_app_core::pages::shortcuts::{self as page, Effect, Input, Notice, Outcome};

use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::ui::messages::{Message, ShortcutsMessage};

fn page_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Shortcuts(ShortcutsMessage::Page(input)))
}

impl AppModel {
    pub(in crate::core::app) fn handle_shortcuts_messages(
        &mut self,
        message: ShortcutsMessage,
    ) -> Task<cosmic::Action<Message>> {
        let ShortcutsMessage::Page(input) = message;
        let Outcome {
            effects,
            clear_notice,
            notice,
        } = page::update(&mut self.shortcuts, input);
        if clear_notice {
            self.clear_action_error(ErrorScope::Shortcuts);
        }
        if let Some(notice) = notice {
            self.set_action_error(ErrorScope::Shortcuts, notice_text(notice));
        }
        Task::batch(effects.into_iter().map(|effect| match effect {
            Effect::List => Task::perform(page::run_list(), page_action),
            Effect::Add(id) => Task::perform(page::run_add(id), page_action),
            Effect::Remove(id) => Task::perform(page::run_remove(id), page_action),
        }))
    }
}

/// What the page tells the user, in this app's words.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::CouldNotLoad(e) => crate::fl!("shortcuts-load-failed", error = e),
        Notice::CouldNotChange(e) => crate::fl!("shortcut-change-failed", error = e),
    }
}
