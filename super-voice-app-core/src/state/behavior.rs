// SPDX-License-Identifier: GPL-3.0-only
//! Speech to text's Behavior tab: how a recording stops and is typed, the
//! sounds around it, and the typing test.

use super_voice_shared::models::recording_stop_mode::RecordingStopMode;
use super_voice_shared::models::write_method::WriteMethod;

use super::AudioTheme;

/// What the tab shows and edits. Each setting holds the daemon's value: a
/// change shows once the daemon has stored it, except the sounds, which show
/// at once and go back if the daemon refuses them.
#[derive(Debug, Clone)]
pub struct BehaviorState {
    /// What stops a recording.
    pub stop_mode: RecordingStopMode,
    /// Whether the words are typed while they are still being heard.
    pub preview_typing: bool,
    /// How transcripts are typed.
    pub write_method: WriteMethod,
    /// How the daemon typed the last test, when it said: the method `Auto`
    /// came to.
    pub typed_with: Option<WriteMethod>,
    /// What the typing test's field holds.
    pub typing_test_text: String,
    /// Seconds left before a delayed typing test types, while one counts down.
    pub typing_countdown: Option<u8>,
    /// The sound themes to pick from.
    pub audio_themes: Vec<AudioTheme>,
    /// The sound theme in use; `Silent` while sounds are off.
    pub audio_theme: AudioTheme,
    /// The theme sounds come back with when they are turned on again.
    pub sounding_theme: AudioTheme,
    /// The sounds' volume, 0 to 100, as the slider has it.
    pub volume: u8,
    /// The volume the daemon last stored, which a refused one goes back to.
    pub stored_volume: u8,
}

impl Default for BehaviorState {
    fn default() -> Self {
        Self {
            stop_mode: RecordingStopMode::default(),
            preview_typing: false,
            write_method: WriteMethod::default(),
            typed_with: None,
            typing_test_text: String::new(),
            typing_countdown: None,
            audio_themes: Vec::new(),
            audio_theme: AudioTheme::default(),
            sounding_theme: AudioTheme::default(),
            volume: 100,
            stored_volume: 100,
        }
    }
}

impl BehaviorState {
    /// Whether the recording sounds are on.
    #[must_use]
    pub fn sounds_on(&self) -> bool {
        self.audio_theme != AudioTheme::Silent
    }
}
