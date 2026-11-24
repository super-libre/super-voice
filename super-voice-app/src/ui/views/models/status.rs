// SPDX-License-Identifier: GPL-3.0-only
//! How each stage's readiness is drawn: its dot's color and its words. The
//! rule itself is the core's (`rules::status`); this module reads the app's
//! fields into it.

pub(super) use super_voice_app_core::rules::status::{ModelStatus, unmet_requirements};
use super_voice_app_core::rules::status::{
    NotReady, classify_block_status, classify_model_status, not_ready, speaks_in_nothing,
};

use crate::core::app::AppModel;
use crate::state::{BlockStage, Stage};

/// How this app draws a [`ModelStatus`].
pub(super) trait StatusLook {
    /// The color of this state's dot: gray for no backend, yellow for nothing
    /// loaded or loading, green for ready or idle, red for setup incomplete,
    /// not running, or no voice to speak in.
    fn color(self) -> fn(&cosmic::Theme) -> cosmic::iced::Color;

    /// The hover text that names this state, after the feature's name.
    fn detail(self) -> String;
}

impl StatusLook for ModelStatus {
    fn color(self) -> fn(&cosmic::Theme) -> cosmic::iced::Color {
        match self {
            Self::None => |t| t.cosmic().palette.neutral_5.into(),
            Self::Blocked | Self::Failed | Self::NoVoice => |t| t.cosmic().destructive.base.into(),
            Self::NotLoaded => |t| t.cosmic().warning.base.into(),
            Self::Ready | Self::Idle => |t| t.cosmic().success.base.into(),
        }
    }

    fn detail(self) -> String {
        match self {
            Self::None => crate::fl!("no-backend-selected"),
            Self::Blocked => crate::fl!("backend-setup-incomplete"),
            Self::NotLoaded => crate::fl!("no-model-loaded"),
            Self::Ready => crate::fl!("model-loaded-and-ready"),
            Self::Idle => crate::fl!("model-ready"),
            Self::Failed => crate::fl!("the-selected-model-isn-t-loaded"),
            // The words the card's voice dropdown shows for the same state.
            Self::NoVoice => crate::fl!("no-cloned-voice-chosen"),
        }
    }
}

/// The [`ModelStatus`] of any stage, read from the app.
pub(super) fn stage_status(app: &AppModel, stage: Stage) -> ModelStatus {
    let status = match stage.block() {
        None => classify_model_status(
            app.models_page.active_backend.as_deref(),
            &app.backends,
            &app.backend_secret_configured,
            &app.current_model,
            &app.current_source,
        ),
        Some(block) => classify_block_status(
            app.block(block),
            &app.backends,
            &app.backend_secret_configured,
        ),
    };
    let no_voice = stage == Stage::Synthesis
        && speaks_in_nothing(app.block(BlockStage::Synthesis), &app.voice);
    super_voice_app_core::rules::status::stage_status(
        stage,
        app.model_operations.get(stage),
        status,
        app.idle_model(stage).is_some(),
        no_voice,
    )
}

/// Why `stage` cannot be used right now, in the words the Notes and Clips
/// pages show beside their button, or `None` when it can.
pub(in crate::ui::views) fn not_ready_reason(app: &AppModel, stage: Stage) -> Option<String> {
    not_ready(stage, stage_status(app, stage)).map(|reason| match reason {
        NotReady::NoVoice => crate::fl!("choose-a-voice-on-the-models-page"),
        NotReady::TextToSpeech => crate::fl!("text-to-speech-is-not-ready"),
        NotReady::SpeechToText => crate::fl!("speech-to-text-is-not-ready"),
    })
}
