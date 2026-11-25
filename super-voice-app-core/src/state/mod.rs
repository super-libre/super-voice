// SPDX-License-Identifier: GPL-3.0-only
//! The app's state: plain data, no UI types.
//!
//! A state that holds an editor is generic over the editor's buffer (see
//! [`TextBuffer`]), so each app keeps its toolkit's own.

pub mod assistants;
pub mod behavior;
pub mod clips;
pub mod contexts;
pub mod device_offers;
pub mod language;
pub mod model_operations;
pub mod notes;
pub mod registry;
pub mod scripts;
pub mod shortcuts;
pub mod speech;
pub mod stage;
pub mod stage_catalog;
pub mod staged_picks;
pub mod text_buffer;
pub mod voice;
pub mod voices;

pub use language::LanguageResolution;
pub use speech::{SpeakingStatus, SpeechState};
pub use stage::{BlockStage, Stage};
pub use super_voice_shared::models::theme::AudioTheme;
pub use text_buffer::TextBuffer;
pub use voice::{VoiceChoice, VoiceResolution, VoiceState};
