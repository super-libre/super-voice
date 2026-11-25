// SPDX-License-Identifier: GPL-3.0-only
//! super-voice-app-core for apps written in other languages.
//!
//! `UniFFI` generates each language's bindings from what this crate exports
//! (`uniffi-bindgen generate --library`). The `SwiftUI` app on macOS is the
//! first to use it. What is exported is deliberately small and grows with the
//! pages that app has: plain records and functions, with the core's own types
//! kept behind them, so the core needs no `UniFFI` of its own.

use std::sync::LazyLock;

use super_voice_app_core::state::TextBuffer;

pub mod behavior;
pub mod clips;
pub mod contexts;
mod events;
pub mod notes;
pub mod speech;
pub mod status;
pub mod voices;

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

/// An editor's text as a store holds it: the open note's, the next clip's.
///
/// The app keeps its own editor. It hands its text over as it changes, and
/// takes the store's back only when the store replaced it (words dictated into
/// a note, a draft emptied once its clip is made), which `replaced` counts.
/// Taking it back on every change would undo keystrokes still on their way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Buffer {
    pub(crate) text: String,
    pub(crate) replaced: u64,
}

impl TextBuffer for Buffer {
    fn from_text(text: &str) -> Self {
        Self {
            text: text.to_string(),
            replaced: 0,
        }
    }

    fn text(&self) -> String {
        self.text.clone()
    }

    fn replace_text(&mut self, text: &str) {
        text.clone_into(&mut self.text);
        self.replaced += 1;
    }
}

/// Which feature a readiness is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Feature {
    SpeechToText,
    TextToSpeech,
}

/// Why a feature can't be used right now, for the buttons that start work on
/// it (Record, Make clip). Each app says it in its own words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NotReady {
    /// The speech model has no voice to speak in.
    NoVoice,
    /// Text to speech's model is not up, or not set up.
    TextToSpeech,
    /// Speech to text's model is not up, or not set up.
    SpeechToText,
}

/// Why `feature`, whose model reads as `readiness`, can't be used right now,
/// or `None` when it can: the settings app's rule.
#[must_use]
#[uniffi::export]
pub fn not_ready(feature: Feature, readiness: status::Readiness) -> Option<NotReady> {
    use super_voice_app_core::rules::status::{self as rule, ModelStatus};
    use super_voice_app_core::state::Stage;
    let stage = match feature {
        Feature::SpeechToText => Stage::Transcription,
        Feature::TextToSpeech => Stage::Synthesis,
    };
    let status = match readiness {
        status::Readiness::None => ModelStatus::None,
        status::Readiness::Blocked => ModelStatus::Blocked,
        status::Readiness::NotLoaded => ModelStatus::NotLoaded,
        status::Readiness::Ready => ModelStatus::Ready,
        status::Readiness::Idle => ModelStatus::Idle,
        status::Readiness::Failed => ModelStatus::Failed,
        status::Readiness::NoVoice => ModelStatus::NoVoice,
    };
    rule::not_ready(stage, status).map(|reason| match reason {
        rule::NotReady::NoVoice => NotReady::NoVoice,
        rule::NotReady::TextToSpeech => NotReady::TextToSpeech,
        rule::NotReady::SpeechToText => NotReady::SpeechToText,
    })
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
