// SPDX-License-Identifier: GPL-3.0-only

//! Dictation contexts: what the user is dictating, so a model can hear it.
//!
//! A context is two things a backend can be told about the speech it is about
//! to receive. They are separate fields because they are consumed differently,
//! and a backend may accept one and not the other:
//!
//! - [`prompt`](DictationContext::prompt) is free text, for a model that
//!   follows instructions.
//! - [`vocabulary`](DictationContext::vocabulary) is a list of terms to bias
//!   toward, for a model that does not. Most transcription models are this
//!   kind: `OpenAI`'s own guide says Whisper "doesn't follow instructions like a
//!   general-purpose text model", and its `prompt` parameter is a vocabulary
//!   hint. Deepgram wants one `keyterm` per term; `whisper-1` wants them
//!   joined. Qwen3-ASR takes either shape.
//!
//! The vocabulary is a list rather than a blob precisely because each consumer
//! splits it differently, and a term containing whatever delimiter a blob chose
//! — `"Menjivar, Jorge"` — would be ambiguous in one and not the other. Holding
//! the structure here means the daemon splits once and nobody guesses.
//!
//! One context is active at a time, and a backend may be pointed at a different
//! one; see the daemon's config. The wire shape lives here so the daemon that
//! serves it and the settings app that renders it cannot drift.

use serde::{Deserialize, Serialize};

/// Longest a context's prompt may be, in characters.
///
/// The ceiling is the request header it is delivered as. Matches the backend
/// option cap for the same reason — see `MAX_OPTION_CHARS` in
/// `super-voice-registry-types` — and is far longer than any dictation
/// instruction anyone writes.
pub const MAX_PROMPT_CHARS: usize = 4000;

/// Most terms one context's vocabulary may hold.
///
/// Every consumer has a tighter limit of its own (`whisper-1` takes 224 tokens,
/// Deepgram 500), so this is not the number that matters to a backend — it is
/// the number that keeps a runaway list from being stored at all.
pub const MAX_VOCABULARY_TERMS: usize = 200;

/// Longest a context's vocabulary may be in total, in characters.
pub const MAX_VOCABULARY_CHARS: usize = 4000;

/// Ids a context may not take, because a path already means something by them.
///
/// Contexts are addressed at `/v1/stt/context/{id}`, and that namespace holds two
/// fixed siblings — `/v1/stt/context/list` and `/v1/stt/context/active`. A router
/// prefers the literal, so a context called `active` would be stored fine and
/// then be unreachable: every read of it would answer with the active
/// selection instead. Refusing the two ids costs the user nothing, since the
/// id is a slug they never see, while the alternative — renaming the fixed
/// paths to something no id could collide with — would make the whole
/// namespace read worse to buy back two words.
pub const RESERVED_IDS: [&str; 2] = ["active", "list"];

/// One named dictation context.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct DictationContext {
    /// Stable identifier, chosen by the client that created it. Used in every
    /// path that addresses this context and in the active selection, so it
    /// never changes — renaming edits [`name`](Self::name) alone.
    pub id: String,
    /// What the user calls it: "Coding", "Email". Free text, and the only
    /// field a rename touches.
    #[serde(default)]
    pub name: String,
    /// Instructions for a model that follows them. Empty is normal and
    /// common — a context that is only a vocabulary is a perfectly good
    /// context.
    #[serde(default)]
    pub prompt: String,
    /// Terms to bias recognition toward, in the order the user wrote them.
    #[serde(default)]
    pub vocabulary: Vec<String>,
}

impl DictationContext {
    /// Whether this context would tell a backend anything at all.
    ///
    /// A context with neither half is not an error — it is one the user has
    /// started and not filled in — but nothing is injected for it, so the
    /// daemon can skip the headers entirely.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.prompt.trim().is_empty() && self.vocabulary.is_empty()
    }

    /// What to call this context in a list.
    ///
    /// Falls back to the id when the name is blank. A stored context always has
    /// a name — [`check`](Self::check) refuses one without — but a draft being
    /// typed does not yet, and a picker rendering an empty string would show a
    /// row the user cannot see or click.
    #[must_use]
    pub fn display_name(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() {
            self.id.clone()
        } else {
            name.to_string()
        }
    }

    /// The vocabulary with blank entries dropped and each term trimmed.
    ///
    /// The settings UI edits the vocabulary as one input per term and keeps a
    /// trailing empty row for the next one, so a blank arriving from a client
    /// is expected rather than malformed. Normalizing on the way in means
    /// neither storage nor any backend has to think about it.
    #[must_use]
    pub fn clean_vocabulary(vocabulary: &[String]) -> Vec<String> {
        vocabulary
            .iter()
            .map(|term| term.trim())
            .filter(|term| !term.is_empty())
            .map(ToString::to_string)
            .collect()
    }

    /// Whether `value` is a usable context id: a short slug a path can carry
    /// without escaping.
    ///
    /// Deliberately narrow. The id appears in `/v1/stt/context/{id}`, and the
    /// alternative — accepting anything and percent-encoding it the way
    /// `backend_id` is handled — buys nothing here, because unlike a backend's
    /// `source` this id is not a pre-existing name the daemon must accept. A
    /// client that wants "Coding" as an id sends `coding`.
    #[must_use]
    pub fn is_valid_id(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 64
            && value
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    }

    /// Whether `value` is an id the context namespace has already spoken for.
    ///
    /// See [`RESERVED_IDS`].
    #[must_use]
    pub fn is_reserved_id(value: &str) -> bool {
        RESERVED_IDS.contains(&value)
    }

    /// Whether this context's fields are within the limits the daemon stores.
    ///
    /// # Errors
    /// The user-facing sentence naming what is wrong.
    pub fn check(&self) -> Result<(), String> {
        if !Self::is_valid_id(&self.id) {
            return Err(
                "A context id is 1 to 64 characters of lowercase letters, digits, - and _."
                    .to_string(),
            );
        }
        if Self::is_reserved_id(&self.id) {
            return Err(format!(
                "`{}` is a reserved context id; the endpoint that addresses contexts already \
                 uses it.",
                self.id
            ));
        }
        if self.name.trim().is_empty() {
            return Err("A context needs a name.".to_string());
        }
        check_limits(&self.prompt, &self.vocabulary)
    }
}

/// Whether a prompt and a vocabulary fit in the headers they are sent as.
fn check_limits(prompt: &str, vocabulary: &[String]) -> Result<(), String> {
    let prompt_chars = prompt.chars().count();
    if prompt_chars > MAX_PROMPT_CHARS {
        return Err(format!(
            "The prompt is {prompt_chars} characters; the most that can be sent is \
             {MAX_PROMPT_CHARS}."
        ));
    }
    let terms = DictationContext::clean_vocabulary(vocabulary);
    if terms.len() > MAX_VOCABULARY_TERMS {
        return Err(format!(
            "That is {} terms; the most that can be sent is {MAX_VOCABULARY_TERMS}.",
            terms.len()
        ));
    }
    let vocabulary_chars: usize = terms.iter().map(|t| t.chars().count()).sum();
    if vocabulary_chars > MAX_VOCABULARY_CHARS {
        return Err(format!(
            "The vocabulary is {vocabulary_chars} characters; the most that can be sent is \
             {MAX_VOCABULARY_CHARS}."
        ));
    }
    Ok(())
}

/// Context for one dictation, added to the user's own: what the app or the
/// conversation the user is in knows about what they are about to say, like
/// the names and terms an assistant has just used. It comes with the request
/// that starts the dictation and is used for that dictation alone; nothing is
/// stored.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtraContext {
    /// Instructions or a topic, for a model that follows them. Goes after the
    /// user's own prompt.
    #[serde(default)]
    #[cfg_attr(
        feature = "openapi",
        schema(example = "The user is answering a question about the release notes.")
    )]
    pub prompt: String,
    /// Terms the user may say. Go after the user's own terms.
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(example = json!(["agy", "Antigravity"])))]
    pub vocabulary: Vec<String>,
}

impl ExtraContext {
    /// Whether it would add anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.prompt.trim().is_empty()
            && DictationContext::clean_vocabulary(&self.vocabulary).is_empty()
    }

    /// Whether it fits the limits a context does, on its own.
    ///
    /// # Errors
    /// The user-facing sentence naming what is too long.
    pub fn check(&self) -> Result<(), String> {
        check_limits(&self.prompt, &self.vocabulary)
    }

    /// The user's context, `user`, with this one added.
    ///
    /// The prompt is the user's, a blank line, then this one's; the
    /// vocabulary is the user's terms, then this one's that it does not have
    /// yet, compared without regard to case. The user's own always fits:
    /// what this one adds is cut to the room left within the limits, the
    /// prompt where it runs out and the terms from the end.
    #[must_use]
    pub fn added_to(&self, user: Option<&DictationContext>) -> DictationContext {
        let mut merged = user.cloned().unwrap_or_default();
        let prompt = self.prompt.trim();
        if !prompt.is_empty() {
            let own = merged.prompt.trim();
            let separator = if own.is_empty() { "" } else { "\n\n" };
            let room =
                MAX_PROMPT_CHARS.saturating_sub(own.chars().count() + separator.chars().count());
            let added: String = prompt.chars().take(room).collect();
            if !added.is_empty() {
                merged.prompt = format!("{own}{separator}{added}");
            }
        }
        let mut terms = DictationContext::clean_vocabulary(&merged.vocabulary);
        let mut chars: usize = terms.iter().map(|term| term.chars().count()).sum();
        for term in DictationContext::clean_vocabulary(&self.vocabulary) {
            let known = terms
                .iter()
                .any(|own| own.to_lowercase() == term.to_lowercase());
            if known {
                continue;
            }
            let length = term.chars().count();
            if terms.len() >= MAX_VOCABULARY_TERMS || chars + length > MAX_VOCABULARY_CHARS {
                break;
            }
            chars += length;
            terms.push(term);
        }
        merged.vocabulary = terms;
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DictationContext, ExtraContext, MAX_PROMPT_CHARS, MAX_VOCABULARY_CHARS,
        MAX_VOCABULARY_TERMS, RESERVED_IDS,
    };

    fn terms(words: &[&str]) -> Vec<String> {
        words.iter().map(ToString::to_string).collect()
    }

    /// One dictation's context goes after the user's own: its prompt after a
    /// blank line, its terms after the user's, without the ones the user has.
    #[test]
    fn extra_context_goes_after_the_users_own() {
        let user = ctx("coding");
        let extra = ExtraContext {
            prompt: " The user is answering a question. ".to_string(),
            vocabulary: terms(&["agy", "Rebase", " ", "Antigravity"]),
        };
        let merged = extra.added_to(Some(&user));
        assert_eq!(
            merged.prompt,
            "I dictate code.\n\nThe user is answering a question."
        );
        assert_eq!(
            merged.vocabulary,
            terms(&["main branch", "rebase", "agy", "Antigravity"])
        );
        assert_eq!(
            (merged.id.as_str(), merged.name.as_str()),
            ("coding", "Coding")
        );

        // With no context of the user's, it stands alone.
        let alone = extra.added_to(None);
        assert_eq!(alone.prompt, "The user is answering a question.");
        assert_eq!(alone.vocabulary, terms(&["agy", "Rebase", "Antigravity"]));

        // An empty one changes nothing.
        assert_eq!(ExtraContext::default().added_to(Some(&user)), user);
        assert!(ExtraContext::default().is_empty());
        assert!(!extra.is_empty());
    }

    /// The user's own context always fits whole; one dictation's is cut to
    /// the room left.
    #[test]
    fn extra_context_is_cut_to_the_room_the_users_leaves() {
        let mut user = ctx("coding");
        user.prompt = "a".repeat(MAX_PROMPT_CHARS - 5);
        user.vocabulary = (0..MAX_VOCABULARY_TERMS - 1)
            .map(|n| format!("t{n}"))
            .collect();
        let extra = ExtraContext {
            prompt: "bcdefgh".to_string(),
            vocabulary: terms(&["one", "two"]),
        };
        let merged = extra.added_to(Some(&user));
        assert_eq!(merged.prompt.chars().count(), MAX_PROMPT_CHARS);
        assert!(
            merged.prompt.ends_with("\n\nbcd"),
            "{}",
            &merged.prompt[3990..]
        );
        assert_eq!(merged.vocabulary.len(), MAX_VOCABULARY_TERMS);
        assert_eq!(merged.vocabulary.last().map(String::as_str), Some("one"));

        // A full prompt takes nothing more.
        user.prompt = "a".repeat(MAX_PROMPT_CHARS);
        assert_eq!(extra.added_to(Some(&user)).prompt, user.prompt);

        // Nor a vocabulary full by length.
        let long = ctx("long");
        let mut full = long.clone();
        full.vocabulary = vec!["x".repeat(MAX_VOCABULARY_CHARS)];
        assert_eq!(extra.added_to(Some(&full)).vocabulary, full.vocabulary);
    }

    /// One dictation's context is held to the limits a stored one is.
    #[test]
    fn extra_context_is_held_to_the_limits() {
        let mut extra = ExtraContext {
            prompt: "a".repeat(MAX_PROMPT_CHARS),
            vocabulary: Vec::new(),
        };
        assert!(extra.check().is_ok());
        extra.prompt.push('a');
        assert!(extra.check().is_err());
        extra.prompt.clear();
        extra.vocabulary = (0..=MAX_VOCABULARY_TERMS).map(|n| n.to_string()).collect();
        assert!(extra.check().is_err());
    }

    fn ctx(id: &str) -> DictationContext {
        DictationContext {
            id: id.to_string(),
            name: "Coding".to_string(),
            prompt: "I dictate code.".to_string(),
            vocabulary: vec!["main branch".to_string(), "rebase".to_string()],
        }
    }

    /// A context that is only a vocabulary is a normal context, and so is one
    /// that is only a prompt. Only the one carrying neither is skipped.
    #[test]
    fn emptiness_is_about_both_halves() {
        assert!(DictationContext::default().is_empty());

        let mut only_terms = DictationContext::default();
        only_terms.vocabulary = vec!["kubectl".to_string()];
        assert!(!only_terms.is_empty());

        let mut only_prompt = DictationContext::default();
        only_prompt.prompt = "I dictate code.".to_string();
        assert!(!only_prompt.is_empty());

        // Whitespace is not a prompt.
        let mut blank = DictationContext::default();
        blank.prompt = "   \n ".to_string();
        assert!(blank.is_empty());
    }

    /// The settings UI keeps a trailing empty row for the next term, so blanks
    /// arrive by design rather than by mistake.
    #[test]
    fn vocabulary_is_cleaned_of_blanks_and_padding() {
        let raw = vec![
            "  main branch ".to_string(),
            String::new(),
            "rebase".to_string(),
            "   ".to_string(),
        ];
        assert_eq!(
            DictationContext::clean_vocabulary(&raw),
            vec!["main branch".to_string(), "rebase".to_string()]
        );
    }

    /// The id rides in a path segment, so it is a slug or it is refused —
    /// there is no third option that does not involve escaping.
    #[test]
    fn an_id_is_a_slug() {
        for good in ["coding", "email", "standup-notes", "ctx_2", "a"] {
            assert!(
                DictationContext::is_valid_id(good),
                "{good} should be valid"
            );
        }
        for bad in ["", "Coding", "with space", "sl/ash", "dot.ted", "caf\u{e9}"] {
            assert!(
                !DictationContext::is_valid_id(bad),
                "{bad:?} should be refused"
            );
        }
        assert!(DictationContext::is_valid_id(&"a".repeat(64)));
        assert!(!DictationContext::is_valid_id(&"a".repeat(65)));
    }

    /// A context named after one of its own endpoints would be stored and then
    /// be unreachable, because the router prefers the literal path.
    #[test]
    fn the_ids_the_namespace_uses_are_refused() {
        for reserved in RESERVED_IDS {
            assert!(
                DictationContext::is_valid_id(reserved),
                "{reserved} is a well-formed slug — refusing it is the point"
            );
            let mut taken = ctx(reserved);
            assert!(
                taken.check().is_err(),
                "{reserved} is a path of its own and cannot also be a context"
            );
            // Nothing else about the id rules changed: a longer id containing
            // one is fine.
            taken.id = format!("{reserved}-notes");
            assert!(taken.check().is_ok());
        }
    }

    #[test]
    fn check_refuses_what_cannot_be_stored() {
        assert!(ctx("coding").check().is_ok());

        let mut bad_id = ctx("Coding");
        bad_id.name = "Coding".to_string();
        assert!(bad_id.check().is_err(), "an id must be a slug");

        let mut unnamed = ctx("coding");
        unnamed.name = "  ".to_string();
        assert!(unnamed.check().is_err(), "a context needs a name");

        let mut long_prompt = ctx("coding");
        long_prompt.prompt = "x".repeat(MAX_PROMPT_CHARS + 1);
        assert!(long_prompt.check().is_err());
        long_prompt.prompt = "x".repeat(MAX_PROMPT_CHARS);
        assert!(long_prompt.check().is_ok(), "the cap is inclusive");

        let mut many_terms = ctx("coding");
        many_terms.vocabulary = (0..=MAX_VOCABULARY_TERMS).map(|n| n.to_string()).collect();
        assert!(many_terms.check().is_err());

        // Few terms, but far too much text in them.
        let mut long_terms = ctx("coding");
        long_terms.vocabulary = vec!["x".repeat(MAX_VOCABULARY_CHARS + 1)];
        assert!(long_terms.check().is_err());
    }

    /// Blank rows do not count against the term limit — they are the UI's
    /// trailing row, not the user's data.
    #[test]
    fn blank_rows_are_not_counted_against_the_limits() {
        let mut padded = ctx("coding");
        padded.vocabulary = (0..MAX_VOCABULARY_TERMS)
            .map(|n| n.to_string())
            .chain(std::iter::repeat_n(String::new(), 50))
            .collect();
        assert!(padded.check().is_ok());
    }

    /// A context written before a field existed still loads: every field but
    /// the id carries a serde default.
    #[test]
    fn an_older_context_deserializes() {
        let value: DictationContext =
            serde_json::from_str(r#"{"id":"coding"}"#).expect("deserializes");
        assert_eq!(value.id, "coding");
        assert!(value.name.is_empty());
        assert!(value.vocabulary.is_empty());
    }
}
