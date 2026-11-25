// SPDX-License-Identifier: GPL-3.0-only
//! Speech to text's recording test, on its Behavior tab: what the take has
//! heard so far, and what the last one came to.

use super_voice_shared::models::protocol::PreviewSource;

/// The recording test.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordingTest {
    /// The newest preview of what the take hears.
    pub preview: String,
    /// What the preview spans, when the daemon said.
    pub preview_source: Option<PreviewSource>,
    /// What the last take came to, until the next starts.
    pub heard: Option<Heard>,
}

/// What a take came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// Its transcript.
    Words(String),
    /// It recorded, and no speech was in it.
    Nothing,
    /// It failed, in the daemon's words.
    Failed(String),
}
