// SPDX-License-Identifier: GPL-3.0-only
//! Whether the daemon answers: the core's Connection page
//! (`pages::connection`) run for the window, which shows it in its status
//! bar, and shows the Connection page in place of the pages that need the
//! daemon while it is down.
//!
//! The pattern every model follows: it owns the core's state, an input goes
//! through the core's `update`, and each effect it returns is either done
//! here at once or run on the core's runtime, whose answer comes back as the
//! next input. What the window must hear of (the daemon answering again,
//! going down, the event stream to start again) it emits as a
//! [`ConnectionEvent`].

use std::time::Duration;

use gpui_kit::{AsyncApp, Context, EventEmitter, Task, WeakEntity};
use super_voice_app_core::pages::connection::{self as page, Effect, Fix, Input};
use super_voice_app_core::state::connection::{ConnectionState, DaemonStatus};
use super_voice_shared::daemon::http_client::HttpError;

use crate::runtime;

/// How often a connected daemon is pinged, so a daemon that stops is noticed.
const KEEP_ALIVE: Duration = Duration::from_secs(5);

/// What the window hears from the connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionEvent {
    /// The daemon answers, first or again: read what it shows.
    Reached,
    /// The daemon stopped answering, or refused the app.
    Lost,
    /// Start the daemon's event stream again: it ended when the user refused
    /// the app.
    Resubscribe,
}

/// The connection, as a GPUI model.
pub struct Connection {
    state: ConnectionState,
    /// The ping in flight, or the wait before the next one. Replacing it
    /// cancels the one before.
    pending: Option<Task<()>>,
    /// The keep-alive timer, while the daemon answers.
    keep_alive: Option<Task<()>>,
}

impl EventEmitter<ConnectionEvent> for Connection {}

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
    /// there). An error or a refusal is down, as after a failed ping.
    #[must_use]
    pub fn fixed(status: DaemonStatus) -> Self {
        let failure = matches!(status, DaemonStatus::Error(_) | DaemonStatus::Blocked(_))
            .then(|| status.clone());
        Self {
            state: ConnectionState {
                status,
                failure,
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

    /// Whether the window shows the Connection page in place of the pages
    /// that need the daemon.
    #[must_use]
    pub fn is_down(&self) -> bool {
        self.state.is_down()
    }

    /// Why the daemon is down, while it is: the failure, even while the app
    /// tries again.
    #[must_use]
    pub fn failure(&self) -> Option<&DaemonStatus> {
        self.state.failure.as_ref()
    }

    /// What the Connection page offers to fix it.
    #[must_use]
    pub fn fix(&self) -> Option<Fix> {
        page::fix(&self.state)
    }

    /// Take in `input`: the core decides, this does what it says.
    pub fn send(&mut self, input: Input, cx: &mut Context<Self>) {
        let was_down = self.state.is_down();
        let outcome = page::update(&mut self.state, input);
        for effect in outcome.effects {
            self.run(&effect, cx);
        }
        if outcome.reconnected {
            cx.emit(ConnectionEvent::Reached);
        }
        if self.state.is_down() && !was_down {
            cx.emit(ConnectionEvent::Lost);
            // On macOS launchd runs the daemon only while its agent is
            // registered and switched on: read which, for the fix to offer.
            #[cfg(target_os = "macos")]
            cx.spawn(async move |this, cx| {
                let read =
                    runtime::spawn(async { tokio::task::spawn_blocking(page::read_agent).await })
                        .await;
                if let Ok(Ok(agent)) = read {
                    answer(&this, Input::AgentRead(agent), cx);
                }
            })
            .detach();
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
            Effect::Resubscribe => cx.emit(ConnectionEvent::Resubscribe),
            #[cfg(target_os = "macos")]
            Effect::RegisterAgents => {
                cx.spawn(async move |this, cx| {
                    let read = runtime::spawn(async {
                        tokio::task::spawn_blocking(|| {
                            page::register_agents();
                            page::read_agent()
                        })
                        .await
                    })
                    .await;
                    if let Ok(Ok(agent)) = read {
                        answer(&this, Input::AgentRead(agent), cx);
                    }
                })
                .detach();
            }
            #[cfg(target_os = "macos")]
            Effect::OpenLoginItems => page::open_login_items(),
            #[cfg(not(target_os = "macos"))]
            Effect::RegisterAgents | Effect::OpenLoginItems => {
                log::debug!("only macOS has the daemon's agent: {effect:?}");
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
