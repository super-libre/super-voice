// SPDX-License-Identifier: GPL-3.0-only

use crate::core::app::AppModel;
use crate::ui::messages::{DaemonMessage, Message, UpdateMessage};
use cosmic::prelude::*;
use log::{info, warn};
use super_voice_shared::models::protocol::{DaemonStatusEvent, NotificationEvent};

/// How a `SettingsChanged { setting }` SSE event should refresh the Updates
/// page's state, for the two self-update settings (`language` needs
/// `AppModel` state — `model_language_for` — to build its follow-up task, so
/// it's matched separately in the caller and never reaches this function).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::core::app) enum SelfUpdateSettingRoute {
    /// Reload the toggler value and re-fetch the cached status. Cheap, and
    /// correct here because `update_check_enabled` doesn't change what the
    /// next check would compute.
    RefreshCachedStatus,
    /// Force a real re-check rather than trust the cached status.
    ForceRecheck,
}

/// `update_beta_optin` MUST route through a real re-check (`ForceRecheck`,
/// which the caller sends via `CheckNow`) rather than a cached status
/// refetch: `beta_optin_effective` is computed server-side at check time,
/// not stored, so a plain refetch would show the STALE value until whatever
/// triggered this event's own check lands — a regression a previous review
/// round fixed. `update_check_enabled` has no such effective-at-check-time
/// field, so the cheaper cached refresh is correct for it. Any other
/// setting name (including `language`) isn't a self-update setting at all.
pub(in crate::core::app) fn self_update_setting_route(
    setting: &str,
) -> Option<SelfUpdateSettingRoute> {
    match setting {
        "update_check_enabled" => Some(SelfUpdateSettingRoute::RefreshCachedStatus),
        "update_beta_optin" => Some(SelfUpdateSettingRoute::ForceRecheck),
        _ => None,
    }
}

impl AppModel {
    pub(in crate::core::app) fn handle_daemon_events(
        &mut self,
        message: DaemonMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            DaemonMessage::DaemonEventsReceived(events) => {
                // Process EVERY event and collect their tasks. Returning on the
                // first event that yields a task dropped the rest of the batch
                // (and left `last_event_timestamp` stuck before them) — latent
                // only because producers currently wrap singletons (Tier 1 #17).
                let mut tasks = Vec::new();
                for event in events {
                    // Update timestamp for next polling
                    self.last_event_timestamp = Some(event.timestamp.clone());

                    // What it means for the Models page's cards: a stage to
                    // read again, a load started or ended, a download tick.
                    for input in super_voice_app_core::pages::models::heard(&event) {
                        tasks.push(self.models_input(input));
                    }
                    if event.event_type == "daemon_status_changed"
                        && let Some(task) = self.process_daemon_status_event(&event)
                    {
                        tasks.push(task);
                    }
                }
                // Force UI update after processing events that may change state
                tasks.push(self.update_title());
                Task::batch(tasks)
            }

            _ => Task::none(),
        }
    }
    // ^ `handle_daemon_events` only ever receives `DaemonEventsReceived`, but it
    // takes the full `DaemonMessage` for a uniform delegate signature; the
    // catch-all covers the other (unreachable) variants without a panic.

    pub(in crate::core::app) fn process_daemon_status_event(
        &mut self,
        event: &NotificationEvent,
    ) -> Option<Task<cosmic::Action<Message>>> {
        info!("Received daemon event: {:?}", event.data);
        // Text to speech's events have a type of their own, and only one stage.
        if feature_of(&event.data) == Some("tts") {
            return self.process_synthesis_event(&event.data);
        }
        // Typed deserialize replaces the former hand-matched `.get("status")` /
        // `.get("<field>")` reads: a field rename/typo is now a compile error, not
        // a silent no-op (audit 2 Tier 2 #9). The injected `timestamp` key is
        // ignored; an unrecognized or malformed event yields `None`.
        let event = match serde_json::from_value::<DaemonStatusEvent>(event.data.clone()) {
            Ok(ev) => ev,
            Err(e) => {
                info!("Unhandled/unparseable daemon status event: {e}");
                return None;
            }
        };
        match event {
            DaemonStatusEvent::SettingsChanged { setting } => self.handle_setting_changed(&setting),
            // The daemon completed a periodic self-update check and found a
            // newer release. Re-fetch the status so the header badge and the
            // Updates page pick it up without waiting for the user to open
            // that page.
            DaemonStatusEvent::UpdateAvailable { latest_version } => {
                log::info!("Daemon reports update available: {latest_version}");
                Some(self.handle_update_messages(UpdateMessage::Page(
                    super_voice_app_core::pages::updates::Input::Available,
                )))
            }
            // A stage's model: the Models page's cards take these (`heard`).
            DaemonStatusEvent::Ready { .. }
            | DaemonStatusEvent::LoadingModel { .. }
            | DaemonStatusEvent::LoadingModelForDevice { .. }
            | DaemonStatusEvent::ModelSwitched { .. }
            | DaemonStatusEvent::SwitchingDevice { .. }
            | DaemonStatusEvent::DeviceSwitchError { .. }
            | DaemonStatusEvent::ActiveBackendChanged { .. } => None,
        }
    }

    /// A setting changed — possibly from another client, or this very client's
    /// own change coming back over SSE. Either feature's, or the daemon's own:
    /// a setting means the same thing whichever names it.
    fn handle_setting_changed(&mut self, setting: &str) -> Option<Task<cosmic::Action<Message>>> {
        match setting {
            "language" => {
                // Re-fetch the language state so a per-model button that
                // follows the global value, and the global card, reflect
                // the new value.
                // Every pair answered for, not just one: both stages
                // show a language control, and a block that follows the
                // global value goes stale on either card.
                Some(Task::batch([
                    crate::core::app::handlers::settings_page::load_language(),
                    Task::done(crate::core::app::handlers::language::page_action(
                        super_voice_app_core::pages::model_language::Input::PrimaryChanged,
                    )),
                ]))
            }
            // The Models page's cards read every stage again with it
            // (`heard`): whether a model out of memory is idle follows it.
            "idle_unload" => Some(crate::core::app::handlers::settings_page::load_idle_unload()),
            // The rest of the daemon's settings: another client may have
            // changed one, and this app shows each of them somewhere.
            "audio_theme" => Some(crate::core::app::handlers::tasks::load_audio_theme()),
            "volume" => Some(crate::core::app::handlers::tasks::load_volume()),
            "notification_method" => {
                Some(crate::core::app::handlers::settings_page::load_notification_method())
            }
            "write_method" => Some(crate::core::app::handlers::tasks::load_write_method()),
            "preview_typing" => Some(crate::core::app::handlers::tasks::load_preview_typing()),
            "recording_stop_mode" => {
                Some(crate::core::app::handlers::tasks::load_recording_stop_mode())
            }
            "read_method" => Some(crate::core::app::handlers::tasks::load_read_method()),
            // A model's voice was set or cleared: the Synthesis card shows it.
            "voice" => Some(self.reload_model_voices()),
            // A voice was added, renamed or deleted. The Voices tab lists the
            // library, and the Synthesis card offers the voices drawn from
            // it: without this, the card said the model had no voices after
            // another client added one.
            "voices" => Some(Task::batch([
                Task::done(cosmic::Action::App(Message::Voices(
                    crate::ui::messages::VoicesMessage::Page(
                        super_voice_app_core::pages::voices::Input::Refresh,
                    ),
                ))),
                self.reload_model_voices(),
            ])),
            // A context was added, edited, deleted, or put in force.
            // One topic covers the whole family because the answer to
            // any of them is the same: re-read the list. The open
            // editor is deliberately untouched — see
            // `ContextsState::replace`.
            "contexts" => Some(Task::done(cosmic::Action::App(Message::Contexts(
                crate::ui::messages::ContextsMessage::Page(
                    super_voice_app_core::pages::contexts::Input::Reload,
                ),
            )))),
            // A backend's option was set or cleared, or a secret stored or
            // removed. The catalog carries each option's value, and reloading
            // it asks again which secrets are configured: without this, a
            // Configure sheet opened after another client's change showed the
            // value from when the app started. The cards read theirs with
            // it (`heard`).
            "options" | "secrets" => Some(Task::done(cosmic::Action::App(Message::Backend(
                crate::ui::messages::BackendMessage::BackendsReload,
            )))),
            // Both self-update settings need something beyond a plain
            // `SettingsChanged` no-op, but *what* they need differs —
            // see `self_update_setting_route`'s doc comment for why
            // `update_beta_optin` can't just reuse the cached-refresh
            // path `update_check_enabled` uses.
            other => match self_update_setting_route(other) {
                Some(SelfUpdateSettingRoute::RefreshCachedStatus) => Some(Task::batch(vec![
                    crate::core::app::handlers::tasks::load_update_check_enabled(),
                    crate::core::app::handlers::tasks::refresh_update_status(),
                ])),
                Some(SelfUpdateSettingRoute::ForceRecheck) => {
                    // `CheckNow`'s `if self.update.checking { return }`
                    // guard makes this a no-op for the app's own
                    // self-triggered change (its `BetaOptinToggled`
                    // already chains `CheckNow`), while a genuine
                    // cross-app change still gets a correct fresh
                    // recompute.
                    Some(self.handle_update_messages(UpdateMessage::Page(
                        super_voice_app_core::pages::updates::Input::CheckNow,
                    )))
                }
                None => None,
            },
        }
    }

    /// Text to speech's events, on its one stage.
    ///
    /// Parsed with that feature's own event type: it reports voice preparation,
    /// which speech to text's has no variant for, and names no stage. The
    /// match is exhaustive, so an event the feature adds is handled here
    /// deliberately rather than read as speech to text's.
    fn process_synthesis_event(
        &mut self,
        data: &serde_json::Value,
    ) -> Option<Task<cosmic::Action<Message>>> {
        use super_voice_shared::tts::models::protocol::DaemonStatusEvent as SynthesisEvent;
        let event = match serde_json::from_value::<SynthesisEvent>(data.clone()) {
            Ok(event) => event,
            Err(e) => {
                info!("Unhandled/unparseable text-to-speech event: {e}");
                return None;
            }
        };
        match event {
            SynthesisEvent::SettingsChanged { setting } => self.handle_setting_changed(&setting),
            SynthesisEvent::UpdateAvailable { latest_version } => {
                log::info!("Daemon reports update available: {latest_version}");
                Some(self.handle_update_messages(UpdateMessage::Page(
                    super_voice_app_core::pages::updates::Input::Available,
                )))
            }
            // The stage's model: the Models page's cards take these (`heard`).
            SynthesisEvent::Ready { .. }
            | SynthesisEvent::LoadingModel { .. }
            | SynthesisEvent::DeviceSwitchError { .. }
            | SynthesisEvent::ActiveBackendChanged { .. }
            | SynthesisEvent::ModelSwitched { .. }
            | SynthesisEvent::SwitchingDevice { .. }
            | SynthesisEvent::LoadingModelForDevice { .. } => None,
            // The wait is the same whoever started it, so the Voices tab shows
            // it for a preparation another client asked for too.
            SynthesisEvent::PreparingVoice { voice, model } => {
                info!("daemon is preparing voice {voice} for {model}");
                self.voices.preparing = Some(voice);
                None
            }
            SynthesisEvent::VoicePrepared {
                voice,
                model,
                error,
            } => {
                // Not surfaced as a banner: nothing the user did has failed
                // yet. The next attempt to speak in the voice tries again and
                // reports properly if it still cannot.
                if let Some(e) = &error {
                    warn!("daemon could not prepare voice {voice} for {model}: {e}");
                } else {
                    info!("daemon prepared voice {voice} for {model}");
                }
                if self.voices.preparing.as_deref() == Some(voice.as_str()) {
                    self.voices.preparing = None;
                }
                None
            }
        }
    }
}

/// The feature an event names, or `None` for the daemon's own news.
fn feature_of(data: &serde_json::Value) -> Option<&str> {
    data.get("feature").and_then(serde_json::Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::{SelfUpdateSettingRoute, self_update_setting_route};

    /// The regression this pins: `update_beta_optin` must force a real
    /// re-check, never the cheaper cached-status path — `beta_optin_effective`
    /// is computed server-side at check time and isn't part of the cached
    /// status a plain refetch would return.
    #[test]
    fn update_beta_optin_forces_a_recheck() {
        assert_eq!(
            self_update_setting_route("update_beta_optin"),
            Some(SelfUpdateSettingRoute::ForceRecheck)
        );
    }

    #[test]
    fn update_check_enabled_uses_the_cached_refresh() {
        assert_eq!(
            self_update_setting_route("update_check_enabled"),
            Some(SelfUpdateSettingRoute::RefreshCachedStatus)
        );
    }

    #[test]
    fn other_settings_including_language_are_not_self_update_settings() {
        assert_eq!(self_update_setting_route("language"), None);
        assert_eq!(self_update_setting_route("something_else"), None);
    }
}
