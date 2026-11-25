// SPDX-License-Identifier: GPL-3.0-only
//! The app's connection to the daemon: whether it answers, and, on macOS,
//! where the daemon's background agent stands.

use super_voice_shared::daemon::retry::RetryStrategy;

/// Whether the daemon answers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DaemonStatus {
    /// Not asked yet.
    #[default]
    Disconnected,
    Connecting,
    Connected,
    /// It did not answer, for this reason. The app tries again by itself.
    Error(String),
    /// The user refused this app access, now or through the daemon's
    /// remembered answer. Nothing is tried again until the user asks to
    /// (Retry authorization), usually after restarting the daemon, which
    /// forgets the answer. Trying again by itself would only make the daemon
    /// remember it longer.
    Blocked(String),
}

/// Where the daemon's background agent stands with macOS: the bundle's
/// `LaunchAgent`, which is what runs the daemon there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    /// Never registered, or unregistered since.
    NotRegistered,
    /// Registered and allowed to run.
    Enabled,
    /// Registered, but switched off in Login Items. Only the user can turn it
    /// back on.
    RequiresApproval,
    /// The app is not running from its bundle, so it has no agent.
    NotInBundle,
}

#[cfg(target_os = "macos")]
impl From<super_voice_shared::launch_agents::Status> for AgentStatus {
    fn from(status: super_voice_shared::launch_agents::Status) -> Self {
        use super_voice_shared::launch_agents::Status;
        match status {
            Status::NotRegistered => Self::NotRegistered,
            Status::Enabled => Self::Enabled,
            Status::RequiresApproval => Self::RequiresApproval,
            Status::NotInBundle => Self::NotInBundle,
        }
    }
}

/// The connection's state.
#[derive(Debug, Clone)]
pub struct ConnectionState {
    pub status: DaemonStatus,
    /// The daemon's agent, once read. Only macOS has one, and it is read only
    /// while the daemon cannot be reached.
    pub agent: Option<AgentStatus>,
    /// How long to wait before trying again: longer each time, with jitter,
    /// so many clients reconnecting after a daemon restart don't all ask at
    /// once.
    pub retry: RetryStrategy,
    /// Why the daemon is down: the last failure to answer, or the refusal,
    /// when it hasn't answered since. The window shows the Connection page in
    /// place of the pages that need the daemon while there is one. Trying
    /// again doesn't clear it, so the window doesn't flicker back to a page
    /// on every try.
    pub failure: Option<DaemonStatus>,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self {
            status: DaemonStatus::default(),
            agent: None,
            retry: RetryStrategy::for_initial_connection(),
            failure: None,
        }
    }
}

impl ConnectionState {
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.status == DaemonStatus::Connected
    }

    /// Whether the window shows the Connection page in place of the pages
    /// that need the daemon: see [`Self::failure`].
    #[must_use]
    pub fn is_down(&self) -> bool {
        self.failure.is_some()
    }
}
