// SPDX-License-Identifier: GPL-3.0-only

//! Application state and domain models.
//!
//! Most of it lives in the core crate, which every app shares; this module
//! re-exports it under the paths the app has always used. What stays here is
//! this app's own: its pages and their labels, the Models page's tab model,
//! and the updater's state.

pub mod editor;
pub mod models;
pub mod models_page;
pub mod update;

pub use super_voice_app_core::state::{
    assistants, behavior, device_offers, language, model_operations, registry, scripts, shortcuts,
    stage_catalog, staged_picks, voices,
};

/// The Clips tab's state, with libcosmic's editor for the next clip.
pub mod clips {
    pub type ClipsState = super_voice_app_core::state::clips::ClipsState<super::Editor>;
}

/// The Contexts page's state, with libcosmic's editor for the prompt.
pub mod contexts {
    pub use super_voice_app_core::state::contexts::*;
    pub type ContextsState = super_voice_app_core::state::contexts::ContextsState<super::Editor>;
    pub type ContextDraft = super_voice_app_core::state::contexts::ContextDraft<super::Editor>;
}

/// The Notes tab's state, with libcosmic's editor for the open note.
pub mod notes {
    pub use super_voice_app_core::state::notes::*;
    pub type NotesState = super_voice_app_core::state::notes::NotesState<super::Editor>;
    pub type OpenNote = super_voice_app_core::state::notes::OpenNote<super::Editor>;
}

// Re-export commonly used types
pub use editor::Editor;
pub use models::{
    ActionError, AudioTheme, ContextPage, DaemonStatus, ErrorScope, MenuAction, ModelsTab, Page,
    RecordingStatus, Section,
};
pub use super_voice_app_core::state::{
    BlockStage, LanguageResolution, SpeakingStatus, SpeechState, Stage, VoiceChoice,
    VoiceResolution, VoiceState,
};
