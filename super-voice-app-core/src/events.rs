// SPDX-License-Identifier: GPL-3.0-only
//! What the daemon tells an app as it happens: its `/events` stream, read into
//! [`Event`]s.
//!
//! The stream heals itself: a dropped connection, an idle one, or a revoked
//! token is reconnected with backoff, so an app only hears that the stream is
//! live again ([`Event::Connected`]) or that the user refused it for good
//! ([`Event::Blocked`]).

use futures_util::{Stream, StreamExt};
use log::warn;
use serde_json::Value;
use super_voice_shared::daemon::http_client::WidgetEvent;
use super_voice_shared::daemon::widget_subscription::{
    WidgetSubscriptionConfig, WidgetSubscriptionUpdate, run_widget_subscription,
};
use super_voice_shared::models::protocol::NotificationEvent;
use super_voice_shared::registry::events::RegistryEvent;
use super_voice_shared::validation::get_http_socket_path;

use crate::daemon::client::SETTINGS_SCOPES;

/// The topics an app subscribes to.
///
/// `recording_state` drives the recording badge (`recording_events`),
/// `frequency_bands` the microphone meter (`audio_visualization`),
/// `speaking_state` the speaking badge (`playback_events`),
/// `playback_frequency_bands` the playback meter (`audio_visualization`), and
/// `daemon_status_changed` / `download_progress` / `registry_install` the
/// model-switch progress and the Library's install cards (`daemon_status`).
pub const TOPICS: &[&str] = &[
    "recording_state",
    "frequency_bands",
    "speaking_state",
    "playback_frequency_bands",
    "daemon_status_changed",
    "download_progress",
    "registry_install",
];

/// Something the daemon said, or the stream itself.
#[derive(Debug, Clone)]
pub enum Event {
    /// The stream is live, first or again. What happened while it was down
    /// was missed, so anything shown from it is read again.
    Connected,
    /// The user refused this app; the stream has ended.
    Blocked(String),
    /// The microphone opened or closed.
    Recording(bool),
    /// How loud the microphone is, 0 to 1, and whether that sounds like
    /// speech.
    MicrophoneLevel { level: f32, is_speech: bool },
    /// The daemon started or stopped speaking, and the utterance, when it
    /// names one. It speaks for whoever asked, not only this app.
    Speaking {
        is_speaking: bool,
        utterance_id: Option<String>,
    },
    /// How loud what the daemon says is, 0 to 1, and whether sound is
    /// reaching the speaker.
    PlaybackLevel { level: f32, is_speech: bool },
    /// A feature's model, or a download, moved on; or the daemon's own news,
    /// such as a shared setting changed. In the shape the daemon's older
    /// event list had.
    Status(NotificationEvent),
    /// A backend install moved on, finished or failed.
    Install(RegistryEvent),
}

/// The daemon's `/events` stream, as [`Event`]s, until the user refuses this
/// app.
pub fn subscribe() -> impl Stream<Item = Event> + Send + 'static {
    let config = WidgetSubscriptionConfig::new(
        crate::daemon::client::app_id(),
        crate::daemon::client::app_name(),
        SETTINGS_SCOPES,
        TOPICS,
    );
    run_widget_subscription(get_http_socket_path(), config).filter_map(|update| async move {
        match update {
            WidgetSubscriptionUpdate::Connected => Some(Event::Connected),
            WidgetSubscriptionUpdate::Event(event) => parse(&event),
            WidgetSubscriptionUpdate::Disconnected { reason } => {
                warn!("/events disconnected ({reason}); reconnecting");
                None
            }
            WidgetSubscriptionUpdate::NeedsReauth { reason } => {
                warn!("/events needs a new token ({reason}); asking again on the next attempt");
                None
            }
            WidgetSubscriptionUpdate::Blocked { reason } => {
                warn!("/events refused by the user ({reason}); the stream has ended");
                Some(Event::Blocked(reason))
            }
        }
    })
}

/// The event `event` is, or `None` for one an app doesn't show.
#[must_use]
pub fn parse(event: &WidgetEvent) -> Option<Event> {
    let p = &event.payload;
    match event.name.as_str() {
        "recording_state" => Some(Event::Recording(flag(p, "is_recording"))),
        "frequency_bands" => {
            let (level, is_speech) = level(p);
            Some(Event::MicrophoneLevel { level, is_speech })
        }
        "speaking_state" => Some(Event::Speaking {
            is_speaking: flag(p, "is_speaking"),
            utterance_id: p
                .get("utterance_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }),
        "playback_frequency_bands" => {
            let (level, is_speech) = level(p);
            Some(Event::PlaybackLevel { level, is_speech })
        }
        // Both features publish these, each naming itself in `feature`, and
        // the daemon's own news names none. The apps show both features, so
        // they take all three; a feature they don't know isn't theirs.
        "daemon_status_changed" | "download_progress"
            if p.get("feature")
                .and_then(Value::as_str)
                .is_none_or(|feature| matches!(feature, "stt" | "tts")) =>
        {
            notification(event).map(Event::Status)
        }
        "registry_install" => match serde_json::from_value::<RegistryEvent>(p.clone()) {
            Ok(RegistryEvent::RefreshCompleted { .. } | RegistryEvent::RefreshFailed { .. }) => {
                None
            }
            Ok(install) => Some(Event::Install(install)),
            Err(_) => {
                warn!("registry_install event isn't a RegistryEvent; dropped (payload: {p})");
                None
            }
        },
        _ => None,
    }
}

fn flag(payload: &Value, key: &str) -> bool {
    payload.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// A meter's level from the energy an event carries, and whether that much
/// is speech.
fn level(payload: &Value) -> (f32, bool) {
    // reason: audio energy is a small positive float, well within f32; a
    // meter loses nothing to the cast.
    #[allow(clippy::cast_possible_truncation)]
    let energy = payload
        .get("total_energy")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    (level_percent(energy), energy > 0.0001)
}

/// An event in the shape of the daemon's older event list, which the apps'
/// status handling reads.
///
/// `None` when the event has no string `timestamp`: every
/// `daemon_status_changed` and `download_progress` the daemon publishes has
/// one, so one without it is broken and dropped, rather than given a clock
/// value made up here.
#[must_use]
pub fn notification(event: &WidgetEvent) -> Option<NotificationEvent> {
    let Some(timestamp) = event.payload.get("timestamp").and_then(Value::as_str) else {
        warn!(
            "event '{}' has no string `timestamp`; dropped (payload: {})",
            event.name, event.payload
        );
        return None;
    };
    Some(NotificationEvent {
        event_type_field: event.name.clone(),
        event_type: event.name.clone(),
        client_id: "daemon".to_string(),
        timestamp: timestamp.to_owned(),
        data: event.payload.clone(),
    })
}

/// Raw frequency-band energy (typically 0.00001 to 0.1) as a meter's 0 to 1,
/// on a -60 dB to 0 dB scale, where 0.1 is full. Quiet, normal and loud speech
/// (0.003, 0.005, 0.008) come to about 49, 57 and 63%.
#[must_use]
pub fn level_percent(energy: f32) -> f32 {
    let db = if energy <= 0.0 {
        -60.0
    } else {
        (20.0 * (energy * 10.0).log10()).clamp(-60.0, 0.0)
    };
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests;
