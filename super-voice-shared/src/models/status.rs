// SPDX-License-Identifier: GPL-3.0-only
//! What `GET /v1/status` reports: each feature's model, device and activity.
//!
//! The daemon serves both features from one process, so a status is one per
//! feature, side by side. A client reads the feature it is about — the
//! dictation hotkey reads `stt.busy`, a speech widget `tts.busy`.

use crate::models::protocol::ModelState;
use serde::{Deserialize, Serialize};

/// What `GET /v1/status` answers with.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct VoiceStatus {
    /// Always `success`.
    #[cfg_attr(feature = "openapi", schema(example = "success"))]
    pub status: String,
    /// Speech to text. `busy` while a daemon-mic cycle is active — capture
    /// *and* the transcription and typing that follow it. A toggle hotkey reads
    /// it and calls `POST /stt/transcribe/stop` when true, `POST
    /// /stt/transcribe` when false.
    pub stt: FeatureStatus,
    /// Text to speech. `busy` while an utterance is being synthesized or
    /// played.
    pub tts: FeatureStatus,
}

/// One feature's part of a [`VoiceStatus`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FeatureStatus {
    /// The accelerator the loaded model actually runs on: `cpu`, `cuda`,
    /// `rocm`, `metal`, `vulkan`, or `remote` for a model served over the
    /// network. `unknown` when nothing is loaded.
    #[cfg_attr(feature = "openapi", schema(example = "cuda"))]
    pub device: String,
    /// `false` while the feature's model is still loading, after a failed
    /// switch, or when none is selected.
    pub model_loaded: bool,
    /// The loaded model's name. Absent when `model_loaded` is `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(example = "whisper-tiny"))]
    pub current_model: Option<String>,
    /// The model selected in the feature's first stage, loaded or not. Absent
    /// when none is selected, and from a daemon older than the field.
    ///
    /// `current_model` alone could not tell a model asleep from none at all:
    /// with idle unload on, a selected model is out of memory most of the
    /// time, and `voice status` said "(none loaded)" for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(example = "whisper-tiny"))]
    pub selected_model: Option<String>,
    /// Where `selected_model` stands, as `GET /stt/pipeline/1/model` and
    /// `GET /tts/pipeline/1/model` report it: `loaded`, `idle` (out of memory,
    /// and its next use loads it) or `not_loaded`, which is also what it says
    /// with nothing selected. Absent from a daemon older than the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_state: Option<ModelState>,
    /// Whether the feature is at work right now. See [`VoiceStatus`].
    pub busy: bool,
    /// Speech to text only: whether the daemon may type into other apps. On
    /// macOS, whether it holds Device Control (Accessibility before macOS 27).
    /// Absent where the daemon can't tell without trying (Linux, where the
    /// first dictation builds the keyboard), from `tts`, and from a daemon
    /// older than the field. A dictation that can't be typed is copied to the
    /// clipboard instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_type: Option<bool>,
}
