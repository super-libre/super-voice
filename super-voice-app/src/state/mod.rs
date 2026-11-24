// SPDX-License-Identifier: GPL-3.0-only

//! Application state and domain models.

pub mod assistants;
pub mod clips;
pub mod contexts;
pub mod device_offers;
pub mod language;
pub mod model_operations;
pub mod models;
pub mod models_page;
pub mod notes;
pub mod registry;
pub mod scripts;
pub mod shortcuts;
pub mod speech;
pub mod stage_catalog;
pub mod staged_picks;
pub mod update;
pub mod voices;

// Re-export commonly used types
pub use models::{
    ActionError, AudioTheme, ContextPage, DaemonStatus, ErrorScope, MenuAction, ModelsTab, Page,
    RecordingStatus, Section,
};
pub use speech::{SpeakingStatus, SpeechState};
// The types the daemon client reads into live with it, in the core crate.
pub use super_voice_app_core::state::{
    BlockStage, LanguageResolution, Stage, VoiceChoice, VoiceResolution, VoiceState,
};
