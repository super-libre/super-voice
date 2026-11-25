// SPDX-License-Identifier: GPL-3.0-only
//! The Connection page for an app written in another language: a store that
//! keeps asking whether the daemon answers, runs the core's logic for it
//! (`pages::connection`), and tells the app whenever the page changed. The
//! app shows the page in place of its window's content while the daemon
//! can't be reached.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use super_voice_app_core::events::Event;
use super_voice_app_core::pages::connection::{self as page, Effect, Fix, Input};
use super_voice_app_core::state::connection::{ConnectionState, DaemonStatus};
use tokio::sync::broadcast::error::RecvError;

use crate::RUNTIME;

/// How often a connected daemon is pinged, as the settings app does.
const KEEP_ALIVE: Duration = Duration::from_secs(5);

/// How often the daemon's agent is read while the daemon can't be reached.
#[cfg(target_os = "macos")]
const AGENT_POLL: Duration = Duration::from_secs(3);

/// What the app is told. Called from the store's own threads.
#[uniffi::export(with_foreign)]
pub trait ConnectionObserver: Send + Sync {
    /// The page changed; draw this.
    fn changed(&self, page: ConnectionPage);
}

/// Whether the daemon answers.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectionStatus {
    /// Not asked yet: the first ping is on its way.
    Disconnected,
    Connecting,
    Connected,
    /// It didn't answer, for this reason. The store tries again by itself.
    Error {
        message: String,
    },
    /// The user refused the app. Nothing is tried again until Retry
    /// authorization.
    Denied {
        reason: String,
    },
}

/// What the page offers to fix the connection.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectionFix {
    /// The daemon isn't running: the command that starts it, to copy.
    Start { command: String },
    /// The daemon's background service is switched off in Login Items: Open
    /// Login Items.
    TurnOn,
    /// The daemon's background service isn't registered: Register.
    Register,
    /// The user refused the app: restart the daemon with `restart`, then
    /// Retry authorization.
    Reauthorize { restart: String },
}

/// Everything the page shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConnectionPage {
    pub status: ConnectionStatus,
    /// The socket the app reaches the daemon on.
    pub socket_path: String,
    /// What to offer, when anything.
    pub fix: Option<ConnectionFix>,
}

/// What the user did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectionAction {
    RetryAuthorization,
    /// Register the daemon's background service.
    Register,
    /// Open System Settings at Login Items.
    OpenLoginItems,
}

/// The connection's store. Asks the daemon until it is dropped.
#[derive(uniffi::Object)]
pub struct ConnectionStore {
    held: Mutex<ConnectionState>,
    observer: Arc<dyn ConnectionObserver>,
}

#[uniffi::export]
impl ConnectionStore {
    /// A store that tells `observer` what the page shows, now and whenever it
    /// changes, and starts asking the daemon.
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn ConnectionObserver>) -> Arc<Self> {
        crate::identify();
        let store = Arc::new(Self {
            held: Mutex::new(ConnectionState::default()),
            observer,
        });
        // The first ping leaves the status as it is (not asked yet) until it
        // answers, so a daemon that answers at once never shows the page.
        Arc::clone(&store).run(&Effect::Ping);
        keep_alive(Arc::downgrade(&store));
        follow(Arc::downgrade(&store));
        #[cfg(target_os = "macos")]
        read_agent(Arc::downgrade(&store));
        store
    }

    pub fn page(&self) -> ConnectionPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: ConnectionAction) {
        self.dispatch(match action {
            ConnectionAction::RetryAuthorization => Input::RetryAuthorization,
            ConnectionAction::Register => Input::Register,
            ConnectionAction::OpenLoginItems => Input::OpenLoginItems,
        });
    }
}

impl ConnectionStore {
    fn lock(&self) -> MutexGuard<'_, ConnectionState> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(target_os = "macos")]
    fn connected(&self) -> bool {
        self.lock().is_connected()
    }

    /// Hand `input` to the page's logic, tell the app when the page changed,
    /// and start the work it asked for.
    fn dispatch(self: Arc<Self>, input: Input) {
        let (before, after, effects) = {
            let mut held = self.lock();
            let before = snapshot(&held);
            let outcome = page::update(&mut held, input);
            (before, snapshot(&held), outcome.effects)
        };
        if before != after {
            self.observer.changed(after);
        }
        for effect in &effects {
            Arc::clone(&self).run(effect);
        }
    }

    fn run(self: Arc<Self>, effect: &Effect) {
        match effect {
            Effect::Ping => {
                RUNTIME.spawn(async move {
                    let input = page::run_ping().await;
                    self.dispatch(input);
                });
            }
            Effect::RetryAfter(delay) => {
                let delay = *delay;
                let store = Arc::downgrade(&self);
                RUNTIME.spawn(async move {
                    tokio::time::sleep(delay).await;
                    if let Some(store) = store.upgrade() {
                        store.dispatch(Input::Retry);
                    }
                });
            }
            Effect::ForgetSession => page::forget_session(),
            Effect::Resubscribe => crate::events::resubscribe(),
            Effect::RegisterAgents => register_launch_agents(),
            // On the caller's thread: the button's, the app's main thread.
            Effect::OpenLoginItems => {
                #[cfg(target_os = "macos")]
                page::open_login_items();
            }
        }
    }
}

/// Ping the daemon every [`KEEP_ALIVE`], while it answers, so a daemon that
/// goes away is noticed.
fn keep_alive(store: Weak<ConnectionStore>) {
    RUNTIME.spawn(async move {
        loop {
            tokio::time::sleep(KEEP_ALIVE).await;
            let Some(store) = store.upgrade() else {
                return;
            };
            store.dispatch(Input::Tick);
        }
    });
}

/// Follow the daemon's event stream: connected again, or refused by the user.
fn follow(store: Weak<ConnectionStore>) {
    let mut events = crate::events::listen();
    RUNTIME.spawn(async move {
        loop {
            let input = match events.recv().await {
                Ok(Event::Connected) => Input::Reached,
                Ok(Event::Blocked(reason)) => Input::Refused(reason),
                Ok(_) | Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            };
            let Some(store) = store.upgrade() else {
                return;
            };
            store.dispatch(input);
        }
    });
}

/// Read the daemon's agent every [`AGENT_POLL`] while the daemon can't be
/// reached, for what the page offers.
#[cfg(target_os = "macos")]
fn read_agent(store: Weak<ConnectionStore>) {
    RUNTIME.spawn(async move {
        loop {
            let Some(connected) = store.upgrade().map(|store| store.connected()) else {
                return;
            };
            if !connected {
                let Ok(agent) = tokio::task::spawn_blocking(page::read_agent).await else {
                    return;
                };
                let Some(store) = store.upgrade() else {
                    return;
                };
                store.dispatch(Input::AgentRead(agent));
            }
            tokio::time::sleep(AGENT_POLL).await;
        }
    });
}

fn snapshot(state: &ConnectionState) -> ConnectionPage {
    ConnectionPage {
        status: match &state.status {
            DaemonStatus::Disconnected => ConnectionStatus::Disconnected,
            DaemonStatus::Connecting => ConnectionStatus::Connecting,
            DaemonStatus::Connected => ConnectionStatus::Connected,
            DaemonStatus::Error(message) => ConnectionStatus::Error {
                message: message.clone(),
            },
            DaemonStatus::Blocked(reason) => ConnectionStatus::Denied {
                reason: reason.clone(),
            },
        },
        socket_path: page::socket_path(),
        fix: page::fix(state).map(|fix| match fix {
            Fix::Start { command } => ConnectionFix::Start { command },
            Fix::TurnOn => ConnectionFix::TurnOn,
            Fix::Register => ConnectionFix::Register,
            Fix::Reauthorize { restart } => ConnectionFix::Reauthorize { restart },
        }),
    }
}

/// Register each of the app bundle's background services (the daemon and the
/// shortcut listener) that has never been registered, which starts it and
/// runs it at every login: what installs the daemon for someone who drags
/// the app into Applications and opens it. Called at every launch. Returns at
/// once; the work runs on a thread of its own. Does nothing outside macOS,
/// or outside a bundle that carries the services.
#[uniffi::export]
pub fn register_launch_agents() {
    #[cfg(target_os = "macos")]
    std::thread::spawn(page::register_agents);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_offers_to_ask_again() {
        let state = ConnectionState {
            status: DaemonStatus::Blocked("auth_denied (user_denied)".into()),
            ..ConnectionState::default()
        };
        let page = snapshot(&state);
        assert_eq!(
            page.status,
            ConnectionStatus::Denied {
                reason: "auth_denied (user_denied)".into()
            }
        );
        assert!(matches!(page.fix, Some(ConnectionFix::Reauthorize { .. })));
    }

    #[test]
    fn a_daemon_that_does_not_answer_offers_its_start_command() {
        let state = ConnectionState {
            status: DaemonStatus::Error("gone".into()),
            ..ConnectionState::default()
        };
        assert_eq!(
            snapshot(&state).fix,
            Some(ConnectionFix::Start {
                command: page::start_command()
            })
        );
    }
}
