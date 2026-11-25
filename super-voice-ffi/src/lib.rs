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

/// Show the app's words in the first of `languages` there is a catalog for,
/// falling back to English. Tags as the platform lists them: `fr-CA`, `en`.
#[uniffi::export]
// reason: UniFFI hands a list over by value
#[allow(clippy::needless_pass_by_value)]
pub fn set_languages(languages: Vec<String>) {
    super_voice_app_core::i18n::select_tags(&languages);
}

/// The message `key` in the selected language: the same catalogs the
/// settings app uses.
#[must_use]
#[uniffi::export]
pub fn text(key: &str) -> String {
    super_voice_app_core::i18n::text(key, &[])
}

/// [`text`], with `args` filled in. A value that reads as a number is passed
/// as one, so a count picks its plural form.
///
/// Without the marks Fluent puts around each value to keep its direction
/// apart (U+2068, U+2069): the app's languages all read left to right, and the
/// marks would ride along, unseen, into text the user copies, such as a
/// voice's id.
#[must_use]
#[uniffi::export]
// reason: UniFFI hands a map over by value
#[allow(clippy::needless_pass_by_value, clippy::implicit_hasher)]
pub fn text_with(key: &str, args: std::collections::HashMap<String, String>) -> String {
    let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    super_voice_app_core::i18n::text(key, &args).replace(['\u{2068}', '\u{2069}'], "")
}

/// The folder the notes are kept in.
#[must_use]
#[uniffi::export]
pub fn notes_folder() -> String {
    super_voice_app_core::notes::dir().display().to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    #[test]
    fn filled_in_words_carry_no_direction_marks() {
        let args = HashMap::from([
            ("seconds".to_string(), "3.8".to_string()),
            ("transcript".to_string(), "with transcript".to_string()),
            ("id".to_string(), "voice:abc".to_string()),
        ]);
        let words = super::text_with("voice-meta", args);
        assert_eq!(words, "3.8 s · with transcript · voice:abc");
    }
}
