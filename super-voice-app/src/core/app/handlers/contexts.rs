// SPDX-License-Identifier: GPL-3.0-only
//! The Contexts tab, wired to its logic in the core
//! (`super_voice_app_core::pages::contexts`): the app applies the prompt
//! editor's own actions, runs the work the logic asks for as iced tasks, shows
//! and hides its editor sheet, and says its notices in its own words.
//!
//! Which context one backend uses is set from the Models page's Configure
//! sheet, and handled here until that page moves into the core too.

use cosmic::prelude::*;
use cosmic::widget::text_input;
use super_voice_app_core::pages::contexts::{self as page, Effect, Input, Notice, Outcome};

use crate::core::app::AppModel;
use crate::daemon::client::v1::contexts::backend as backend_client;
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
            per_backend @ (ContextsMessage::BackendPinned { .. }
            | ContextsMessage::BackendFollowsActive(_)
            | ContextsMessage::BackendContextLoaded { .. }) => {
                self.handle_backend_context_messages(per_backend)
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
                // at, read when it opened. The list is read again when
                // another app changed the contexts, and a backend pinned or
                // unpinned elsewhere left the sheet saying the opposite until
                // it was closed and opened again.
                match self.models_page.configure_backend.clone() {
                    Some(source) if self.contexts.backend_contexts.contains_key(&source) => {
                        Task::batch([list, Self::fetch_backend_context(source)])
                    }
                    _ => list,
                }
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

    /// Which context one backend uses, from its Configure sheet.
    fn handle_backend_context_messages(
        &mut self,
        message: ContextsMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            ContextsMessage::BackendPinned { source, id } => {
                self.clear_action_error(crate::state::ErrorScope::ConfigureBackend);
                let for_message = source.clone();
                Task::perform(
                    backend_client::set_backend_context(source, id),
                    move |res| match res {
                        Ok(state) => app(ContextsMessage::BackendContextLoaded {
                            source: for_message.clone(),
                            state,
                        }),
                        Err(e) => cosmic::Action::App(Message::SettingActionFailed {
                            scope: crate::state::ErrorScope::ConfigureBackend,
                            message: crate::fl!(
                                "backend-context-change-failed",
                                error = e.to_string()
                            ),
                        }),
                    },
                )
            }
            ContextsMessage::BackendFollowsActive(source) => {
                self.clear_action_error(crate::state::ErrorScope::ConfigureBackend);
                let for_message = source.clone();
                Task::perform(
                    backend_client::clear_backend_context(source),
                    move |res| match res {
                        Ok(state) => app(ContextsMessage::BackendContextLoaded {
                            source: for_message.clone(),
                            state,
                        }),
                        Err(e) => cosmic::Action::App(Message::SettingActionFailed {
                            scope: crate::state::ErrorScope::ConfigureBackend,
                            message: crate::fl!(
                                "backend-context-change-failed",
                                error = e.to_string()
                            ),
                        }),
                    },
                )
            }
            ContextsMessage::BackendContextLoaded { source, state } => {
                self.contexts.backend_contexts.insert(source, state);
                Task::none()
            }

            other @ (ContextsMessage::Page(_) | ContextsMessage::PromptAction(_)) => {
                log::error!("contexts: the backend handler can't serve {other:?}");
                Task::none()
            }
        }
    }

    /// Ask the daemon what one backend uses, for the Configure sheet's picker.
    ///
    /// Read on demand rather than published with the catalog: it is one line of
    /// the sheet, and `GET /backend/list` is fetched on every navigation to two
    /// pages that never show it.
    pub(in crate::core::app) fn fetch_backend_context(
        source: String,
    ) -> Task<cosmic::Action<Message>> {
        let for_message = source.clone();
        Task::perform(
            backend_client::get_backend_context(source),
            move |res| match res {
                Ok(state) => app(ContextsMessage::BackendContextLoaded {
                    source: for_message.clone(),
                    state,
                }),
                // A backend whose context cannot be read renders as "follows
                // the active one", which is the default and the truth for
                // almost every backend. Not worth a banner in a sheet that is
                // about secrets and options.
                Err(e) => {
                    log::warn!("Couldn't read this backend's context: {e}");
                    cosmic::Action::None
                }
            },
        )
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
