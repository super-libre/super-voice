// SPDX-License-Identifier: GPL-3.0-only

use crate::core::app::AppModel;
use crate::daemon::client::v1::speak::speak_read_command;
use crate::daemon::client::{set_idle_unload, set_notification_method, set_read_method};
use crate::ui::messages::{
    IdleUnloadMessage, Message, NotificationMethodMessage, ReadMethodMessage,
};
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

    /// Handle notification method messages
    pub(in crate::core::app) fn handle_notification_method_messages(
        &mut self,
        message: NotificationMethodMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            // Confirm-then-apply, as with the write method: apply only on the
            // daemon ack so a failed save doesn't strand an optimistic value.
            NotificationMethodMessage::Changed(method) => {
                let method_str = method.to_string();
                Task::perform(
                    set_notification_method(method_str),
                    move |result| match result {
                        Ok(()) => cosmic::Action::App(Message::NotificationMethod(
                            NotificationMethodMessage::Loaded(method),
                        )),
                        Err(e) => cosmic::Action::App(Message::NotificationMethod(
                            NotificationMethodMessage::Error(e.to_string()),
                        )),
                    },
                )
            }

            NotificationMethodMessage::Loaded(method) => {
                self.notification_method = method;
                self.clear_action_error(crate::state::ErrorScope::Customization);
                Task::none()
            }

            NotificationMethodMessage::Error(err) => {
                log::warn!("Notification method error: {err}");
                self.set_action_error(
                    crate::state::ErrorScope::Customization,
                    crate::fl!("notification-method-could-not-save", error = err),
                );
                Task::none()
            }
        }
    }

    /// Handle idle-unload messages
    pub(in crate::core::app) fn handle_idle_unload_messages(
        &mut self,
        message: IdleUnloadMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            // Confirm-then-apply, as with the notification method.
            IdleUnloadMessage::Changed(after_secs) => {
                Task::perform(set_idle_unload(after_secs), move |result| match result {
                    Ok(()) => cosmic::Action::App(Message::IdleUnload(IdleUnloadMessage::Loaded(
                        after_secs,
                    ))),
                    Err(e) => cosmic::Action::App(Message::IdleUnload(IdleUnloadMessage::Error(
                        e.to_string(),
                    ))),
                })
            }

            IdleUnloadMessage::Loaded(after_secs) => {
                self.idle_unload_secs = after_secs;
                self.clear_action_error(crate::state::ErrorScope::Customization);
                Task::none()
            }

            IdleUnloadMessage::Error(err) => {
                log::warn!("Idle unload error: {err}");
                self.set_action_error(
                    crate::state::ErrorScope::Customization,
                    crate::fl!("idle-unload-could-not-save", error = err),
                );
                Task::none()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::read_failure_message;
    use super_voice_shared::models::read_method::ReadMethod;

    /// Nothing to read says how to fix it for the source in use. Anything
    /// else keeps the daemon's words.
    #[test]
    fn a_refused_read_test_says_what_to_do() {
        let nothing = "nothing_to_read (HTTP 409)";
        assert!(read_failure_message(ReadMethod::Selection, nothing).contains("Highlight"));
        assert!(read_failure_message(ReadMethod::Clipboard, nothing).contains("Copy"));
        assert!(read_failure_message(ReadMethod::Selection, "text_too_long").contains("Too much"));
        assert!(
            read_failure_message(ReadMethod::Clipboard, "model_not_loaded (HTTP 409)")
                .contains("model_not_loaded")
        );
    }
}
