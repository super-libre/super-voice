// SPDX-License-Identifier: GPL-3.0-only
//! The state the daemon client reads into: plain data, no UI types.

pub mod language;
pub mod stage;
pub mod voice;

pub use language::LanguageResolution;
pub use stage::{BlockStage, Stage};
pub use super_voice_shared::models::theme::AudioTheme;
pub use voice::{VoiceChoice, VoiceResolution, VoiceState};
