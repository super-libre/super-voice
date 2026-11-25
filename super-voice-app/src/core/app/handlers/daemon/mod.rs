// SPDX-License-Identifier: GPL-3.0-only

mod events;

use crate::core::app::handlers::tasks::{build_load_settings_tasks, ping_task};
use crate::core::app::{AppModel, DeviceState};
use crate::ui::messages::{DaemonMessage, Message, ModelMessage};
use cosmic::prelude::*;
use log::info;
use super_voice_app_core::pages::connection::{self as page, Effect, Input, Outcome};

impl AppModel {
    /// Handle daemon connection messages
    pub(in crate::core::app) fn handle_daemon_messages(
        &mut self,
        message: DaemonMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            DaemonMessage::RefreshDaemonStatus => Task::perform(page::run_check(), |input| {
                cosmic::Action::App(Message::Daemon(DaemonMessage::Connection(input)))
            }),

            DaemonMessage::PingTimeout => {
                // Surface a stalled model switch (no progress for too long)
                // rather than letting the UI spin on the now-untimed POST.
                self.check_switch_stall();
                self.connection(Input::Tick).1
            }

            DaemonMessage::DaemonConnected => self.handle_daemon_connected(),

            DaemonMessage::EventStreamConnected => {
                // Same connection-status handling as DaemonConnected, plus a
                // current-model re-fetch: the live event stream is now
                // subscribed, so re-read the daemon's authoritative model state
                // to capture anything that changed before this point (e.g. a
                // startup-load broadcast that fired before we subscribed). The
                // fetch is epoch-guarded, so it can't clobber a live event that
                // arrives while it's in flight.
                Task::batch([self.handle_daemon_connected(), self.fetch_current_model()])
            }

            DaemonMessage::DaemonError(_)
            | DaemonMessage::RetryConnection
            | DaemonMessage::WidgetBlocked(_)
            | DaemonMessage::RetryAuthorization
            | DaemonMessage::Connection(_) => self.handle_daemon_errors_retry(message),

            DaemonMessage::DaemonEventsReceived(_) => self.handle_daemon_events(message),
        }
    }

    /// Hand `input` to the Connection page's logic, and start the work it
    /// asks for. Returns what it came to, for the caller to read
    /// `reconnected` from.
    pub(in crate::core::app) fn connection(
        &mut self,
        input: Input,
    ) -> (Outcome, Task<cosmic::Action<Message>>) {
        let outcome = page::update(&mut self.connection, input);
        let tasks: Vec<_> = outcome
            .effects
            .iter()
            .map(|effect| self.connection_effect(effect))
            .collect();
        (outcome, Task::batch(tasks))
    }

    fn connection_effect(&mut self, effect: &Effect) -> Task<cosmic::Action<Message>> {
        match effect {
            Effect::Ping => ping_task(),
            Effect::RetryAfter(delay) => {
                let delay = *delay;
                Task::perform(tokio::time::sleep(delay), |()| {
                    cosmic::Action::App(Message::Daemon(DaemonMessage::RetryConnection))
                })
            }
            Effect::ForgetSession => {
                page::forget_session();
                Task::none()
            }
            Effect::Resubscribe => {
                // Bump the iced subscription key so the events task starts
                // again from scratch: the one before ended with `Blocked`.
                self.udp_restart_counter = self.udp_restart_counter.wrapping_add(1);
                Task::none()
            }
            Effect::RegisterAgents => {
                // The next status poll reports how it went.
                #[cfg(target_os = "macos")]
                std::thread::spawn(page::register_agents);
                Task::none()
            }
            Effect::OpenLoginItems => {
                #[cfg(target_os = "macos")]
                page::open_login_items();
                Task::none()
            }
        }
    }

    fn handle_daemon_connected(&mut self) -> Task<cosmic::Action<Message>> {
        // Connected: the backoff starts again from its short first wait on the
        // next drop. Whether it was down before is what the rest turns on.
        let was_disconnected = self.connection(Input::Reached).0.reconnected;
        // Only clear potentially stuck switching states on actual reconnect, not periodic pings
        if was_disconnected {
            self.device_state = DeviceState::Ready;
            // Every stage: an operation the app was tracking belongs to a
            // session that is gone.
            self.model_operations.reset();
            // It asks for nothing: it only forgets the test.
            let _ = super_voice_app_core::pages::recording_test::update(
                &mut self.recording_test,
                &mut self.microphone,
                super_voice_app_core::pages::recording_test::Input::Reconnected,
            );
        }

        // The /events subscription is self-healing
        // (`run_widget_subscription` in super-voice-shared owns
        // its own reconnect loop), so we deliberately do NOT
        // bump `udp_restart_counter` on reconnect. Restarting
        // the iced subscription would cancel the helper
        // mid-retry and cause another `session::obtain` round
        // — i.e. another read of the session-token file.
        if was_disconnected {
            info!("Daemon reconnected; events subscription is self-healing, no iced restart");
        }

        // Do NOT navigate on (re)connect: the launch page is Models (set in
        // `init.rs`), and yanking a mid-flow user to another page on every
        // daemon restart was both surprising and contradicted that default
        // (Tier 1 #18). Whatever page the user is on stays active.

        // Load settings/models/languages ONLY on the disconnected→connected
        // transition. Periodic keep-alive pings also resolve to `DaemonConnected`
        // (daemon/mod.rs `PingTimeout`), so re-fetching here unconditionally re-ran
        // six settings GETs + a language load every tick and clobbered optimistic
        // local edits (Tier 1 #14). Cross-client settings sync rides the SSE
        // `settings_changed` topic, not this poll.
        if was_disconnected {
            // Fresh connection — drop any banner left over from before the drop.
            self.action_error = None;
            Task::batch([
                self.handle_model_messages(ModelMessage::LoadInitialData),
                build_load_settings_tasks(),
                super::settings_page::load_language(),
            ])
        } else {
            Task::none()
        }
    }

    fn handle_daemon_errors_retry(
        &mut self,
        message: DaemonMessage,
    ) -> Task<cosmic::Action<Message>> {
        let input = match message {
            // A refusal stops the retries, so the app doesn't keep asking the
            // daemon and renewing its remembered refusal; any other failure
            // is tried again after a growing wait.
            DaemonMessage::DaemonError(err) => Input::Failed(err),
            DaemonMessage::RetryConnection => Input::Retry,
            DaemonMessage::WidgetBlocked(reason) => Input::Refused(reason),
            DaemonMessage::RetryAuthorization => Input::RetryAuthorization,
            DaemonMessage::Connection(input) => input,
            _ => return Task::none(),
        };
        self.connection(input).1
    }
}
