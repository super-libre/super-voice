// SPDX-License-Identifier: GPL-3.0-only
//! A recording the daemon streams back, read for a page: the Notes page's
//! takes and the Behavior page's recording test. The stream is read on the
//! core's runtime and each item handed to the page on the UI thread; dropping
//! the [`Take`] stops reading, which stops the recording.

use futures_util::{Stream, StreamExt as _};
use gpui_kit::{Context, Task, Window};

use crate::runtime;

/// A stream being read for a page.
pub struct Take {
    reader: tokio::task::AbortHandle,
    _forward: Task<()>,
}

impl Drop for Take {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

/// Read `stream`, handing each item to the page with `take_in`.
pub fn read<P: 'static, I: Send + 'static>(
    stream: impl Stream<Item = I> + Send + 'static,
    window: &mut Window,
    cx: &mut Context<P>,
    take_in: fn(&mut P, I, &mut Window, &mut Context<P>),
) -> Take {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let reader = runtime::spawn(async move {
        let mut stream = std::pin::pin!(stream);
        while let Some(item) = stream.next().await {
            if tx.send(item).is_err() {
                return;
            }
        }
    });
    let forward = cx.spawn_in(window, async move |this, cx| {
        while let Some(item) = rx.recv().await {
            if this
                .update_in(cx, |page, window, cx| take_in(page, item, window, cx))
                .is_err()
            {
                return;
            }
        }
    });
    Take {
        reader: reader.abort_handle(),
        _forward: forward,
    }
}
