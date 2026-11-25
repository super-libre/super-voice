// SPDX-License-Identifier: GPL-3.0-only
//! What a stage's card says about itself in a word or a line: its status
//! (the dot and the word on its pill), and the memory its model needs or
//! uses, and whether that fits.
//!
//! Worked out from the page's own state, so an app draws the card from
//! [`ModelsState`] alone, without the full backend list the older apps
//! kept beside it.

use crate::daemon::backends::BackendInfo;
use crate::rules::status::{
    ModelStatus, classify_block_status, classify_model_status, speaks_in_nothing,
};
use crate::state::Stage;
use crate::state::voice::VoiceState;

use super::{ModelsState, Shown, card, idle_model, running_model};

/// How ready `stage` is, as its card's pill says it: no backend, a backend
/// not set up, nothing loaded (or loading), ready, idle, failed, or for
/// synthesis no voice to speak in. `voice` is the synthesis model's voice,
/// as the card shows it.
#[must_use]
pub fn stage_status(cards: &ModelsState, stage: Stage, voice: &VoiceState) -> ModelStatus {
    let view = cards.view(stage);
    let running = running_model(cards, stage);
    let backends = cards.catalog.backends(stage);
    let status = match (stage.block(), backends) {
        (None, Some(backends)) => classify_model_status(
            view.source.as_deref(),
            backends,
            &cards.secrets,
            running.unwrap_or_default(),
            view.source.as_deref().unwrap_or_default(),
        ),
        // Before the stage's backends are read, the view alone says it.
        (None, None) if view.source.is_none() => ModelStatus::None,
        (None, None) if running.is_some() => ModelStatus::Ready,
        (None, None) => ModelStatus::NotLoaded,
        (Some(_), backends) => {
            classify_block_status(view, backends.unwrap_or_default(), &cards.secrets)
        }
    };
    let no_voice = stage == Stage::Synthesis && speaks_in_nothing(view, voice);
    crate::rules::status::stage_status(
        stage,
        cards.operations.get(stage),
        status,
        idle_model(cards, stage).is_some(),
        no_voice,
    )
}

/// The memory a card's model needs, in bytes, and how it stands against the
/// GPU's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Memory {
    /// The running model uses about this much.
    Uses(u64),
    /// The staged model needs about this much; where it goes says nothing
    /// of fitting (the CPU, or a GPU the daemon reported nothing about).
    Needs(u64),
    /// The staged model needs `needed` on the GPU, which has `free`.
    Fits { needed: u64, free: u64 },
    /// The staged model needs more than the GPU has free: it may not load.
    MayNotFit { needed: u64, free: u64 },
}

/// What `stage`'s card says of its model's memory, or `None` when it says
/// nothing: no model, a model that declares no estimate (an online one), or
/// one that can't run here at all, which the card says instead.
#[must_use]
pub fn memory(cards: &ModelsState, stage: Stage) -> Option<Memory> {
    let card = card(cards, stage);
    let backend = card.backend?;
    match &card.shown {
        Shown::Running { model, .. } => estimate(backend, model).map(Memory::Uses),
        Shown::Staging { model: Some(model) } if !card.no_device => {
            let needed = estimate(backend, model)?;
            if let Some((needed, free)) = card.vram_shortfall {
                return Some(Memory::MayNotFit { needed, free });
            }
            let on_gpu = card.device.is_some_and(|d| !matches!(d, "cpu" | "none"));
            let free = cards
                .gpus
                .first()
                .map(|gpu| gpu.free_bytes.unwrap_or(gpu.total_bytes));
            Some(match free.filter(|_| on_gpu) {
                Some(free) => Memory::Fits { needed, free },
                None => Memory::Needs(needed),
            })
        }
        Shown::Staging { .. } | Shown::Empty => None,
    }
}

/// The bytes `backend` says `model` needs, when it says.
fn estimate(backend: &BackendInfo, model: &str) -> Option<u64> {
    backend
        .models
        .iter()
        .find(|m| m.name == model)
        .map(|m| m.estimated_vram_bytes)
        .filter(|bytes| *bytes > 0)
}

#[cfg(test)]
mod tests {
    use super_voice_shared::models::protocol::GpuInfo;

    use super::super::{Input, update};
    use super::*;
    use crate::daemon::backends::{BackendModel, BackendSecret};
    use crate::daemon::client::{ModelState, StageState};
    use crate::state::model_operations::ModelOperationState;
    use crate::state::voice::VoiceResolution;

    const GIB: u64 = 1024 * 1024 * 1024;
    const SOURCE: &str = "github.com/x/whisper";

    fn backend(vram: u64) -> BackendInfo {
        BackendInfo {
            source: SOURCE.into(),
            name: "Whisper".into(),
            description: String::new(),
            version: "1.0.0".into(),
            kind: "subprocess".into(),
            allowed_hosts: Vec::new(),
            installed_accel: Vec::new(),
            models: vec![BackendModel {
                name: "large".into(),
                provider: String::new(),
                supported_devices: vec!["cpu".into(), "cuda".into()],
                estimated_vram_bytes: vram,
                multilingual: false,
                supported_languages: Vec::new(),
                primary_language: String::new(),
                realtime: false,
                role: "transcription".into(),
                accepts_context: false,
                clones_voices: false,
            }],
            secrets: Vec::new(),
            options: Vec::new(),
            supports: Vec::new(),
        }
    }

    fn view(stage_source: Option<&str>, loaded: bool, state: ModelState) -> StageState {
        StageState {
            source: stage_source.map(String::from),
            name: stage_source.map(|_| "Whisper".into()),
            // A stage with a switch is on while its model is up; on and not
            // up reads as failed.
            enabled: loaded,
            model: stage_source.map(|_| "large".into()),
            loaded,
            state,
            device: None,
        }
    }

    /// Whisper in `stage`, its model `large` staged on `device` (or loaded).
    fn page(stage: Stage, vram: u64, loaded: bool, device: &str) -> ModelsState {
        let mut cards = ModelsState::default();
        cards.catalog.record(stage, vec![backend(vram)]);
        let state = if loaded {
            ModelState::Loaded
        } else {
            ModelState::NotLoaded
        };
        update(
            &mut cards,
            Input::ViewLoaded(stage, Ok(view(Some(SOURCE), loaded, state))),
        );
        update(
            &mut cards,
            Input::DevicesLoaded {
                stage,
                source: SOURCE.into(),
                model: "large".into(),
                result: Ok((vec!["cpu".into(), "cuda".into()], Some(device.into()))),
            },
        );
        update(
            &mut cards,
            Input::GpusLoaded(vec![GpuInfo {
                name: "NVIDIA GeForce RTX 3090".into(),
                vendor: "nvidia".into(),
                total_bytes: 24 * GIB,
                free_bytes: Some(18 * GIB),
                used_bytes: Some(6 * GIB),
                arch_target: None,
            }]),
        );
        cards
    }

    #[test]
    fn an_empty_stage_has_no_backend() {
        let cards = ModelsState::default();
        let voice = VoiceState::default();
        for stage in Stage::ALL {
            assert_eq!(stage_status(&cards, stage, &voice), ModelStatus::None);
        }
    }

    #[test]
    fn a_loaded_model_is_ready_and_a_staged_one_not_loaded() {
        let voice = VoiceState::default();
        let ready = page(Stage::Transcription, 0, true, "cuda");
        assert_eq!(
            stage_status(&ready, Stage::Transcription, &voice),
            ModelStatus::Ready
        );
        let staged = page(Stage::PostProcessing, 0, false, "cuda");
        assert_eq!(
            stage_status(&staged, Stage::PostProcessing, &voice),
            ModelStatus::NotLoaded
        );
    }

    /// A backend chosen before its list is read reads from the view alone.
    #[test]
    fn transcription_reads_from_its_view_before_its_backends() {
        let mut cards = ModelsState::default();
        update(
            &mut cards,
            Input::ViewLoaded(
                Stage::Transcription,
                Ok(view(Some(SOURCE), true, ModelState::Loaded)),
            ),
        );
        assert_eq!(
            stage_status(&cards, Stage::Transcription, &VoiceState::default()),
            ModelStatus::Ready
        );
    }

    #[test]
    fn a_required_secret_not_stored_needs_setup() {
        let mut cards = page(Stage::Transcription, 0, false, "cuda");
        let mut with_key = backend(0);
        with_key.secrets.push(BackendSecret {
            name: "api_key".into(),
            label: Some("API key".into()),
            description: String::new(),
            required: true,
        });
        cards.catalog.record(Stage::Transcription, vec![with_key]);
        assert_eq!(
            stage_status(&cards, Stage::Transcription, &VoiceState::default()),
            ModelStatus::Blocked
        );
    }

    #[test]
    fn a_failed_load_reads_failed() {
        let mut cards = page(Stage::PostProcessing, 0, false, "cuda");
        cards.operations.set(
            Stage::PostProcessing,
            ModelOperationState::Error {
                message: "out of memory".into(),
            },
        );
        assert_eq!(
            stage_status(&cards, Stage::PostProcessing, &VoiceState::default()),
            ModelStatus::Failed
        );
    }

    /// A cloning model with no voice chosen can't speak.
    #[test]
    fn a_synthesis_model_without_a_voice_says_so() {
        let cards = page(Stage::Synthesis, 0, true, "cuda");
        let voice = VoiceState {
            resolution: Some(VoiceResolution {
                effective: None,
                model_override: None,
                default: None,
                kinds: vec!["cloned".into()],
            }),
            choices: Vec::new(),
            target: Some((SOURCE.into(), "large".into())),
        };
        assert_eq!(
            stage_status(&cards, Stage::Synthesis, &voice),
            ModelStatus::NoVoice
        );
    }

    #[test]
    fn a_staged_model_on_the_gpu_fits_or_may_not() {
        let fits = page(Stage::Transcription, 3 * GIB, false, "cuda");
        assert_eq!(
            memory(&fits, Stage::Transcription),
            Some(Memory::Fits {
                needed: 3 * GIB,
                free: 18 * GIB
            })
        );
        let too_big = page(Stage::Transcription, 22 * GIB, false, "cuda");
        assert_eq!(
            memory(&too_big, Stage::Transcription),
            Some(Memory::MayNotFit {
                needed: 22 * GIB,
                free: 18 * GIB
            })
        );
    }

    /// On the CPU the GPU's memory says nothing.
    #[test]
    fn a_staged_model_on_the_cpu_only_needs() {
        let cpu = page(Stage::Transcription, 22 * GIB, false, "cpu");
        assert_eq!(
            memory(&cpu, Stage::Transcription),
            Some(Memory::Needs(22 * GIB))
        );
    }

    #[test]
    fn a_running_model_uses_its_memory() {
        let running = page(Stage::Transcription, 3 * GIB, true, "cuda");
        assert_eq!(
            memory(&running, Stage::Transcription),
            Some(Memory::Uses(3 * GIB))
        );
    }

    /// A model that declares no estimate says nothing.
    #[test]
    fn no_estimate_no_line() {
        let cards = page(Stage::Transcription, 0, false, "cuda");
        assert_eq!(memory(&cards, Stage::Transcription), None);
    }
}
