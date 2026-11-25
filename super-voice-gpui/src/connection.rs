// SPDX-License-Identifier: GPL-3.0-only
//! Whether the daemon answers: the core's Connection page
//! (`pages::connection`) run for the window, which shows it in its status
//! bar.
//!
//! The pattern every page follows: the model owns the core's state, an input
//! goes through the core's `update`, and each effect it returns is either
//! done here at once or run on the core's runtime, whose answer comes back as
//! the next input. The tasks it starts are kept in the model, so dropping the
//! model cancels them.

use std::time::Duration;

use gpui_kit::{AsyncApp, Context, Task, WeakEntity};
use super_voice_app_core::pages::connection::{self as page, Effect, Input};
use super_voice_app_core::state::connection::{ConnectionState, DaemonStatus};
use super_voice_shared::daemon::http_client::HttpError;

use crate::runtime;

/// How often a connected daemon is pinged, so a daemon that stops is noticed.
const KEEP_ALIVE: Duration = Duration::from_secs(5);

/// The connection, as a GPUI model.
pub struct Connection {
    state: ConnectionState,
    /// The ping in flight, or the wait before the next one. Replacing it
    /// cancels the one before.
    pending: Option<Task<()>>,
    /// The keep-alive timer, while the daemon answers.
    keep_alive: Option<Task<()>>,
}

impl Connection {
    /// Start asking the daemon.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut connection = Self::fixed(DaemonStatus::Disconnected);
        connection.send(Input::Retry, cx);
        connection
    }

    /// A connection that stays as `status` and asks the daemon nothing: for a
    /// screenshot, which must not reach the daemon on the desktop it is taken
    /// on (a new binary asking for a token brings up the consent dialog
    /// there).
    #[must_use]
    pub fn fixed(status: DaemonStatus) -> Self {
        Self {
            state: ConnectionState {
                status,
                ..ConnectionState::default()
            },
            pending: None,
            keep_alive: None,
        }
    }

    #[must_use]
    pub fn status(&self) -> &DaemonStatus {
        &self.state.status
    }

    /// Take in `input`: the core decides, this does what it says.
    pub fn send(&mut self, input: Input, cx: &mut Context<Self>) {
        let outcome = page::update(&mut self.state, input);
        for effect in outcome.effects {
            self.run(&effect, cx);
        }
        if self.state.is_connected() {
            if self.keep_alive.is_none() {
                self.keep_alive = Some(cx.spawn(keep_alive));
            }
        } else {
            self.keep_alive = None;
        }
        cx.notify();
    }

    fn run(&mut self, effect: &Effect, cx: &mut Context<Self>) {
        match effect {
            Effect::Ping => {
                self.pending = Some(cx.spawn(async move |this, cx| {
                    let input = runtime::spawn(page::run_ping())
                        .await
                        .unwrap_or_else(|e| Input::Failed(HttpError::Other(e.to_string())));
                    answer(&this, input, cx);
                }));
            }
            &Effect::RetryAfter(delay) => {
                self.pending = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(delay).await;
                    answer(&this, Input::Retry, cx);
                }));
            }
            Effect::ForgetSession => page::forget_session(),
            // The event stream and the macOS agent come with the pages that
            // use them.
            Effect::Resubscribe | Effect::RegisterAgents | Effect::OpenLoginItems => {
                log::debug!("not handled yet: {effect:?}");
            }
        }
    }
}

/// Hand an answer back to the model, if it is still there.
fn answer(this: &WeakEntity<Connection>, input: Input, cx: &mut AsyncApp) {
    if let Err(e) = this.update(cx, |connection, cx| connection.send(input, cx)) {
        log::debug!("the connection is gone: {e}");
    }
}

/// Ping again every [`KEEP_ALIVE`] while the daemon answers.
async fn keep_alive(this: WeakEntity<Connection>, cx: &mut AsyncApp) {
    loop {
        cx.background_executor().timer(KEEP_ALIVE).await;
        if this
            .update(cx, |connection, cx| connection.send(Input::Tick, cx))
            .is_err()
        {
            return;
        }
    }
}
