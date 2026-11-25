// SPDX-License-Identifier: GPL-3.0-only
//! The daemon's event stream, read once for every store that shows something
//! from it: the speech test's status and meter today, more pages later.

use std::sync::LazyLock;

use futures_util::StreamExt;
use super_voice_app_core::events::Event;
use tokio::sync::broadcast;

use crate::RUNTIME;

/// How many events a slow store may fall behind before it skips some. Meters
/// send many a second; a store that skips one only misses a level.
const BACKLOG: usize = 256;

/// The stream, started on the first [`listen`]. It runs for as long as the
/// app does, reconnecting by itself, and ends only if the user refuses the
/// app.
static HUB: LazyLock<broadcast::Sender<Event>> = LazyLock::new(|| {
    let (sender, _) = broadcast::channel(BACKLOG);
    let hub = sender.clone();
    RUNTIME.spawn(async move {
        crate::identify();
        let mut events = Box::pin(super_voice_app_core::events::subscribe());
        while let Some(event) = events.next().await {
            // No store listening is fine: the event wasn't needed.
            let _ = hub.send(event);
        }
    });
    sender
});

/// The daemon's events from now on.
pub(crate) fn listen() -> broadcast::Receiver<Event> {
    HUB.subscribe()
}
