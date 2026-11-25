// SPDX-License-Identifier: GPL-3.0-only
//! The backend list, and a backend's Configure sheet, wired to its logic in
//! the core (`super_voice_app_core::pages::configure`): the app runs the work
//! the logic asks for as iced tasks, and says its notices in its own words.

use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::ui::messages::{BackendMessage, ContextsMessage, Message};
use cosmic::prelude::*;
use super_voice_app_core::pages::configure::{self as page, Effect, Input, Notice, Outcome};

impl AppModel {
    /// The backend list, and the Configure sheet's secrets, options and
    /// context.
    pub(in crate::core::app) fn handle_backend_messages(
        &mut self,
        message: BackendMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            BackendMessage::BackendsLoaded(backends) => {
                // Drop uninstall errors for backends no longer present.
                self.registry
                    .uninstall_errors
                    .retain(|src, _| backends.iter().any(|b| &b.source == src));
                self.backends = backends;
                let mut tasks = vec![self.configure_input(Input::BackendsLoaded)];
                // Load the registry index once so installed cards can show each
                // backend's description (it lives on the registry entry, keyed
                // by source). Guarded so it fires a single time; the Browse tab's
                // own first-open trigger then short-circuits.
                if self.registry.backends.is_empty() && self.registry.last_refresh.is_none() {
                    tasks.push(crate::core::app::handlers::tasks::fetch_registry_catalog(
                        false,
                    ));
                }
                Task::batch(tasks)
            }

            BackendMessage::BackendsError(err) => {
                log::warn!("Backends load error: {err}");
                Task::none()
            }

            BackendMessage::BackendsReload => crate::core::app::handlers::tasks::reload_backends(),

            BackendMessage::Configure(input) => self.configure_input(input),
        }
    }

    /// Hand `input` to the Configure sheet's logic, and do what it asks.
    pub(in crate::core::app) fn configure_input(
        &mut self,
        input: Input,
    ) -> Task<cosmic::Action<Message>> {
        let Outcome {
            effects,
            clear_notice,
            notice,
        } = page::update(&mut self.configure, &self.backends, input);
        if clear_notice {
            self.clear_action_error(ErrorScope::ConfigureBackend);
        }
        if let Some(notice) = notice {
            self.set_action_error(ErrorScope::ConfigureBackend, notice_text(notice));
        }
        Task::batch(effects.into_iter().map(run_configure))
    }
}

/// What the sheet asks for, as an iced task.
fn run_configure(effect: Effect) -> Task<cosmic::Action<Message>> {
    match effect {
        Effect::ListSecrets(source) => Task::perform(page::run_list_secrets(source), page_action),
        Effect::SaveSecret {
            source,
            name,
            value,
        } => Task::perform(page::run_save_secret(source, name, value), page_action),
        Effect::RemoveSecret { source, name } => {
            Task::perform(page::run_remove_secret(source, name), page_action)
        }
        Effect::SetOption {
            source,
            name,
            value,
        } => Task::perform(page::run_set_option(source, name, value), page_action),
        Effect::ClearOption { source, name } => {
            Task::perform(page::run_clear_option(source, name), page_action)
        }
        Effect::ReadContext(source) => Task::perform(page::run_read_context(source), page_action),
        Effect::PickContext { source, pick } => {
            Task::perform(page::run_pick_context(source, pick), page_action)
        }
        Effect::ReloadBackends => crate::core::app::handlers::tasks::reload_backends(),
        Effect::ReloadContexts => Task::done(cosmic::Action::App(Message::Contexts(
            ContextsMessage::Page(super_voice_app_core::pages::contexts::Input::Reload),
        ))),
    }
}

fn page_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Backend(BackendMessage::Configure(input)))
}

/// What the sheet tells the user, in this app's words.
fn notice_text(notice: Notice) -> String {
    match notice {
        // The daemon's own words, as the sheet has always shown them.
        Notice::CouldNotSave(e) => e,
        Notice::ContextNotChanged(e) => crate::fl!("backend-context-change-failed", error = e),
    }
}
