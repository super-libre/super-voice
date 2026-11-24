// SPDX-License-Identifier: GPL-3.0-only
//! The buffer the app's editors keep their text in.

use cosmic::widget::text_editor;
use super_voice_app_core::state::TextBuffer;

/// libcosmic's editor buffer, as the core's [`TextBuffer`].
///
/// The core's states hold an editor's text in whatever buffer the app's
/// toolkit has. This app's is libcosmic's `text_editor::Content`, which also
/// keeps the cursor and the selection. A wrapper, because neither the trait
/// nor the buffer is this crate's.
#[derive(Debug, Clone, Default)]
pub struct Editor(pub text_editor::Content);

impl TextBuffer for Editor {
    fn from_text(text: &str) -> Self {
        Self(text_editor::Content::with_text(text))
    }

    fn text(&self) -> String {
        self.0.text()
    }

    fn replace_text(&mut self, text: &str) {
        self.0 = text_editor::Content::with_text(text);
        self.0
            .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
    }
}
