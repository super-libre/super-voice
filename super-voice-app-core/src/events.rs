// SPDX-License-Identifier: GPL-3.0-only
//! What the daemon tells an app as it happens: its `/events` stream, read into
//! [`Event`]s.
//!
//! The stream heals itself: a dropped connection, an idle one, or a revoked
//! token is reconnected with backoff. An app hears that the stream dropped
//! ([`Event::Disconnected`]: the daemon may be gone), that it is live again
//! ([`Event::Connected`]), or that the user refused it for good
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
/// model-switch progress and the Library's install cards (`daemon_status`),
/// and `typing_failed` the toast for a dictation that went to the clipboard
/// (`global_transcriptions`, since it carries the words).
///
/// Oldest first: a new topic goes at the end. A daemon older than the app
/// refuses a topic it doesn't know (`400 invalid_topic`), and [`subscribe`]
/// then asks again without the newest ones.
pub const TOPICS: &[&str] = &[
    "recording_state",
    "frequency_bands",
    "speaking_state",
    "playback_frequency_bands",
    "daemon_status_changed",
    "download_progress",
    "registry_install",
    "typing_failed",
];

/// Something the daemon said, or the stream itself.
#[derive(Debug, Clone)]
pub enum Event {
    /// The stream is live, first or again. What happened while it was down
    /// was missed, so anything shown from it is read again.
    Connected,
    /// The stream dropped, and is being reconnected. The daemon may have
    /// stopped: an app asks it at once rather than at its next keep-alive.
    Disconnected,
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
    /// A dictation was transcribed and could not be typed.
    TypingFailed(TypingFailed),
}

/// A dictation the daemon could not type: why, whether its words are on the
/// clipboard, and the words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypingFailed {
    pub reason: TypingFailure,
    pub copied: bool,
    pub text: String,
}

/// Why a dictation was not typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypingFailure {
    /// The daemon may not type: Device Control (Accessibility) is off.
    NotAllowed,
    /// Another app held secure keyboard entry.
    SecureInput,
    /// Anything else.
    Failed,
}

impl TypingFailure {
    /// The reason the daemon names; one this app doesn't know is
    /// [`Self::Failed`].
    #[must_use]
    pub fn from_wire(reason: &str) -> Self {
        match reason {
            "not_allowed" => Self::NotAllowed,
            "secure_input" => Self::SecureInput,
            _ => Self::Failed,
        }
    }
}

/// The daemon's `/events` stream, as [`Event`]s, until the user refuses this
/// app.
///
/// A daemon older than the app refuses the whole stream for a topic it
/// doesn't know, so the stream is asked for again without the newest topic,
/// one at a time, until the daemon takes it. When that stream drops later
/// (the daemon may have been updated), every topic is asked for again.
pub fn subscribe() -> impl Stream<Item = Event> + Send + 'static {
    subscribe_with(|topics| {
        let config = WidgetSubscriptionConfig::new(
            crate::daemon::client::app_id(),
            crate::daemon::client::app_name(),
            SETTINGS_SCOPES,
            topics,
        );
        run_widget_subscription(get_http_socket_path(), config)
    })
}

/// A subscription's updates, boxed so a new one can take its place.
type Updates = std::pin::Pin<Box<dyn Stream<Item = WidgetSubscriptionUpdate> + Send>>;

/// [`subscribe`], with `open` starting the subscription to the topics given.
fn subscribe_with<S>(
    open: impl Fn(&'static [&'static str]) -> S + Send + 'static,
) -> impl Stream<Item = Event> + Send + 'static
where
    S: Stream<Item = WidgetSubscriptionUpdate> + Send + 'static,
{
    let first: Updates = Box::pin(open(TOPICS));
    futures_util::stream::unfold(
        (first, TOPICS.len(), open),
        |(mut updates, mut kept, open)| async move {
            loop {
                let update = updates.next().await?;
                if let WidgetSubscriptionUpdate::Disconnected { reason } = &update {
                    if let Some(fewer) = fewer_topics(reason, kept) {
                        warn!(
                            "The daemon is older than the app: it doesn't know the event topic `{}`; \
                             subscribing without it",
                            TOPICS[fewer]
                        );
                        kept = fewer;
                        updates = Box::pin(open(&TOPICS[..kept]));
                        continue;
                    }
                    if kept < TOPICS.len() {
                        kept = TOPICS.len();
                        updates = Box::pin(open(TOPICS));
                    }
                }
                if let Some(event) = from_update(update) {
                    return Some((event, (updates, kept, open)));
                }
            }
        },
    )
}

/// How many of [`TOPICS`] to ask for next, when the stream asking for the
/// first `kept` dropped for `reason`: one fewer when the daemon refused a
/// topic it doesn't know, or `None` to ask for the same again. Never none:
/// the daemon refuses an empty list too.
fn fewer_topics(reason: &str, kept: usize) -> Option<usize> {
    (reason.starts_with("invalid_topic") && kept > 1).then(|| kept - 1)
}

/// What the stream's `update` tells an app, or `None` for one it needn't hear.
fn from_update(update: WidgetSubscriptionUpdate) -> Option<Event> {
    match update {
        WidgetSubscriptionUpdate::Connected => Some(Event::Connected),
        WidgetSubscriptionUpdate::Event(event) => parse(&event),
        WidgetSubscriptionUpdate::Disconnected { reason } => {
            warn!("/events disconnected ({reason}); reconnecting");
            Some(Event::Disconnected)
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
        "typing_failed" => Some(Event::TypingFailed(TypingFailed {
            reason: TypingFailure::from_wire(p.get("reason").and_then(Value::as_str).unwrap_or("")),
            copied: flag(p, "copied"),
            text: p
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        })),
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

/// The setting a status event says changed, from either feature: another
/// client changed it, or this one did, and a page that shows it reads it
/// again.
#[must_use]
pub fn setting_changed(event: &Event) -> Option<&str> {
    let Event::Status(status) = event else {
        return None;
    };
    if status.data.get("status").and_then(Value::as_str) != Some("settings_changed") {
        return None;
    }
    status.data.get("setting").and_then(Value::as_str)
}

/// The stage a `download_progress` event is about, and the report: a download
/// tick, a load step, or the end of either. The daemon says which stage it is
/// provisioning for, so the report lands on that stage's card.
#[must_use]
pub fn download_progress(
    event: &Event,
) -> Option<(
    crate::state::Stage,
    super_voice_shared::models::protocol::DownloadProgress,
)> {
    let Event::Status(status) = event else {
        return None;
    };
    if status.event_type != "download_progress" {
        return None;
    }
    let progress =
        serde_json::from_value::<super_voice_shared::models::protocol::DownloadProgress>(
            status.data.clone(),
        )
        .ok()?;
    let feature = status.data.get("feature").and_then(Value::as_str);
    let stage = crate::state::Stage::from_wire(feature, progress.stage)?;
    Some((stage, progress))
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
