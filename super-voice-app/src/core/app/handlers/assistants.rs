// SPDX-License-Identifier: GPL-3.0-only
//! The Assistants page: asking the daemon where each assistant stands,
//! connecting and disconnecting one, whether it asks its questions aloud, and
//! the keys made by hand for other MCP clients.

use crate::core::app::AppModel;
use crate::daemon::client::v1::assistants::{
    connect_assistant, create_key, disconnect_assistant, list_assistants, remove_key,
    set_ask_questions_aloud,
};
use crate::state::ErrorScope;
use crate::state::assistants::{ConnectDialog, KeyDialog};
use crate::ui::messages::{AssistantsMessage, Message};
use cosmic::prelude::*;
use super_voice_shared::models::assistants::{AssistantId, AssistantList, NewKey};

fn answer(message: AssistantsMessage) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Assistants(message))
}

/// Read the list again: what failed may have changed something.
fn refresh() -> Task<cosmic::Action<Message>> {
    Task::done(answer(AssistantsMessage::Refresh))
}

impl AppModel {
    /// Route the Assistants page's messages. The daemon answers every change
    /// with the whole list, so the page shows what the configs now say.
    pub(in crate::core::app) fn handle_assistants_messages(
        &mut self,
        message: AssistantsMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            AssistantsMessage::Refresh => Task::perform(list_assistants(), |result| {
                answer(AssistantsMessage::Loaded(result.map_err(|e| e.to_string())))
            }),
            AssistantsMessage::Loaded(Ok(list)) | AssistantsMessage::Changed(Ok(list)) => {
                self.assistants.list = Some(list);
                Task::none()
            }
            AssistantsMessage::Loaded(Err(error)) => {
                log::warn!("Could not read the assistants: {error}");
                self.set_action_error(
                    ErrorScope::Assistants,
                    crate::fl!("assistants-load-failed", error = error),
                );
                Task::none()
            }
            AssistantsMessage::Changed(Err(error)) => {
                log::warn!("{error}");
                self.set_action_error(ErrorScope::Assistants, error);
                refresh()
            }
            AssistantsMessage::AskConnect(id) => {
                self.assistants.connect = Some(ConnectDialog {
                    id,
                    connecting: false,
                });
                self.clear_action_error(ErrorScope::Assistants);
                Task::none()
            }
            AssistantsMessage::CancelConnect => {
                if self.assistants.connect.is_some_and(|d| !d.connecting) {
                    self.assistants.connect = None;
                    self.clear_action_error(ErrorScope::Assistants);
                }
                Task::none()
            }
            AssistantsMessage::Connect => self.confirm_connect(),
            AssistantsMessage::Connected(id, result) => self.connect_answered(id, result),
            AssistantsMessage::Disconnect(id) => self.start_disconnect(id),
            AssistantsMessage::Disconnected(id, result) => self.disconnect_answered(id, result),
            AssistantsMessage::SetAskQuestionsAloud(id, on) => {
                self.change_ask_questions_aloud(id, on)
            }
            AssistantsMessage::AskKey => {
                self.assistants.key = Some(KeyDialog::new());
                self.clear_action_error(ErrorScope::Assistants);
                cosmic::widget::text_input::focus(crate::ui::views::assistants::key_name_id())
            }
            AssistantsMessage::KeyNameChanged(name) => {
                if let Some(KeyDialog::Naming {
                    name: field,
                    creating: false,
                }) = self.assistants.key.as_mut()
                {
                    *field = name;
                }
                Task::none()
            }
            AssistantsMessage::CreateKey => self.confirm_create_key(),
            AssistantsMessage::KeyCreated(result) => self.key_answered(result),
            // Not while the key is being made: its answer has to land in the
            // dialog, or the key is lost.
            AssistantsMessage::CloseKey => {
                if !matches!(
                    self.assistants.key,
                    Some(KeyDialog::Naming { creating: true, .. })
                ) {
                    self.assistants.key = None;
                    self.clear_action_error(ErrorScope::Assistants);
                }
                Task::none()
            }
            AssistantsMessage::RemoveKey(id) => {
                self.clear_action_error(ErrorScope::Assistants);
                Task::perform(remove_key(id), |result| {
                    answer(AssistantsMessage::Changed(result.map_err(|e| {
                        crate::fl!("key-remove-failed", error = e.to_string())
                    })))
                })
            }
        }
    }

    /// The user said Connect in the dialog: ask the daemon, once.
    fn confirm_connect(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(dialog) = self.assistants.connect.as_mut().filter(|d| !d.connecting) else {
            return Task::none();
        };
        dialog.connecting = true;
        let id = dialog.id;
        self.clear_action_error(ErrorScope::Assistants);
        Task::perform(connect_assistant(id), move |result| {
            answer(AssistantsMessage::Connected(
                id,
                result.map_err(|e| e.to_string()),
            ))
        })
    }

    /// The daemon's answer to a connect. On a failure the dialog stays open
    /// and says why, so the user can try again.
    fn connect_answered(
        &mut self,
        id: AssistantId,
        result: Result<AssistantList, String>,
    ) -> Task<cosmic::Action<Message>> {
        match result {
            Ok(list) => {
                self.assistants.list = Some(list);
                self.assistants.connect = None;
                self.assistants.connected_here.insert(id);
                Task::none()
            }
            Err(error) => {
                if let Some(dialog) = self.assistants.connect.as_mut() {
                    dialog.connecting = false;
                }
                log::warn!("Could not connect {id}: {error}");
                self.set_action_error(
                    ErrorScope::Assistants,
                    crate::fl!(
                        "assistant-connect-failed",
                        name = id.display_name(),
                        error = error
                    ),
                );
                refresh()
            }
        }
    }

    /// Disconnect, once at a time per assistant: its button waits.
    fn start_disconnect(&mut self, id: AssistantId) -> Task<cosmic::Action<Message>> {
        if !self.assistants.disconnecting.insert(id) {
            return Task::none();
        }
        self.clear_action_error(ErrorScope::Assistants);
        Task::perform(disconnect_assistant(id), move |result| {
            answer(AssistantsMessage::Disconnected(
                id,
                result.map_err(|e| e.to_string()),
            ))
        })
    }

    /// The daemon's answer to a disconnect.
    fn disconnect_answered(
        &mut self,
        id: AssistantId,
        result: Result<AssistantList, String>,
    ) -> Task<cosmic::Action<Message>> {
        self.assistants.disconnecting.remove(&id);
        self.assistants.connected_here.remove(&id);
        match result {
            Ok(list) => {
                self.assistants.list = Some(list);
                Task::none()
            }
            Err(error) => {
                log::warn!("Could not disconnect {id}: {error}");
                self.set_action_error(
                    ErrorScope::Assistants,
                    crate::fl!(
                        "assistant-disconnect-failed",
                        name = id.display_name(),
                        error = error
                    ),
                );
                refresh()
            }
        }
    }

    /// The switch moves at once; the daemon's answer, or the list read again
    /// after a failure, puts it where the daemon has it.
    fn change_ask_questions_aloud(
        &mut self,
        id: AssistantId,
        on: bool,
    ) -> Task<cosmic::Action<Message>> {
        if let Some(assistant) = self
            .assistants
            .list
            .as_mut()
            .and_then(|list| list.assistants.iter_mut().find(|a| a.id == id))
        {
            assistant.ask_questions_aloud = on;
        }
        self.clear_action_error(ErrorScope::Assistants);
        Task::perform(set_ask_questions_aloud(id, on), move |result| {
            answer(AssistantsMessage::Changed(result.map_err(|e| {
                crate::fl!(
                    "assistant-change-failed",
                    name = id.display_name(),
                    error = e.to_string()
                )
            })))
        })
    }

    /// The user said Create: make the key under the name typed, once.
    fn confirm_create_key(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(name) = self
            .assistants
            .key
            .as_ref()
            .and_then(KeyDialog::name_to_create)
            .map(str::to_string)
        else {
            return Task::none();
        };
        if let Some(KeyDialog::Naming { creating, .. }) = self.assistants.key.as_mut() {
            *creating = true;
        }
        self.clear_action_error(ErrorScope::Assistants);
        Task::perform(create_key(name), |result| {
            answer(AssistantsMessage::KeyCreated(
                result.map_err(|e| e.to_string()),
            ))
        })
    }

    /// The daemon's answer to Create: the key, shown in the dialog with the
    /// list read again for its row, or why not, with the name kept.
    fn key_answered(&mut self, result: Result<NewKey, String>) -> Task<cosmic::Action<Message>> {
        match result {
            Ok(key) => {
                self.assistants.key = Some(KeyDialog::Made(key));
                refresh()
            }
            Err(error) => {
                if let Some(KeyDialog::Naming { creating, .. }) = self.assistants.key.as_mut() {
                    *creating = false;
                }
                log::warn!("Could not create a key: {error}");
                self.set_action_error(
                    ErrorScope::Assistants,
                    crate::fl!("key-create-failed", error = error),
                );
                Task::none()
            }
        }
    }
}
