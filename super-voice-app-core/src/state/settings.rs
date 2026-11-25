// SPDX-License-Identifier: GPL-3.0-only
//! The Settings page: the settings both features share, other than the
//! primary language, which the language state holds.

use super_voice_shared::models::notification_method::NotificationMethod;

/// What the Settings page shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsState {
    /// How recording and speech failures reach the user.
    pub notification_method: NotificationMethod,
    /// Seconds an unused model stays in memory; `0` keeps it there.
    pub idle_unload_secs: u64,
}
