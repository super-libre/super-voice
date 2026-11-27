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
    /// The event stream dropped: the daemon may have stopped. Ping now,
    /// while connected, rather than at the next keep-alive.
    StreamDropped,
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
            state.failure = None;
            // The next drop starts again from the short first wait.
            state.retry.reset();
            Outcome {
                effects: Vec::new(),
                reconnected,
            }
        }
        Input::Failed(error) => {
            state.status = classify(error);
            state.failure = Some(state.status.clone());
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
        Input::Tick | Input::StreamDropped if state.is_connected() => {
            Outcome::effects(vec![Effect::Ping])
        }
        Input::Tick | Input::StreamDropped => Outcome::default(),
        Input::Retry => {
            state.status = DaemonStatus::Connecting;
            Outcome::effects(vec![Effect::Ping])
        }
        Input::Checked(Ok(())) => {
            state.status = DaemonStatus::Connected;
            state.failure = None;
            Outcome::default()
        }
        Input::Checked(Err(error)) => {
            state.status = classify(error);
            state.failure = Some(state.status.clone());
            Outcome::default()
        }
        Input::Refused(reason) => {
            log::warn!("Event stream blocked ({reason}); halting auto-retry");
            state.status = DaemonStatus::Blocked(reason);
            state.failure = Some(state.status.clone());
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

/// Whether `text`, a failure's words, says the daemon could not be reached:
/// its socket missing or refusing, or the connection dropped mid-call. While
/// the Connection page says so, an app needn't say it again for each call.
#[must_use]
pub fn says_unreachable(text: &str) -> bool {
    // The client's own words for each (super-engine-client's transport).
    const UNREACHABLE: [&str; 4] = [
        "Daemon HTTP listener not running",
        "Connection failed: ",
        "HTTP handshake failed: ",
        "Request failed: ",
    ];
    UNREACHABLE.iter().any(|words| text.contains(words))
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
    // While it tries again, the fix is still the failure's.
    let status = match (&state.status, &state.failure) {
        (DaemonStatus::Connecting, Some(failure)) => failure,
        (status, _) => status,
    };
    match status {
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
/// First it stops the agents an older Super Voice.app left running under the
/// old labels (see [`retire_old_agents`]), which a drag-install over that app
/// never stops. An enabled agent launchd refuses to start is registered again
/// ([`register_again_if_refused`]).
///
/// Blocking (`ServiceManagement` answers over XPC), so it runs off the UI
/// thread.
#[cfg(target_os = "macos")]
pub fn register_agents() {
    use super_voice_shared::launch_agents::{Agent, Status};
    retire_old_agents();
    for agent in Agent::ALL {
        match agent.status() {
            Status::NotRegistered => match agent.register() {
                Ok(()) => log::info!("registered {}", agent.label()),
                Err(e) => log::warn!("{e:#}"),
            },
            Status::Enabled => register_again_if_refused(agent),
            _ => {}
        }
    }
}

/// Register `agent` again when launchd holds it and won't start it.
///
/// After a switch between differently signed builds (ad hoc to team signed),
/// `ServiceManagement` says the agent is registered and enabled, and launchd
/// kills the job each time it starts with "Launch Constraint Violation"
/// (exit 78, "needs LWCR update"). Registering it again changes nothing; only
/// unregistering it first does. So an agent launchd reports not running, and
/// still not running a few seconds later, with that exit or that state, is
/// unregistered and registered once.
///
/// One `launchctl print` on a normal launch, which says the job is running.
#[cfg(target_os = "macos")]
fn register_again_if_refused(agent: super_voice_shared::launch_agents::Agent) {
    use std::process::Command;
    use std::time::Instant;

    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let job = format!("gui/{uid}/{}", agent.label());
    let print = || {
        Command::new("/bin/launchctl")
            .args(["print", job.as_str()])
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let Some(mut shown) = print() else {
        return;
    };
    // launchd may be starting it, or waiting out its throttle: a few seconds.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !job_running(&shown) {
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
        match print() {
            Some(again) => shown = again,
            None => return,
        }
    }
    if job_running(&shown) || !refused_at_launch(&shown) {
        return;
    }
    log::warn!("launchd won't start {job} (a launch constraint, exit 78); registering it again");
    if let Err(e) = agent.unregister() {
        log::warn!("{e:#}");
        return;
    }
    match agent.register() {
        Ok(()) => log::info!("registered {} again", agent.label()),
        Err(e) => log::warn!("{e:#}"),
    }
}

/// Whether `launchctl print` says the job is running.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn job_running(print: &str) -> bool {
    print
        .lines()
        .any(|line| line.trim().replace(' ', "") == "state=running")
}

/// Whether `launchctl print` says launchd refused to run the job for its
/// launch constraints: exit 78 (`EX_CONFIG`, what a constraint violation
/// leaves), or the "needs LWCR update" state.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn refused_at_launch(print: &str) -> bool {
    let print = print.to_ascii_lowercase();
    print.contains("lwcr")
        || print.contains("launch constraint")
        || print.lines().any(|line| {
            line.trim()
                .strip_prefix("last exit code")
                .and_then(|rest| rest.trim_start().strip_prefix('='))
                .is_some_and(|code| {
                    code.trim_start()
                        .split(|c: char| !c.is_ascii_digit())
                        .next()
                        == Some("78")
                })
        })
}

/// Boot out each agent launchd still holds under an old label, and wait up to
/// 5 s for launchd to let go of them: until then the old daemon still holds
/// the socket. Labels launchd doesn't have are skipped, so this costs one
/// `launchctl print` per label on a Mac with no old install. Files in
/// `~/Library/LaunchAgents` are left alone; the old plists lived inside the
/// old bundle.
#[cfg(target_os = "macos")]
fn retire_old_agents() {
    use std::process::{Command, Stdio};
    use std::time::Instant;
    use super_voice_shared::launch_agents::OLD_LABELS;

    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let loaded = |job: &str| {
        Command::new("/bin/launchctl")
            .args(["print", job])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };

    let old: Vec<String> = OLD_LABELS
        .into_iter()
        .map(|label| format!("gui/{uid}/{label}"))
        .filter(|job| loaded(job))
        .collect();
    if old.is_empty() {
        return;
    }
    for job in &old {
        log::info!("stopping {job}, an agent an older Super Voice left running");
        match Command::new("/bin/launchctl")
            .args(["bootout", job.as_str()])
            .output()
        {
            Ok(out) if !out.status.success() => log::warn!(
                "launchctl bootout {job}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            Ok(_) => {}
            Err(e) => log::warn!("could not run launchctl bootout {job}: {e}"),
        }
    }
    // The new daemon can't serve while the old one holds the socket, so wait
    // until launchd lets go of it, as `just install-macos` does.
    let deadline = Instant::now() + Duration::from_secs(5);
    while old.iter().any(|job| loaded(job)) {
        if Instant::now() >= deadline {
            log::warn!("launchd still runs an older Super Voice's agents after 5 s");
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    log::info!("stopped the agents an older Super Voice left running");
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

    /// What `launchctl print` shows of an agent killed for its launch
    /// constraints, and of one running.
    const REFUSED: &str = "gui/501/org.superlibre.super-voice = {
\tactive count = 0
\tpath = /Applications/Super Voice.app/Contents/Library/LaunchAgents/org.superlibre.super-voice.plist
\tstate = not running
\tlast exit code = 78: EX_CONFIG
\tjob state = exited
\tproperties = needs LWCR update | inferred program
}";
    const RUNNING: &str = "gui/501/org.superlibre.super-voice = {
\tactive count = 1
\tstate = running
\tpid = 4242
\tlast exit code = (never exited)
}";

    #[test]
    fn an_agent_refused_for_its_launch_constraints_is_told_apart() {
        assert!(!job_running(REFUSED));
        assert!(refused_at_launch(REFUSED));
        assert!(job_running(RUNNING));
        assert!(!refused_at_launch(RUNNING));
        // Exit 78 alone is enough; another exit is not.
        assert!(refused_at_launch(
            "\tstate = not running\n\tlast exit code = 78: EX_CONFIG\n"
        ));
        assert!(!refused_at_launch(
            "\tstate = not running\n\tlast exit code = 1\n"
        ));
        assert!(!refused_at_launch("\tlast exit code = 780\n"));
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
    fn a_dropped_stream_pings_at_once_while_connected() {
        let mut state = ConnectionState::default();
        update(&mut state, Input::Reached);
        assert_eq!(
            update(&mut state, Input::StreamDropped).effects,
            [Effect::Ping]
        );
        state.status = DaemonStatus::Error("gone".into());
        assert!(update(&mut state, Input::StreamDropped).effects.is_empty());
    }

    #[test]
    fn a_failure_that_says_the_daemon_is_gone_is_told_apart() {
        assert!(says_unreachable(
            "The clip could not be made: Daemon HTTP listener not running. Start the daemon first."
        ));
        assert!(says_unreachable(
            "Connection failed: connection reset by peer"
        ));
        assert!(says_unreachable("Request failed: connection closed"));
        assert!(!says_unreachable("The note could not be saved: disk full"));
        assert!(!says_unreachable("Unknown voice"));
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

    /// The window shows the Connection page from the first failure until the
    /// daemon answers, and not while it tries again in between.
    #[test]
    fn the_daemon_is_down_from_a_failure_until_it_answers() {
        let mut state = ConnectionState::default();
        assert!(!state.is_down(), "not before the first answer");
        update(&mut state, Input::Retry);
        assert!(!state.is_down());
        update(
            &mut state,
            Input::Failed(HttpError::Other("no socket".into())),
        );
        assert!(state.is_down());
        update(&mut state, Input::Retry);
        assert_eq!(state.status, DaemonStatus::Connecting);
        assert!(state.is_down(), "trying again doesn't bring the pages back");
        update(&mut state, Input::Reached);
        assert!(!state.is_down());
        update(&mut state, Input::Refused("user_denied".into()));
        assert!(state.is_down());
        update(&mut state, Input::RetryAuthorization);
        assert!(state.is_down());
        update(&mut state, Input::Checked(Ok(())));
        assert!(!state.is_down());
        update(
            &mut state,
            Input::Checked(Err(HttpError::Other("gone".into()))),
        );
        assert!(state.is_down());
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

        // While it tries again after a refusal, the fix is the refusal's.
        let mut state = ConnectionState::default();
        update(&mut state, Input::Refused("user_denied".into()));
        update(&mut state, Input::RetryAuthorization);
        assert_eq!(state.status, DaemonStatus::Connecting);
        assert!(matches!(fix(&state), Some(Fix::Reauthorize { .. })));
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
