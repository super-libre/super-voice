// SPDX-License-Identifier: GPL-3.0-only
//! super-voice-app-core for apps written in other languages.
//!
//! `UniFFI` generates each language's bindings from what this crate exports
//! (`uniffi-bindgen generate --library`). The `SwiftUI` app on macOS is the
//! first to use it. What is exported is deliberately small and grows with the
//! pages that app has: plain records and functions, with the core's own types
//! kept behind them, so the core needs no `UniFFI` of its own.

use super_voice_app_core::notes;

uniffi::setup_scaffolding!();

/// A call into the core that failed, with the reason in words.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    /// The call failed; `message` says why.
    #[error("{message}")]
    Failed { message: String },
}

/// One note on the Notes tab.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NoteSummary {
    /// The note's id: its file name without the extension.
    pub id: String,
    /// When the note was made, in local time, as `YYYY-MM-DD HH:MM`.
    pub created: String,
    /// What the note says.
    pub text: String,
    /// How many words it has.
    pub words: u32,
}

impl From<&notes::Note> for NoteSummary {
    fn from(note: &notes::Note) -> Self {
        Self {
            id: note.id.clone(),
            created: note.created.format("%Y-%m-%d %H:%M").to_string(),
            text: note.text.clone(),
            words: u32::try_from(note.words()).unwrap_or(u32::MAX),
        }
    }
}

/// The folder the notes are kept in.
#[must_use]
#[uniffi::export]
pub fn notes_folder() -> String {
    notes::dir().display().to_string()
}

/// Every note, newest first. A folder that is not there yet holds none.
///
/// # Errors
///
/// When the folder or a note in it cannot be read.
#[uniffi::export(async_runtime = "tokio")]
pub async fn list_notes() -> Result<Vec<NoteSummary>, CoreError> {
    let items = notes::list(&notes::dir())
        .await
        .map_err(|e| CoreError::Failed {
            message: e.to_string(),
        })?;
    Ok(items.iter().map(NoteSummary::from).collect())
}

#[cfg(test)]
mod tests {
    use super::NoteSummary;
    use super_voice_app_core::notes::Note;

    #[test]
    fn a_note_is_summarized_with_its_time_and_words() {
        let note = Note {
            id: "2026-11-24_05-01-16".to_string(),
            created: chrono::NaiveDateTime::parse_from_str("2026-11-24 05:01", "%Y-%m-%d %H:%M")
                .expect("a time"),
            text: "one two three".to_string(),
        };
        assert_eq!(
            NoteSummary::from(&note),
            NoteSummary {
                id: "2026-11-24_05-01-16".to_string(),
                created: "2026-11-24 05:01".to_string(),
                text: "one two three".to_string(),
                words: 3,
            }
        );
    }
}
