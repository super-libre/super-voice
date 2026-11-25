// SPDX-License-Identifier: GPL-3.0-only

use crate::core::app::AppModel;
use crate::daemon::client::set_read_method;
use crate::daemon::client::v1::speak::speak_read_command;
use crate::ui::messages::{Message, ReadMethodMessage};
use cosmic::prelude::*;
use super_voice_shared::models::read_method::ReadMethod;

/// What the Read aloud tab says when its test is refused. The two refusals
/// a user can fix in a moment get a sentence saying how; anything else is
/// the daemon's own words.
fn read_failure_message(method: ReadMethod, error: &str) -> String {
    if error.contains("nothing_to_read") {
        match method {
            ReadMethod::Selection => crate::fl!("read-nothing-selection"),
            ReadMethod::Clipboard => crate::fl!("read-nothing-clipboard"),
        }
    } else if error.contains("text_too_long") {
        crate::fl!("read-too-much")
    } else {
        crate::fl!("read-failed", error = error.to_string())
    }
}

impl AppModel {
    /// The Read aloud tab: where the shortcut reads from, saved
    /// confirm-then-apply like the stop mode below, and its test.
    pub(in crate::core::app) fn handle_read_method_messages(
        &mut self,
        message: ReadMethodMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            ReadMethodMessage::Changed(method) => {
                Task::perform(set_read_method(method.to_string()), move |result| {
                    cosmic::Action::App(Message::ReadMethod(match result {
                        Ok(()) => ReadMethodMessage::Loaded(method),
                        Err(e) => ReadMethodMessage::Error(e.to_string()),
                    }))
                })
            }
            ReadMethodMessage::Loaded(method) => {
                self.read_method = method;
                self.clear_action_error(crate::state::ErrorScope::ReadAloud);
                Task::none()
            }
            ReadMethodMessage::Error(err) => {
                log::warn!("Read method error: {err}");
                self.set_action_error(
                    crate::state::ErrorScope::ReadAloud,
                    crate::fl!("read-method-could-not-save", error = err),
                );
                Task::none()
            }
            // The speaking status follows the daemon's events, as on the
            // Speech tab, so only a refusal needs handling here.
            ReadMethodMessage::Test => {
                self.clear_action_error(crate::state::ErrorScope::ReadAloud);
                Task::perform(speak_read_command(), |result| match result {
                    Ok(_) => cosmic::Action::None,
                    Err(e) => cosmic::Action::App(Message::ReadMethod(
                        ReadMethodMessage::TestFailed(e.to_string()),
                    )),
                })
            }
            ReadMethodMessage::TestFailed(error) => {
                let message = read_failure_message(self.read_method, &error);
                self.set_action_error(crate::state::ErrorScope::ReadAloud, message);
                Task::none()
            }
        }
    }
}
