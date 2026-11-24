// SPDX-License-Identifier: GPL-3.0-only
//! super-voice-app-core for apps written in other languages.
//!
//! `UniFFI` generates each language's bindings from what this crate exports
//! (`uniffi-bindgen generate --library`). The `SwiftUI` app on macOS is the
//! first to use it. What is exported is deliberately small and grows with the
//! pages that app has: plain records and functions, with the core's own types
//! kept behind them, so the core needs no `UniFFI` of its own.

pub mod notes;

uniffi::setup_scaffolding!();

/// The folder the notes are kept in.
#[must_use]
#[uniffi::export]
pub fn notes_folder() -> String {
    super_voice_app_core::notes::dir().display().to_string()
}
