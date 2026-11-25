// SPDX-License-Identifier: GPL-3.0-only
//! The Shortcuts page: what the daemon says about each shortcut, and on
//! macOS, the shortcut listener's keys and where its agent stands.

use super_voice_shared::models::shortcuts::ShortcutList;

use crate::state::connection::AgentStatus;

/// What the Shortcuts page shows.
#[derive(Debug, Clone, Default)]
pub struct ShortcutsState {
    /// The daemon's last answer. `None` until the page is first shown.
    pub list: Option<ShortcutList>,
    /// The keys the macOS shortcut listener listens for, as the daemon last
    /// answered. `None` until read.
    pub hotkey: Option<String>,
    /// Where the shortcut listener's agent stands with macOS. `None` until
    /// read, and always on other systems.
    pub listener: Option<AgentStatus>,
}
