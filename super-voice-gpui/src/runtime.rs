// SPDX-License-Identifier: GPL-3.0-only
//! The tokio runtime the core's async work runs on.
//!
//! GPUI's executors are not tokio, and the daemon client and the pages'
//! runners expect tokio under them. So the app starts one runtime, once, and
//! runs that work on it with [`spawn`]. A tokio `JoinHandle` is an ordinary
//! future, so a GPUI task awaits the result without blocking the UI thread.

use std::future::Future;
use std::sync::LazyLock;

static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("super-voice-core")
        .enable_all()
        .build()
        .expect("the core's tokio runtime starts")
});

/// Run `work` on the core's runtime.
pub fn spawn<F>(work: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    RUNTIME.spawn(work)
}
