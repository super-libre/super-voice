// SPDX-License-Identifier: GPL-3.0-only
//! super-voice-app-core for apps written in other languages.
//!
//! `UniFFI` generates each language's bindings from what this crate exports
//! (`uniffi-bindgen generate --library`). The `SwiftUI` app on macOS is the
//! first to use it. What is exported is deliberately small and grows with the
//! pages that app has: plain records and functions, with the core's own types
//! kept behind them, so the core needs no `UniFFI` of its own.

use std::sync::LazyLock;

pub mod notes;
pub mod status;

uniffi::setup_scaffolding!();

/// The runtime the stores' work runs on. One for every store: the work is
/// small, and the core's calls expect tokio under them.
pub(crate) static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("super-voice-store")
        .enable_all()
        .build()
        .expect("the stores' tokio runtime starts")
});

/// Make this app a client of its own to the daemon, before its first call:
/// its token kept apart from the settings app's, which shares the machine
/// while both exist.
pub(crate) fn identify() {
    super_voice_app_core::daemon::client::set_identity("super-voice-macos", "Super Voice");
}

/// The folder the notes are kept in.
#[must_use]
#[uniffi::export]
pub fn notes_folder() -> String {
    super_voice_app_core::notes::dir().display().to_string()
}
