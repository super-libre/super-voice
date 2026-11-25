// SPDX-License-Identifier: GPL-3.0-only
//! The Connection page: whether the daemon answers, trying again while it
//! doesn't, and what the user can do about it.
//!
//! An app shows the page in place of its window's content while the daemon
//! cannot be reached, and from its View menu at any time.

use std::time::Duration;

use super_voice_shared::daemon::http_client::HttpError;
use super_voice_shared::daemon::widget_subscription::is_user_denied;

use crate::state::connection::{AgentStatus, ConnectionState, DaemonStatus};

/// What happened.
#[derive(Debug, Clone)]
pub enum Input {
    /// The daemon answered a ping, or the event stream connected.
    Reached,
    /// A ping failed.
    Failed(HttpError),
    /// The keep-alive timer: ping again while connected.
    Tick,
    /// The wait after a failure is over: try again.
    Retry,
    /// The status was asked for again (the Settings page's refresh), and this
    /// is the answer. It only says what the status is: nothing is tried again.
    Checked(Result<(), HttpError>),
    /// The event stream was refused by the user, for good.
    Refused(String),
    /// Retry authorization was pressed.
    RetryAuthorization,
    /// The daemon's agent was read.
    AgentRead(AgentStatus),
    /// Register was pressed.
    Register,
    /// Open Login Items was pressed.
    OpenLoginItems,
}

/// What the app should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Ping the daemon ([`run_ping`]).
    Ping,
    /// Wait this long, then send [`Input::Retry`].
    RetryAfter(Duration),
    /// Forget this app's token ([`forget_session`]), so the next call asks the
    /// user again.
    ForgetSession,
    /// Start the event stream again: it ended when the user refused the app.
    Resubscribe,
    /// Register the daemon's agents with macOS. The next read of the agent
    /// says how it went.
    RegisterAgents,
    /// Open System Settings at Login Items.
    OpenLoginItems,
}

/// What an input came to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub effects: Vec<Effect>,
    /// The daemon answers again after it didn't: whatever was shown from it
    /// is read again.
    pub reconnected: bool,
}

impl Outcome {
    fn effects(effects: Vec<Effect>) -> Self {
        Self {
            effects,
            reconnected: false,
        }
    }
}

/// Take in `input`, and say what to do.
pub fn update(state: &mut ConnectionState, input: Input) -> Outcome {
    match input {
        Input::Reached => {
            let reconnected = !state.is_connected();
            state.status = DaemonStatus::Connected;
            // The next drop starts again from the short first wait.
            state.retry.reset();
            Outcome {
                effects: Vec::new(),
                reconnected,
            }
        }
        Input::Failed(error) => {
            state.status = classify(error);
            if matches!(state.status, DaemonStatus::Blocked(_)) {
                log::warn!(
                    "Daemon access blocked by user denial; auto-retry suppressed: {:?}",
                    state.status
                );
                return Outcome::default();
            }
            let delay = state.retry.next_delay();
            state.retry.should_retry();
            Outcome::effects(vec![Effect::RetryAfter(delay)])
        }
        Input::Tick if state.is_connected() => Outcome::effects(vec![Effect::Ping]),
        Input::Tick => Outcome::default(),
        Input::Retry => {
            state.status = DaemonStatus::Connecting;
            Outcome::effects(vec![Effect::Ping])
        }
        Input::Checked(Ok(())) => {
            state.status = DaemonStatus::Connected;
            Outcome::default()
        }
        Input::Checked(Err(error)) => {
            state.status = classify(error);
            Outcome::default()
        }
        Input::Refused(reason) => {
            log::warn!("Event stream blocked ({reason}); halting auto-retry");
            state.status = DaemonStatus::Blocked(reason);
            Outcome::default()
        }
        Input::RetryAuthorization => {
            log::info!("Retrying authorization after user denial");
            state.status = DaemonStatus::Connecting;
            Outcome::effects(vec![
                Effect::ForgetSession,
                Effect::Resubscribe,
                Effect::Ping,
            ])
        }
        Input::AgentRead(agent) => {
            state.agent = Some(agent);
            Outcome::default()
        }
        Input::Register => Outcome::effects(vec![Effect::RegisterAgents]),
        Input::OpenLoginItems => Outcome::effects(vec![Effect::OpenLoginItems]),
    }
}

/// The status a failed call leaves the connection in.
///
/// The user refusing the app ([`is_user_denied`]) is [`DaemonStatus::Blocked`],
/// which stops the app trying again: every try would only have the daemon
/// remember the refusal longer. Every other failure is passing (the daemon
/// restarting, its socket missing, a token expired) and is
/// [`DaemonStatus::Error`], which the app tries again after. The decision is on
/// the error's kind; its words are kept only to show.
#[must_use]
pub fn classify(error: HttpError) -> DaemonStatus {
    if is_user_denied(&error) {
        DaemonStatus::Blocked(error.into())
    } else {
        DaemonStatus::Error(error.into())
    }
}

/// What the page offers to fix the connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// The daemon isn't running: the command that starts it, to copy.
    Start { command: String },
    /// The daemon's agent is switched off in Login Items: Open Login Items.
    TurnOn,
    /// The daemon's agent is not registered: Register.
    Register,
    /// The user refused the app: restart the daemon with `restart`, which
    /// forgets the refusal, then Retry authorization.
    Reauthorize { restart: String },
}

/// What the page offers for the connection as `state` has it, or `None` when
/// there is nothing to fix.
///
/// The app and the daemon install together, so a daemon that doesn't answer
/// is one that isn't running. On macOS launchd runs it only while its agent
/// is registered and switched on, which a command fixes neither of.
#[must_use]
pub fn fix(state: &ConnectionState) -> Option<Fix> {
    match &state.status {
        DaemonStatus::Disconnected | DaemonStatus::Error(_) => Some(match state.agent {
            Some(AgentStatus::RequiresApproval) => Fix::TurnOn,
            Some(AgentStatus::NotRegistered) => Fix::Register,
            _ => Fix::Start {
                command: start_command(),
            },
        }),
        DaemonStatus::Blocked(_) => Some(Fix::Reauthorize {
            restart: restart_command(),
        }),
        DaemonStatus::Connecting | DaemonStatus::Connected => None,
    }
}

/// The command that starts the daemon.
///
/// Built when asked rather than kept in a `const`, because the launchd form
/// needs the user's id: `launchctl` addresses a user agent by its domain
/// target (`gui/<uid>/<label>`), never by its label alone.
#[must_use]
pub fn start_command() -> String {
    #[cfg(target_os = "macos")]
    {
        format!("launchctl kickstart {}", launchd_target())
    }
    #[cfg(not(target_os = "macos"))]
    {
        "systemctl --user start super-voice".to_string()
    }
}

/// The command that restarts a running daemon. `-k` stops the running one
/// first: a plain `kickstart` of a running agent does nothing, which would
/// leave the remembered refusal in place.
#[must_use]
pub fn restart_command() -> String {
    #[cfg(target_os = "macos")]
    {
        format!("launchctl kickstart -k {}", launchd_target())
    }
    #[cfg(not(target_os = "macos"))]
    {
        "systemctl --user restart super-voice".to_string()
    }
}

/// The daemon agent's domain target. Matches the label in
/// `super-voice-daemon/launchd/` and the `launchd_target` the justfile builds.
#[cfg(target_os = "macos")]
fn launchd_target() -> String {
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    format!(
        "gui/{uid}/{}",
        super_voice_shared::launch_agents::Agent::Daemon.label()
    )
}

/// The socket the app reaches the daemon on, as the page shows it.
#[must_use]
pub fn socket_path() -> String {
    super_voice_shared::validation::get_http_socket_path()
        .to_string_lossy()
        .into_owned()
}

/// Register each of the bundle's agents that has never been registered,
/// which starts it and runs it at every login.
///
/// This is what installs the daemon for someone who drags Super Voice into
/// Applications and opens it, so an app runs it on every launch rather than
/// once. An agent switched off in Login Items is left alone: that was the
/// user's choice, and only they can undo it. Outside the bundle there is no
/// agent to register, and nothing is done.
///
/// Blocking (`ServiceManagement` answers over XPC), so it runs off the UI
/// thread.
#[cfg(target_os = "macos")]
pub fn register_agents() {
    use super_voice_shared::launch_agents::{Agent, Status};
    for agent in Agent::ALL {
        if agent.status() != Status::NotRegistered {
            continue;
        }
        match agent.register() {
            Ok(()) => log::info!("registered {}", agent.label()),
            Err(e) => log::warn!("{e:#}"),
        }
    }
}

/// Where the daemon's agent stands now. Blocking, as [`register_agents`].
#[cfg(target_os = "macos")]
#[must_use]
pub fn read_agent() -> AgentStatus {
    super_voice_shared::launch_agents::Agent::Daemon
        .status()
        .into()
}

/// Open System Settings at General › Login Items, the one place an agent the
/// user switched off goes back on.
#[cfg(target_os = "macos")]
pub fn open_login_items() {
    super_voice_shared::launch_agents::open_login_items_settings();
}

/// Ping the daemon.
pub async fn run_ping() -> Input {
    match crate::daemon::client::ping_daemon().await {
        Ok(_) => Input::Reached,
        Err(error) => Input::Failed(error),
    }
}

/// Ask whether the daemon answers, only to show it.
pub async fn run_check() -> Input {
    Input::Checked(crate::daemon::client::test_daemon_connection().await)
}

/// Forget this app's token, so the next call asks the user again; after a
/// daemon restart, that brings up a new consent dialog.
pub fn forget_session() {
    if let Err(e) = super_voice_shared::daemon::session::forget(crate::daemon::client::app_id()) {
        log::warn!("Failed to forget the session before retrying: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth_denied(reason: &str) -> HttpError {
        HttpError::AuthDenied {
            reason: reason.to_string(),
        }
    }

    fn refused() -> HttpError {
        auth_denied("user_denied")
    }

    #[test]
    fn user_denied_cached_routes_to_blocked() {
        let next = classify(auth_denied("user_denied_cached"));
        match next {
            DaemonStatus::Blocked(reason) => {
                // Blocked keeps the human string for display.
                assert_eq!(reason, "auth_denied (user_denied_cached)");
            }
            other => panic!("user_denied_cached must route to Blocked, got {other:?}"),
        }
    }

    #[test]
    fn fresh_user_denied_routes_to_blocked() {
        assert!(matches!(classify(refused()), DaemonStatus::Blocked(_)));
    }

    #[test]
    fn dismissed_popup_routes_to_error_so_retry_can_recover() {
        // user_dismissed is recoverable (the next attempt brings the dialog
        // up again), so the app must keep trying.
        let next = classify(auth_denied("user_dismissed"));
        assert!(matches!(next, DaemonStatus::Error(_)));
    }

    #[test]
    fn invalid_session_routes_to_error() {
        // Token expiry and exe_changed pass: the next attempt asks again.
        let next = classify(HttpError::InvalidSession {
            reason: "expired".to_string(),
        });
        assert!(matches!(next, DaemonStatus::Error(_)));
    }

    #[test]
    fn socket_unreachable_routes_to_error() {
        // The daemon restarting, or its socket missing.
        let next = classify(HttpError::Other(
            "Daemon HTTP listener not running. Start the daemon first.".to_string(),
        ));
        assert!(matches!(next, DaemonStatus::Error(_)));
    }

    #[test]
    fn a_passing_failure_is_tried_again_and_a_refusal_is_not() {
        let mut state = ConnectionState::default();
        let failed = update(
            &mut state,
            Input::Failed(HttpError::Other("no socket".to_string())),
        );
        assert!(matches!(state.status, DaemonStatus::Error(_)));
        assert!(matches!(failed.effects[..], [Effect::RetryAfter(_)]));

        let blocked = update(&mut state, Input::Failed(refused()));
        assert!(matches!(state.status, DaemonStatus::Blocked(_)));
        assert_eq!(blocked, Outcome::default());
    }

    #[test]
    fn reaching_the_daemon_after_a_drop_is_a_reconnection() {
        let mut state = ConnectionState::default();
        assert!(update(&mut state, Input::Reached).reconnected);
        assert!(state.is_connected());
        // A keep-alive ping that answers is not.
        assert!(!update(&mut state, Input::Reached).reconnected);
        assert_eq!(update(&mut state, Input::Tick).effects, [Effect::Ping]);
        state.status = DaemonStatus::Error("gone".into());
        assert!(update(&mut state, Input::Tick).effects.is_empty());
        let retry = update(&mut state, Input::Retry);
        assert_eq!(state.status, DaemonStatus::Connecting);
        assert_eq!(retry.effects, [Effect::Ping]);
    }

    #[test]
    fn retrying_authorization_asks_again_from_scratch() {
        let mut state = ConnectionState::default();
        update(&mut state, Input::Refused("user_denied".into()));
        assert_eq!(state.status, DaemonStatus::Blocked("user_denied".into()));
        let retry = update(&mut state, Input::RetryAuthorization);
        assert_eq!(state.status, DaemonStatus::Connecting);
        assert_eq!(
            retry.effects,
            [Effect::ForgetSession, Effect::Resubscribe, Effect::Ping]
        );
    }

    #[test]
    fn a_status_check_only_shows_the_status() {
        let mut state = ConnectionState::default();
        assert_eq!(
            update(&mut state, Input::Checked(Ok(()))),
            Outcome::default()
        );
        assert!(state.is_connected());
        let failed = update(
            &mut state,
            Input::Checked(Err(HttpError::Other("gone".into()))),
        );
        assert_eq!(failed, Outcome::default());
        assert!(matches!(state.status, DaemonStatus::Error(_)));
    }

    #[test]
    fn the_fix_follows_the_status_and_the_agent() {
        let mut state = ConnectionState::default();
        assert!(matches!(fix(&state), Some(Fix::Start { .. })));
        state.agent = Some(AgentStatus::RequiresApproval);
        assert_eq!(fix(&state), Some(Fix::TurnOn));
        state.agent = Some(AgentStatus::NotRegistered);
        assert_eq!(fix(&state), Some(Fix::Register));
        state.agent = Some(AgentStatus::Enabled);
        assert_eq!(
            fix(&state),
            Some(Fix::Start {
                command: start_command()
            })
        );
        state.status = DaemonStatus::Blocked("user_denied".into());
        assert_eq!(
            fix(&state),
            Some(Fix::Reauthorize {
                restart: restart_command()
            })
        );
        state.status = DaemonStatus::Connecting;
        assert_eq!(fix(&state), None);
        state.status = DaemonStatus::Connected;
        assert_eq!(fix(&state), None);
    }

    #[test]
    fn the_buttons_ask_for_their_work() {
        let mut state = ConnectionState::default();
        assert_eq!(
            update(&mut state, Input::Register).effects,
            [Effect::RegisterAgents]
        );
        assert_eq!(
            update(&mut state, Input::OpenLoginItems).effects,
            [Effect::OpenLoginItems]
        );
        update(&mut state, Input::AgentRead(AgentStatus::Enabled));
        assert_eq!(state.agent, Some(AgentStatus::Enabled));
    }
}
