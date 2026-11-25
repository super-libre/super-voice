// SPDX-License-Identifier: GPL-3.0-only

//! Shared `Task` builders re-used across handlers, so the same daemon call +
//! result-to-`Message` mapping isn't re-rolled at each call site.

use crate::daemon::client::{list_backends, ping_daemon};
use crate::ui::messages::{BackendMessage, DaemonMessage, Message};
use cosmic::prelude::*;
use super_voice_app_core::pages::behavior;

use super::behavior::page_action as behavior_action;

/// Ping the daemon and map the result to `DaemonConnected` / `DaemonError`.
/// Shared by the periodic keep-alive, the reconnect retries, and the startup
/// ping.
pub(in crate::core::app) fn ping_task() -> Task<cosmic::Action<Message>> {
    Task::perform(ping_daemon(), |result| {
        cosmic::Action::App(Message::Daemon(match result {
            Ok(_) => DaemonMessage::DaemonConnected,
            Err(e) => DaemonMessage::DaemonError(e),
        }))
    })
}

/// Reload both backend catalogs: the installed list and the annotated registry.
///
/// They carry different halves of one picture — `/backend/list` names what is
/// installed (and its version), `/registry/backend/list` annotates each entry with
/// the `installed_version` it reads off disk per request — and every question a
/// card asks needs both. Refreshed apart, a card can report a version from one
/// and an update from the other and have them disagree.
///
/// No index refresh: the annotation is computed from local state, so the cached
/// index is enough and a network round-trip would only make this slower than
/// the moment that needs it.
pub(in crate::core::app) fn reload_backend_catalogs() -> Task<cosmic::Action<Message>> {
    Task::batch([reload_backends(), fetch_registry_catalog(false)])
}

/// Reload the installed-backend catalog and map the result to `BackendsLoaded`
/// / `BackendsError`.
pub(in crate::core::app) fn reload_backends() -> Task<cosmic::Action<Message>> {
    Task::batch([
        Task::perform(list_backends(), |result| {
            cosmic::Action::App(match result {
                Ok(backends) => Message::Backend(BackendMessage::BackendsLoaded(backends)),
                Err(e) => Message::Backend(BackendMessage::BackendsError(e.to_string())),
            })
        }),
        // Which of them can fill each stage is the daemon's answer, not a
        // filter of the catalog above — and it changes for exactly the same
        // reasons, so it is re-asked here rather than anywhere else.
        Task::done(super::models::models_action(
            super_voice_app_core::pages::models::Input::ReloadBackends,
        )),
    ])
}

/// Fetch the full annotated registry catalog (optionally refreshing the index
/// first) and map the result to `RegistryListLoaded` / `RegistryListFailed`.
/// The catalog always includes incompatible entries; filtering is client-side.
pub(in crate::core::app) fn fetch_registry_catalog(refresh: bool) -> Task<cosmic::Action<Message>> {
    Task::perform(
        super_voice_app_core::pages::library::run_catalog(refresh),
        super::models_page::library_action,
    )
}

/// Reload models, device info, and per-setting state on reconnect. Each setting
/// is fetched with its own dedicated GET call — no bulk `fetch_daemon_config`.
pub(in crate::core::app) fn build_load_settings_tasks() -> Task<cosmic::Action<Message>> {
    Task::batch([
        load_audio_theme(),
        load_volume(),
        // Contexts are global and the daemon owns them, so a restart under an
        // open Contexts page would otherwise leave the list showing whatever
        // the previous daemon had. The `settings_changed` topic covers a live
        // edit; this covers the reconnect, which that topic cannot.
        Task::done(cosmic::Action::App(Message::Contexts(
            crate::ui::messages::ContextsMessage::Page(
                super_voice_app_core::pages::contexts::Input::Reload,
            ),
        ))),
        load_preview_typing(),
        load_recording_stop_mode(),
        load_read_method(),
        load_write_method(),
        super::settings_page::load_notification_method(),
        super::settings_page::load_idle_unload(),
        refresh_update_status(),
        load_update_check_enabled(),
    ])
}

// One loader per setting: what a reconnect reads, and what a
// `settings_changed` naming that setting reads again, so a change another
// client makes shows here.

/// Fetch the sound theme.
pub(in crate::core::app) fn load_audio_theme() -> Task<cosmic::Action<Message>> {
    Task::perform(behavior::run_load_theme(), behavior_action)
}

/// Fetch the sound volume.
pub(in crate::core::app) fn load_volume() -> Task<cosmic::Action<Message>> {
    Task::perform(behavior::run_load_volume(), behavior_action)
}

/// Fetch whether previews are typed.
pub(in crate::core::app) fn load_preview_typing() -> Task<cosmic::Action<Message>> {
    Task::perform(behavior::run_load_preview_typing(), behavior_action)
}

/// Fetch what stops a recording.
pub(in crate::core::app) fn load_recording_stop_mode() -> Task<cosmic::Action<Message>> {
    Task::perform(behavior::run_load_stop_mode(), behavior_action)
}

/// Fetch where reading aloud takes its text from.
pub(in crate::core::app) fn load_read_method() -> Task<cosmic::Action<Message>> {
    Task::perform(
        super_voice_app_core::pages::read_aloud::run_load(),
        super::settings::page_action,
    )
}

/// Fetch how transcripts are typed.
pub(in crate::core::app) fn load_write_method() -> Task<cosmic::Action<Message>> {
    Task::perform(behavior::run_load_write_method(), behavior_action)
}

/// Re-fetch the self-update status. Used by connection-time settings loads
/// (`build_load_settings_tasks`), by `on_nav_select` when the Updates page is
/// opened, and by the `SettingsChanged { setting }` SSE handler for
/// `update_check_enabled`/`update_beta_optin` (the latter because
/// `beta_optin_effective` rides this status, not a separate field), so its
/// data isn't stale from whenever the app last connected or last re-fetched.
pub(in crate::core::app) fn refresh_update_status() -> Task<cosmic::Action<Message>> {
    Task::perform(
        super_voice_app_core::pages::updates::run_status(),
        super::update::page_action,
    )
}

/// Load the automatic-check-enabled setting, defaulting to `true` (its
/// documented daemon-side default) on a fetch failure rather than leaving
/// the Updates page's toggler in a misleading off state. Also re-used by the
/// `SettingsChanged { setting: "update_check_enabled" }` SSE handler so a
/// change from another client is reflected here too.
pub(in crate::core::app) fn load_update_check_enabled() -> Task<cosmic::Action<Message>> {
    Task::perform(
        super_voice_app_core::pages::updates::run_load_auto_check(),
        super::update::page_action,
    )
}
