// SPDX-License-Identifier: GPL-3.0-only
//! The daemon's `/events` stream, as this app's messages. The stream and what
//! its events mean are the core's (`super_voice_app_core::events`).

use futures_util::{SinkExt, StreamExt};
use log::info;

use super::events::message;
use crate::ui::messages::Message;

/// Wrapper for `Subscription::run_with` so the subscription restarts when the counter changes.
#[derive(Hash)]
pub(super) struct UdpSubscriptionId(pub(super) u64);

/// The daemon's events, for the meters, the recording and speaking badges,
/// model-switch progress and the Library's install cards. It reconnects by
/// itself, and ends only when the user refuses this app.
pub(super) fn audio_events_subscription(
    _id: &UdpSubscriptionId,
) -> std::pin::Pin<Box<dyn cosmic::iced::futures::Stream<Item = Message> + Send>> {
    Box::pin(cosmic::iced::stream::channel(100, async |mut channel| {
        let mut events = Box::pin(super_voice_app_core::events::subscribe());
        info!("Settings subscription starting");
        while let Some(event) = events.next().await {
            let Some(msg) = message(event) else {
                continue;
            };
            if channel.send(msg).await.is_err() {
                break;
            }
        }
        info!("Settings subscription ended");
    }))
}
