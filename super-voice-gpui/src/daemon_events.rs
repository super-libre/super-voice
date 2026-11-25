// SPDX-License-Identifier: GPL-3.0-only
//! What the daemon says as it happens: the core's event stream
//! (`super_voice_app_core::events`), read on the core's runtime and handed to
//! the UI thread as a GPUI event of this model's.
//!
//! The window starts it once the daemon answers and hands each event to
//! every page (`PageView::heard`), so a page never subscribes itself. The
//! stream heals itself; it ends only when the user refuses the app, and
//! [`DaemonEvents::restart`] starts it again after Retry authorization.

use futures_util::StreamExt as _;
use gpui_kit::{Context, EventEmitter, Task};
use super_voice_app_core::events::{self, Event};

use crate::runtime;

/// Something the daemon said.
#[derive(Debug, Clone)]
pub struct DaemonEvent(pub Event);

/// The event stream, as a GPUI model.
#[derive(Default)]
pub struct DaemonEvents {
    /// The reading on the core's runtime; aborted when replaced or dropped.
    reader: Option<tokio::task::AbortHandle>,
    /// The hand-over on the UI thread.
    forward: Option<Task<()>>,
}

impl EventEmitter<DaemonEvent> for DaemonEvents {}

impl DaemonEvents {
    /// Not reading yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the stream is being read.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.reader.is_some()
    }

    /// Read the stream, if it isn't read already.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        if !self.is_running() {
            self.restart(cx);
        }
    }

    /// Read the stream from a new subscription.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        self.stop();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let reader = runtime::spawn(async move {
            let mut stream = std::pin::pin!(events::subscribe());
            while let Some(event) = stream.next().await {
                if tx.send(event).is_err() {
                    return;
                }
            }
        });
        self.reader = Some(reader.abort_handle());
        self.forward = Some(cx.spawn(async move |this, cx| {
            while let Some(event) = rx.recv().await {
                let ended = matches!(event, Event::Blocked(_));
                if this
                    .update(cx, |_, cx| cx.emit(DaemonEvent(event)))
                    .is_err()
                {
                    return;
                }
                if ended {
                    break;
                }
            }
            _ = this.update(cx, |events, _| events.reader = None);
        }));
    }

    fn stop(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.abort();
        }
        self.forward = None;
    }
}

impl Drop for DaemonEvents {
    fn drop(&mut self) {
        self.stop();
    }
}
