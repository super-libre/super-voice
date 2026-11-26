// SPDX-License-Identifier: GPL-3.0-only
//! How ready each feature is, and how full the GPU's memory is: what the
//! window shows around its pages, in the sidebar and the status bar.
//!
//! Speech to text is ready when its transcription model is, and text to speech
//! when its synthesis model is: those are the stages that have to run for
//! dictation to type anything, or for speech to play. Each is read from the
//! daemon when it answers, and again when an event says the stage changed. A
//! download or load in flight reads as [`Readiness::Loading`] from the
//! daemon's events, which a read of the stage can't show.
//!
//! Speech to text also needs the daemon to be allowed to type: on macOS,
//! Device Control (Accessibility before macOS 27). While the daemon says it
//! may not ([`ReadinessState::can_type`]), speech to text needs setup,
//! whatever its model; a dictation then goes to the clipboard.
//!
//! The Models page holds more (each backend's secrets, the voice a synthesis
//! model speaks in) and says more on its cards; this is the summary the
//! window keeps whatever page is open.

use std::collections::HashMap;

use super_voice_shared::models::protocol::{GpuInfo, NotificationEvent};

use crate::daemon::client::{self, StageState};
use crate::pages::models;
use crate::rules::status::{Dot, ModelStatus, stage_block_status};
use crate::state::Stage;
use crate::state::model_operations::{LoadCause, ModelOperationState, ModelOperations, Progressed};

/// One of the product's two features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    SpeechToText,
    TextToSpeech,
}

impl Feature {
    pub const ALL: [Self; 2] = [Self::SpeechToText, Self::TextToSpeech];

    /// The stage whose model the feature needs to work.
    #[must_use]
    pub const fn stage(self) -> Stage {
        match self {
            Self::SpeechToText => Stage::Transcription,
            Self::TextToSpeech => Stage::Synthesis,
        }
    }

    /// The feature's name, as a message key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::SpeechToText => "speech-to-text",
            Self::TextToSpeech => "text-to-speech",
        }
    }
}

/// How ready a feature is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// The daemon hasn't said yet, or can't be reached.
    Unknown,
    /// No backend fills its stage.
    NoBackend,
    /// A backend does, with no model up.
    NotLoaded,
    /// Its model is downloading or loading.
    Loading,
    /// Its model is up.
    Ready,
    /// Its model is out of memory until its next use, which loads it.
    Idle,
    /// Its backend needs a key or an option set before it can run.
    NeedsSetup,
    /// Its model failed to come up.
    Failed,
}

impl Readiness {
    /// The colour of its dot: green ready or idle, yellow nothing up yet or
    /// needs setting up, red failed, grey no backend or not known.
    #[must_use]
    pub const fn dot(self) -> Dot {
        match self {
            Self::Ready | Self::Idle => Dot::Green,
            Self::NotLoaded | Self::Loading | Self::NeedsSetup => Dot::Yellow,
            Self::Failed => Dot::Red,
            Self::NoBackend | Self::Unknown => Dot::Grey,
        }
    }

    /// Its words, as a message key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Unknown => "readiness-unknown",
            Self::NoBackend => "readiness-no-backend",
            Self::NotLoaded => "not-loaded",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Idle => "idle",
            Self::NeedsSetup => "status-needs-setup",
            Self::Failed => "failed",
        }
    }
}

/// What the window knows.
#[derive(Debug, Clone, Default)]
pub struct ReadinessState {
    /// The daemon answers. Until it does, every feature is
    /// [`Readiness::Unknown`].
    pub connected: bool,
    /// Each feature's stage, as the daemon last reported it.
    pub views: HashMap<Stage, StageState>,
    /// Each stage's download or load, from the daemon's events.
    pub operations: ModelOperations,
    /// The machine's GPUs, as the daemon last reported them.
    pub gpus: Vec<GpuInfo>,
    /// Whether the daemon may type into other apps, as its status last said:
    /// `None` when it can't tell (Linux) or hasn't said.
    pub can_type: Option<bool>,
}

/// What happened.
#[derive(Debug, Clone)]
pub enum Input {
    /// The daemon answers, first or again: read everything.
    Reached,
    /// The daemon can't be reached: nothing it said holds any more.
    Lost,
    /// Time passed: read the GPU's memory again, which other programs change
    /// too.
    Tick,
    /// A stage was read, or why not.
    Viewed(Stage, Result<StageState, String>),
    /// The GPUs were read.
    GpusRead(Vec<GpuInfo>),
    /// Ask again whether the daemon may type: the app shows how to let it,
    /// and asks every 2 s until it may.
    CheckTyping,
    /// The daemon said whether it may type, or why it couldn't be asked.
    TypingRead(Result<Option<bool>, String>),
    /// The daemon said something about a model.
    Heard(NotificationEvent),
}

/// What the app should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Read the stage ([`run_read_stage`]).
    ReadStage(Stage),
    /// Read the GPUs ([`run_read_gpus`]).
    ReadGpus,
    /// Read whether the daemon may type ([`run_read_typing`]).
    ReadTyping,
}

/// What an input came to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub effects: Vec<Effect>,
}

/// Take in `input`, and say what to do.
pub fn update(known: &mut ReadinessState, input: Input) -> Outcome {
    let effects = match input {
        Input::Reached => {
            known.connected = true;
            known.operations.reset();
            read_all()
        }
        Input::Lost => {
            *known = ReadinessState::default();
            Vec::new()
        }
        Input::Tick if known.connected => vec![Effect::ReadGpus],
        Input::CheckTyping if known.connected => vec![Effect::ReadTyping],
        Input::Tick | Input::CheckTyping => Vec::new(),
        Input::Viewed(stage, Ok(view)) => {
            known.views.insert(stage, view);
            Vec::new()
        }
        Input::Viewed(stage, Err(e)) => {
            log::debug!("couldn't read {stage}: {e}");
            Vec::new()
        }
        Input::GpusRead(gpus) => {
            known.gpus = gpus;
            Vec::new()
        }
        Input::TypingRead(Ok(can_type)) => {
            known.can_type = can_type;
            Vec::new()
        }
        // Left as it was: one failed read says nothing about the permission.
        Input::TypingRead(Err(e)) => {
            log::debug!("couldn't ask whether the daemon may type: {e}");
            Vec::new()
        }
        Input::Heard(event) => heard(known, &event),
    };
    Outcome { effects }
}

fn read_all() -> Vec<Effect> {
    Feature::ALL
        .map(|feature| Effect::ReadStage(feature.stage()))
        .into_iter()
        .chain([Effect::ReadGpus, Effect::ReadTyping])
        .collect()
}

/// What an event means for the features' stages. The Models page's reading of
/// the daemon's events, kept to the two stages this follows.
fn heard(known: &mut ReadinessState, event: &NotificationEvent) -> Vec<Effect> {
    let followed = |stage: Stage| Feature::ALL.iter().any(|f| f.stage() == stage);
    let mut effects = Vec::new();
    for input in models::heard(event) {
        match input {
            models::Input::Progress(stage, progress) if followed(stage) => {
                if known.operations.apply_progress(stage, &progress) == Progressed::Finished {
                    known.operations.set_ready(stage);
                    effects.push(Effect::ReadStage(stage));
                }
            }
            models::Input::LoadStarted { stage, model } if followed(stage) => {
                known.operations.start(
                    stage,
                    ModelOperationState::Loading {
                        target_model: model,
                        cause: LoadCause::Reported,
                        load: None,
                    },
                );
            }
            models::Input::Ready { stage, .. } if followed(stage) => {
                known.operations.set_ready(stage);
                effects.extend([Effect::ReadStage(stage), Effect::ReadGpus]);
            }
            models::Input::Changed(stage) | models::Input::DeviceSwitchFailed(stage, _)
                if followed(stage) =>
            {
                effects.push(Effect::ReadStage(stage));
            }
            _ => {}
        }
    }
    effects.dedup();
    effects
}

/// How ready `feature` is. Speech to text needs setup while the daemon may
/// not type, unless its model is on its way or failed, which say more.
#[must_use]
pub fn readiness(known: &ReadinessState, feature: Feature) -> Readiness {
    let model = model_readiness(known, feature);
    match model {
        Readiness::Ready | Readiness::Idle | Readiness::NotLoaded | Readiness::NoBackend
            if feature == Feature::SpeechToText && typing_blocked(known) =>
        {
            Readiness::NeedsSetup
        }
        _ => model,
    }
}

/// Whether the daemon said it may not type into other apps. Not known is not
/// blocked: Linux can't tell, and an older daemon doesn't say.
#[must_use]
pub fn typing_blocked(known: &ReadinessState) -> bool {
    known.connected && known.can_type == Some(false)
}

/// How ready `feature`'s model is.
fn model_readiness(known: &ReadinessState, feature: Feature) -> Readiness {
    if !known.connected {
        return Readiness::Unknown;
    }
    let stage = feature.stage();
    match known.operations.get(stage) {
        ModelOperationState::Provisioning { .. } | ModelOperationState::Loading { .. } => {
            return Readiness::Loading;
        }
        ModelOperationState::Error { .. } => return Readiness::Failed,
        ModelOperationState::Ready => {}
    }
    let Some(view) = known.views.get(&stage) else {
        return Readiness::Unknown;
    };
    match stage_block_status(stage, view) {
        ModelStatus::None => Readiness::NoBackend,
        ModelStatus::NotLoaded => Readiness::NotLoaded,
        ModelStatus::Ready => Readiness::Ready,
        ModelStatus::Idle => Readiness::Idle,
        ModelStatus::Blocked => Readiness::NeedsSetup,
        ModelStatus::Failed | ModelStatus::NoVoice => Readiness::Failed,
    }
}

/// The model `feature` would load, when its stage has one set and it isn't
/// up or on its way: what the palette's "Load large-v3-turbo" loads.
#[must_use]
pub fn model_to_load(known: &ReadinessState, feature: Feature) -> Option<&str> {
    match model_readiness(known, feature) {
        Readiness::NotLoaded => known.views.get(&feature.stage())?.model.as_deref(),
        _ => None,
    }
}

/// The first GPU's memory, as the status bar shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuMemory {
    /// Its name without the vendor's prefix: `RTX 3090`.
    pub name: String,
    /// Its full name, for a tooltip.
    pub full_name: String,
    /// The bytes in use, when the daemon says.
    pub used: Option<u64>,
    pub total: u64,
}

/// The first GPU's memory, or `None` when the daemon reported no GPU.
#[must_use]
pub fn gpu(state: &ReadinessState) -> Option<GpuMemory> {
    let gpu = state.gpus.first()?;
    Some(GpuMemory {
        name: crate::rules::gpu::short_name(&gpu.name).to_string(),
        full_name: gpu.name.clone(),
        used: gpu.used_bytes,
        total: gpu.total_bytes,
    })
}

/// Read `stage` from the daemon.
pub async fn run_read_stage(stage: Stage) -> Input {
    Input::Viewed(
        stage,
        client::get_stage_view(stage)
            .await
            .map_err(|e| e.to_string()),
    )
}

/// Ask the daemon whether it may type.
pub async fn run_read_typing() -> Input {
    Input::TypingRead(client::get_can_type().await.map_err(|e| e.to_string()))
}

/// Read the machine's GPUs from the daemon.
pub async fn run_read_gpus() -> Input {
    Input::GpusRead(client::get_gpu_info().await.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::daemon::client::ModelState;

    fn view(source: Option<&str>, loaded: bool, state: ModelState) -> StageState {
        StageState {
            source: source.map(ToString::to_string),
            name: source.map(|_| "Whisper".to_string()),
            // A stage is switched on while its model runs.
            enabled: loaded,
            model: source.map(|_| "large-v3-turbo".to_string()),
            loaded,
            state,
            device: None,
        }
    }

    fn event(kind: &str, data: Value) -> NotificationEvent {
        NotificationEvent {
            event_type_field: kind.into(),
            event_type: kind.into(),
            client_id: "daemon".into(),
            timestamp: "0".into(),
            data,
        }
    }

    fn reached() -> ReadinessState {
        let mut state = ReadinessState::default();
        update(&mut state, Input::Reached);
        state
    }

    #[test]
    fn nothing_is_known_until_the_daemon_answers() {
        let state = ReadinessState::default();
        for feature in Feature::ALL {
            assert_eq!(readiness(&state, feature), Readiness::Unknown);
        }
    }

    #[test]
    fn reaching_the_daemon_reads_both_stages_and_the_gpus() {
        let mut state = ReadinessState::default();
        let outcome = update(&mut state, Input::Reached);
        assert_eq!(
            outcome.effects,
            [
                Effect::ReadStage(Stage::Transcription),
                Effect::ReadStage(Stage::Synthesis),
                Effect::ReadGpus,
                Effect::ReadTyping,
            ]
        );
        // Read, but no answer yet.
        assert_eq!(readiness(&state, Feature::SpeechToText), Readiness::Unknown);
    }

    #[test]
    fn each_stage_reads_as_the_daemon_has_it() {
        let mut state = reached();
        let cases = [
            (
                view(None, false, ModelState::NotLoaded),
                Readiness::NoBackend,
            ),
            (
                view(Some("w"), false, ModelState::NotLoaded),
                Readiness::NotLoaded,
            ),
            (view(Some("w"), true, ModelState::Loaded), Readiness::Ready),
            (view(Some("w"), false, ModelState::Idle), Readiness::Idle),
        ];
        for (stage_view, expected) in cases {
            update(
                &mut state,
                Input::Viewed(Stage::Transcription, Ok(stage_view)),
            );
            assert_eq!(readiness(&state, Feature::SpeechToText), expected);
        }
        // Synthesis switched on with nothing up failed to come up.
        let mut failed = view(Some("k"), false, ModelState::NotLoaded);
        failed.enabled = true;
        update(&mut state, Input::Viewed(Stage::Synthesis, Ok(failed)));
        assert_eq!(readiness(&state, Feature::TextToSpeech), Readiness::Failed);
    }

    #[test]
    fn a_download_and_a_load_read_as_loading_until_the_stage_is_ready() {
        let mut state = reached();
        update(
            &mut state,
            Input::Viewed(
                Stage::Synthesis,
                Ok(view(Some("k"), false, ModelState::NotLoaded)),
            ),
        );
        let download = event(
            "download_progress",
            json!({
                "feature": "tts", "model_name": "kokoro-82m", "source": "k", "stage": 1,
                "current_file": "m.bin", "file_index": 0, "total_files": 1,
                "bytes_downloaded": 1, "total_bytes": 2, "percentage": 50.0,
                "status": "downloading", "started_at": "",
            }),
        );
        assert!(
            update(&mut state, Input::Heard(download))
                .effects
                .is_empty()
        );
        assert_eq!(readiness(&state, Feature::TextToSpeech), Readiness::Loading);
        assert_eq!(readiness(&state, Feature::SpeechToText), Readiness::Unknown);

        let ready = event(
            "daemon_status_changed",
            json!({"status": "ready", "feature": "tts", "model_loaded": true}),
        );
        let outcome = update(&mut state, Input::Heard(ready));
        assert_eq!(
            outcome.effects,
            [Effect::ReadStage(Stage::Synthesis), Effect::ReadGpus]
        );
        assert_eq!(
            readiness(&state, Feature::TextToSpeech),
            Readiness::NotLoaded
        );
    }

    #[test]
    fn a_load_started_elsewhere_reads_as_loading() {
        let mut state = reached();
        let loading = event(
            "daemon_status_changed",
            json!({"status": "loading_model", "new_model": "large-v3-turbo"}),
        );
        update(&mut state, Input::Heard(loading));
        assert_eq!(readiness(&state, Feature::SpeechToText), Readiness::Loading);
    }

    #[test]
    fn post_processing_is_not_followed() {
        let mut state = reached();
        let ready = event(
            "daemon_status_changed",
            json!({"status": "ready", "model_loaded": true, "stage": 2}),
        );
        assert!(update(&mut state, Input::Heard(ready)).effects.is_empty());
    }

    #[test]
    fn losing_the_daemon_forgets_what_it_said() {
        let mut state = reached();
        update(
            &mut state,
            Input::Viewed(
                Stage::Transcription,
                Ok(view(Some("w"), true, ModelState::Loaded)),
            ),
        );
        update(&mut state, Input::Lost);
        assert_eq!(readiness(&state, Feature::SpeechToText), Readiness::Unknown);
        assert!(update(&mut state, Input::Tick).effects.is_empty());
    }

    /// While the daemon may not type, speech to text needs setup whatever its
    /// model, and text to speech is untouched. A model on its way or failed
    /// still says so. Not knowing (Linux) is not blocked.
    #[test]
    fn speech_to_text_needs_setup_while_the_daemon_may_not_type() {
        let mut state = reached();
        for stage in [Stage::Transcription, Stage::Synthesis] {
            update(
                &mut state,
                Input::Viewed(stage, Ok(view(Some("w"), true, ModelState::Loaded))),
            );
        }
        update(&mut state, Input::TypingRead(Ok(None)));
        assert_eq!(readiness(&state, Feature::SpeechToText), Readiness::Ready);

        update(&mut state, Input::TypingRead(Ok(Some(false))));
        assert!(typing_blocked(&state));
        assert_eq!(
            readiness(&state, Feature::SpeechToText),
            Readiness::NeedsSetup
        );
        assert_eq!(readiness(&state, Feature::TextToSpeech), Readiness::Ready);

        // A read that fails changes nothing.
        update(&mut state, Input::TypingRead(Err("busy".into())));
        assert!(typing_blocked(&state));

        state.operations.start(
            Stage::Transcription,
            ModelOperationState::Loading {
                target_model: "large-v3-turbo".into(),
                cause: LoadCause::Reported,
                load: None,
            },
        );
        assert_eq!(readiness(&state, Feature::SpeechToText), Readiness::Loading);
        state.operations.set_ready(Stage::Transcription);

        update(&mut state, Input::TypingRead(Ok(Some(true))));
        assert_eq!(readiness(&state, Feature::SpeechToText), Readiness::Ready);
    }

    /// The 2 s check asks only while the daemon answers; losing it forgets
    /// what it said.
    #[test]
    fn typing_is_checked_only_while_the_daemon_answers() {
        let mut state = reached();
        assert_eq!(
            update(&mut state, Input::CheckTyping).effects,
            [Effect::ReadTyping]
        );
        update(&mut state, Input::TypingRead(Ok(Some(false))));
        update(&mut state, Input::Lost);
        assert!(!typing_blocked(&state));
        assert!(update(&mut state, Input::CheckTyping).effects.is_empty());
    }

    /// A model not loaded can still be loaded from the palette while typing
    /// needs setup.
    #[test]
    fn a_model_can_be_loaded_while_typing_needs_setup() {
        let mut state = reached();
        update(
            &mut state,
            Input::Viewed(
                Stage::Transcription,
                Ok(view(Some("w"), false, ModelState::NotLoaded)),
            ),
        );
        update(&mut state, Input::TypingRead(Ok(Some(false))));
        assert_eq!(
            model_to_load(&state, Feature::SpeechToText),
            Some("large-v3-turbo")
        );
    }

    #[test]
    fn only_a_model_that_isnt_up_can_be_loaded() {
        let mut state = reached();
        assert_eq!(model_to_load(&state, Feature::SpeechToText), None);
        update(
            &mut state,
            Input::Viewed(
                Stage::Transcription,
                Ok(view(Some("w"), false, ModelState::NotLoaded)),
            ),
        );
        assert_eq!(
            model_to_load(&state, Feature::SpeechToText),
            Some("large-v3-turbo")
        );
        update(
            &mut state,
            Input::Viewed(
                Stage::Transcription,
                Ok(view(Some("w"), true, ModelState::Loaded)),
            ),
        );
        assert_eq!(model_to_load(&state, Feature::SpeechToText), None);
        update(
            &mut state,
            Input::Viewed(
                Stage::Transcription,
                Ok(view(None, false, ModelState::NotLoaded)),
            ),
        );
        assert_eq!(model_to_load(&state, Feature::SpeechToText), None);
    }

    #[test]
    fn the_gpu_goes_by_its_short_name() {
        let mut state = reached();
        assert_eq!(gpu(&state), None);
        update(
            &mut state,
            Input::GpusRead(vec![GpuInfo {
                name: "NVIDIA GeForce RTX 3090".into(),
                vendor: "nvidia".into(),
                total_bytes: 24 << 30,
                free_bytes: None,
                used_bytes: Some(6 << 30),
                arch_target: None,
            }]),
        );
        let memory = gpu(&state).expect("a GPU");
        assert_eq!(memory.name, "RTX 3090");
        assert_eq!(memory.used, Some(6 << 30));
        assert_eq!(update(&mut state, Input::Tick).effects, [Effect::ReadGpus]);
    }

    #[test]
    fn every_readiness_has_a_dot_and_words() {
        use Readiness as R;
        for readiness in [
            R::Unknown,
            R::NoBackend,
            R::NotLoaded,
            R::Loading,
            R::Ready,
            R::Idle,
            R::NeedsSetup,
            R::Failed,
        ] {
            assert!(
                crate::i18n::LANGUAGE_LOADER.has(readiness.key()),
                "no message {}",
                readiness.key()
            );
        }
        assert_eq!(R::Idle.dot(), Dot::Green);
        assert_eq!(R::Loading.dot(), Dot::Yellow);
        assert_eq!(R::NeedsSetup.dot(), Dot::Yellow);
        assert_eq!(R::Failed.dot(), Dot::Red);
        assert_eq!(R::NoBackend.dot(), Dot::Grey);
        assert_eq!(ModelStatus::Blocked.dot(), Dot::Yellow);
    }
}
