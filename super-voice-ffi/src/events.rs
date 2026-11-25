// SPDX-License-Identifier: GPL-3.0-only
//! The daemon's event stream, read once for every store that shows something
//! from it: the speech test's status and meter today, more pages later.

use std::sync::LazyLock;

use futures_util::StreamExt;
use super_voice_app_core::events::Event;
use tokio::sync::{Notify, broadcast};

use crate::RUNTIME;

/// How many events a slow store may fall behind before it skips some. Meters
/// send many a second; a store that skips one only misses a level.
const BACKLOG: usize = 256;

/// The stream, started on the first [`listen`]. It runs for as long as the
/// app does, reconnecting by itself. It ends only if the user refuses the
/// app, and starts again on [`resubscribe`].
static HUB: LazyLock<broadcast::Sender<Event>> = LazyLock::new(|| {
    let (sender, _) = broadcast::channel(BACKLOG);
    let hub = sender.clone();
    RUNTIME.spawn(async move {
        crate::identify();
        loop {
            let mut events = Box::pin(super_voice_app_core::events::subscribe());
            while let Some(event) = events.next().await {
                // No store listening is fine: the event wasn't needed.
                let _ = hub.send(event);
            }
            AGAIN.notified().await;
        }
    });
    sender
});

/// Wakes the stream once the user refused the app and asked again.
static AGAIN: Notify = Notify::const_new();

/// The daemon's events from now on.
pub(crate) fn listen() -> broadcast::Receiver<Event> {
    HUB.subscribe()
}

/// Start the stream again after the user refused the app: the user asked
/// again (Retry authorization). A stream still running is left as it is.
pub(crate) fn resubscribe() {
    // Started, if it never was: then it starts at once.
    LazyLock::force(&HUB);
    AGAIN.notify_one();
}
