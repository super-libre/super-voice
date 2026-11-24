// SPDX-License-Identifier: GPL-3.0-only
//! The text of an editor an app draws.

/// An editor's text, in whatever buffer the app's toolkit keeps it.
///
/// The core never draws an editor, but it starts one (a note opened, a
/// context edited) and reads one back (a note saved, a clip made). The buffer
/// itself belongs to the toolkit, because it also holds the cursor and the
/// selection: rebuilding it from a `String` on every change is what makes a
/// cursor jump to the start mid-sentence. So each state that holds an editor
/// is generic over its buffer, and reaches it only through this.
pub trait TextBuffer {
    /// A buffer holding `text`.
    fn from_text(text: &str) -> Self;

    /// The text the buffer holds.
    fn text(&self) -> String;

    /// Replace the whole text, leaving the cursor at its end: where words
    /// dictated into an open note go, and where the user carries on typing.
    fn replace_text(&mut self, text: &str);
}

/// The plain buffer, for an app with nothing better and for the tests.
impl TextBuffer for String {
    fn from_text(text: &str) -> Self {
        text.to_string()
    }

    fn text(&self) -> String {
        self.clone()
    }

    fn replace_text(&mut self, text: &str) {
        text.clone_into(self);
    }
}
