// SPDX-License-Identifier: GPL-3.0-only
//! The headers the daemon hands a backend on its requests: which model a
//! request is for, the user's secrets and options for the backend, and the
//! user's dictation context.
//!
//! Each is named `x-voice-<name>`. Backends built before those names read the
//! same value under their feature's prefix, `x-stt-<name>` or `x-tts-<name>`,
//! so the daemon sends every header under both names.

#[cfg(any(feature = "wasm-backends", feature = "subprocess-backends"))]
use super_voice_shared::models::contexts::DictationContext;

/// The prefix of every header the daemon hands a backend.
pub const PREFIX: &str = "x-voice-";

/// Which feature a backend request is for. It decides the prefix the
/// header's former name has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// Speech to text, whose headers were `x-stt-*`.
    SpeechToText,
    /// Text to speech, whose headers were `x-tts-*`.
    TextToSpeech,
}

impl Feature {
    /// The prefix this feature's headers had before they were `x-voice-*`.
    #[must_use]
    pub const fn former_prefix(self) -> &'static str {
        match self {
            Self::SpeechToText => "x-stt-",
            Self::TextToSpeech => "x-tts-",
        }
    }
}

/// Add the header `name`, given without a prefix (`model`,
/// `secret-api_key`), with `value`, under both of its names.
pub fn push(headers: &mut Vec<(String, String)>, feature: Feature, name: &str, value: String) {
    headers.push((format!("{PREFIX}{name}"), value.clone()));
    headers.push((format!("{}{name}", feature.former_prefix()), value));
}

/// Add `context`'s two context headers to `headers`, each only when its half
/// has something in it: a backend reading one and not the other should not
/// have to tell "absent" from "empty".
#[cfg(any(feature = "wasm-backends", feature = "subprocess-backends"))]
pub(crate) fn push_context(headers: &mut Vec<(String, String)>, context: &DictationContext) {
    if !context.prompt.trim().is_empty() {
        push(
            headers,
            Feature::SpeechToText,
            CONTEXT_HEADERS[0],
            json_ascii(&context.prompt),
        );
    }
    if !context.vocabulary.is_empty() {
        push(
            headers,
            Feature::SpeechToText,
            CONTEXT_HEADERS[1],
            json_ascii_array(&context.vocabulary),
        );
    }
}

/// The two context headers' names, without a prefix: the prompt and the
/// vocabulary.
#[cfg(any(feature = "wasm-backends", feature = "subprocess-backends"))]
const CONTEXT_HEADERS: [&str; 2] = ["prompt", "vocabulary"];

/// `headers`, an instance's set, with one dictation's context in place of the
/// instance's: its context headers, under either name, left out, and
/// `context`'s added. A WASM backend's: a subprocess backend's engine
/// replaces them itself.
#[cfg(feature = "wasm-backends")]
pub(crate) fn with_context(
    mut headers: Vec<(String, String)>,
    context: &DictationContext,
) -> Vec<(String, String)> {
    let feature = Feature::SpeechToText;
    headers.retain(|(name, _)| {
        !CONTEXT_HEADERS.iter().any(|half| {
            [PREFIX, feature.former_prefix()]
                .iter()
                .any(|prefix| name.eq_ignore_ascii_case(&format!("{prefix}{half}")))
        })
    });
    push_context(&mut headers, context);
    headers
}

/// One JSON string, escaped so every byte of it is printable ASCII.
///
/// Both context headers are JSON because a prompt holds line breaks and a
/// vocabulary is a list, and a header value can carry neither as itself. JSON
/// is already the encoding every backend has a parser for, so the far side is
/// `serde_json::from_str` and nothing else.
///
/// Written by hand rather than through `serde_json::to_string` for one reason:
/// serde emits non-ASCII characters as themselves, and
/// [`HeaderValue`](axum::http::HeaderValue) is asymmetric about those. It
/// *accepts* any byte from `0x80` up at construction, and then refuses the same
/// bytes in `to_str`. So a Spanish prompt builds a header the daemon considers
/// valid and the backend cannot read back — a failure that shows up as a
/// context silently going missing for some users and not others. Escaping to
/// `\uXXXX` keeps the value inside the visible-ASCII range `to_str` will
/// return.
#[cfg(any(feature = "wasm-backends", feature = "subprocess-backends"))]
fn json_ascii(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        escape_ascii_into(c, &mut out);
    }
    out.push('"');
    out
}

/// A JSON array of strings, each escaped by [`json_ascii`]'s rules.
///
/// The vocabulary rides as an array rather than a joined string because every
/// consumer splits it differently and a term containing the delimiter —
/// `"Menjivar, Jorge"` — would be ambiguous in a flat one. Sharing the escaper
/// with [`json_ascii`] is what keeps the two headers decodable the same way.
#[cfg(any(feature = "wasm-backends", feature = "subprocess-backends"))]
fn json_ascii_array(values: &[String]) -> String {
    let mut out = String::with_capacity(values.iter().map(|v| v.len() + 3).sum::<usize>() + 2);
    out.push('[');
    for (i, value) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_ascii(value));
    }
    out.push(']');
    out
}

/// Append one character in its JSON form, using only printable ASCII.
///
/// Anything outside `0x20..=0x7e` goes out as `\uXXXX`, astral characters as
/// the surrogate pair JSON spells them with — which is what `encode_utf16`
/// produces, so the two agree without a special case.
#[cfg(any(feature = "wasm-backends", feature = "subprocess-backends"))]
fn escape_ascii_into(c: char, out: &mut String) {
    use std::fmt::Write as _;
    match c {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        '\u{8}' => out.push_str("\\b"),
        '\u{c}' => out.push_str("\\f"),
        c if c.is_ascii_graphic() || c == ' ' => out.push(c),
        c => {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                // Writing to a String is infallible.
                let _ = write!(out, "\\u{unit:04x}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_goes_under_its_name_and_its_former_one() {
        let mut headers = Vec::new();
        push(
            &mut headers,
            Feature::SpeechToText,
            "model",
            "whisper-base".into(),
        );
        push(
            &mut headers,
            Feature::TextToSpeech,
            "secret-api_key",
            "k".into(),
        );
        assert_eq!(
            headers,
            [
                ("x-voice-model".to_string(), "whisper-base".to_string()),
                ("x-stt-model".to_string(), "whisper-base".to_string()),
                ("x-voice-secret-api_key".to_string(), "k".to_string()),
                ("x-tts-secret-api_key".to_string(), "k".to_string()),
            ]
        );
    }

    /// One dictation's context goes in place of the instance's, under both
    /// names of each half, and every other header stays.
    #[cfg(feature = "wasm-backends")]
    #[test]
    fn a_dictations_context_replaces_the_instances() {
        use super_voice_shared::models::contexts::{DictationContext, ExtraContext};
        let user = DictationContext {
            vocabulary: vec!["rebase".to_string()],
            ..DictationContext::default()
        };
        let mut instance = Vec::new();
        push(
            &mut instance,
            Feature::SpeechToText,
            "secret-api_key",
            "k".into(),
        );
        push_context(&mut instance, &user);
        let extra = ExtraContext {
            prompt: "About the release.".to_string(),
            vocabulary: vec!["agy".to_string()],
        };
        let merged = crate::stt_models::transcribe::dictation_context(Some(&user), Some(&extra))
            .expect("a model that takes context, and a dictation that brought some");
        let sent = with_context(instance, &merged);
        let value = |name: &str| {
            let found: Vec<_> = sent
                .iter()
                .filter(|(n, _)| n == name)
                .map(|(_, v)| v.as_str())
                .collect();
            found
        };
        assert_eq!(value("x-voice-secret-api_key"), ["k"]);
        assert_eq!(value("x-voice-vocabulary"), [r#"["rebase","agy"]"#]);
        assert_eq!(value("x-stt-vocabulary"), [r#"["rebase","agy"]"#]);
        assert_eq!(value("x-voice-prompt"), [r#""About the release.""#]);
        assert_eq!(value("x-stt-prompt"), [r#""About the release.""#]);

        // A model that takes no context gets none, and a dictation that
        // brought none leaves the instance's.
        assert_eq!(
            crate::stt_models::transcribe::dictation_context(None, Some(&extra)),
            None
        );
        assert_eq!(
            crate::stt_models::transcribe::dictation_context(Some(&user), None),
            None
        );
    }
}
