// SPDX-License-Identifier: GPL-3.0-only
//! How ready a stage is: the rule behind each stage's status dot, and behind
//! the buttons that start work on one.

use crate::daemon::backends::BackendInfo;
use crate::state::Stage;
use crate::state::model_operations::ModelOperationState;

/// The required secrets / options that the user has not yet set for this
/// backend, by human-readable label. Returned in declaration order so the
/// inline warnings are stable across renders. Drives both the inline
/// "{label} must be set." rows and whether the Select button is enabled.
///
/// Takes the daemon-reported "is this secret configured?" map directly, so the
/// rule stays pure and unit-testable.
#[must_use]
pub fn unmet_requirements<'a, S: std::hash::BuildHasher>(
    secret_configured: &std::collections::HashMap<(String, String), bool, S>,
    backend: &'a BackendInfo,
) -> Vec<&'a str> {
    let mut out = Vec::new();
    for secret in &backend.secrets {
        if !secret.required {
            continue;
        }
        let configured = secret_configured
            .get(&(backend.source.clone(), secret.name.clone()))
            .copied()
            .unwrap_or(false);
        if !configured {
            out.push(secret.label.as_deref().unwrap_or(&secret.name));
        }
    }
    for option in &backend.options {
        if !option.required {
            continue;
        }
        let value = option.value.as_deref().unwrap_or("").trim();
        if value.is_empty() {
            out.push(option.label.as_deref().unwrap_or(&option.name));
        }
    }
    out
}

#[cfg(test)]
mod unmet_requirements_tests {
    //! Pin the rule for which secrets/options gate the per-backend Select
    //! button: only `required` ones, the daemon-reported configured map decides
    //! per-secret, and the human-readable `label` (not the wire `name`) is what
    //! surfaces.
    use super::*;
    use crate::daemon::backends::{BackendInfo, BackendModel, BackendOption, BackendSecret};
    use std::collections::HashMap;

    fn backend(secrets: Vec<BackendSecret>, options: Vec<BackendOption>) -> BackendInfo {
        BackendInfo {
            source: "github.com/super-stt/openai".to_string(),
            description: String::new(),
            name: "OpenAI".to_string(),
            version: "1.0.0".to_string(),
            kind: "wasm".to_string(),
            allowed_hosts: Vec::new(),
            installed_accel: Vec::new(),
            models: vec![BackendModel {
                name: "whisper-1".to_string(),
                provider: String::new(),
                supported_devices: vec!["none".to_string()],
                estimated_vram_bytes: 0,
                multilingual: false,
                supported_languages: Vec::new(),
                primary_language: String::new(),
                realtime: false,
                role: "transcription".into(),
                accepts_context: false,
                clones_voices: false,
            }],
            secrets,
            options,
            supports: Vec::new(),
        }
    }

    fn secret(name: &str, label: Option<&str>, required: bool) -> BackendSecret {
        BackendSecret {
            name: name.to_string(),
            label: label.map(str::to_string),
            description: String::new(),
            required,
        }
    }

    fn option_value(
        name: &str,
        label: Option<&str>,
        required: bool,
        value: Option<&str>,
    ) -> BackendOption {
        BackendOption {
            name: name.to_string(),
            label: label.map(str::to_string),
            description: String::new(),
            r#type: None,
            default: None,
            choices: Vec::new(),
            min: None,
            max: None,
            step: None,
            required,
            value: value.map(str::to_string),
        }
    }

    /// A required, unconfigured secret with a label surfaces with the label
    /// (not the `snake_case` wire name).
    #[test]
    fn required_secret_unconfigured_surfaces_label() {
        let bi = backend(
            vec![secret("openai_api_key", Some("OpenAI API key"), true)],
            Vec::new(),
        );
        let map: HashMap<(String, String), bool> = HashMap::new();

        let missing = unmet_requirements(&map, &bi);
        assert_eq!(missing, vec!["OpenAI API key"]);
    }

    /// A required secret with no label falls back to its `name`. The UI is
    /// then no worse than today but no better — every real backend should
    /// supply a label.
    #[test]
    fn required_secret_without_label_falls_back_to_name() {
        let bi = backend(vec![secret("openai_api_key", None, true)], Vec::new());
        let map: HashMap<(String, String), bool> = HashMap::new();

        let missing = unmet_requirements(&map, &bi);
        assert_eq!(missing, vec!["openai_api_key"]);
    }

    /// A required secret that's marked configured in the daemon-reported map is
    /// not surfaced as unmet — Select must be enabled.
    #[test]
    fn configured_secret_is_not_unmet() {
        let bi = backend(
            vec![secret("openai_api_key", Some("OpenAI API key"), true)],
            Vec::new(),
        );
        let mut map = HashMap::new();
        map.insert((bi.source.clone(), "openai_api_key".to_string()), true);

        assert!(unmet_requirements(&map, &bi).is_empty());
    }

    /// A *non*-required secret never surfaces, configured or not — the daemon
    /// doesn't need it for a load, so it doesn't gate the Select button.
    #[test]
    fn non_required_secret_never_surfaces() {
        let bi = backend(
            vec![secret("openai_org", Some("OpenAI Org"), false)],
            Vec::new(),
        );
        let map = HashMap::new();

        assert!(unmet_requirements(&map, &bi).is_empty());
    }

    /// A required option with no effective value surfaces (its `label`); one
    /// with a value (including a manifest default) does not.
    #[test]
    fn required_option_value_gating() {
        let bi = backend(
            Vec::new(),
            vec![option_value("base_url", Some("Base URL"), true, None)],
        );
        let map = HashMap::new();
        assert_eq!(unmet_requirements(&map, &bi), vec!["Base URL"]);

        let bi_with_value = backend(
            Vec::new(),
            vec![option_value(
                "base_url",
                Some("Base URL"),
                true,
                Some("https://api.openai.com"),
            )],
        );
        assert!(unmet_requirements(&map, &bi_with_value).is_empty());
    }

    /// A whitespace-only value is treated as empty — `value = "   "` does not
    /// satisfy a `required` option.
    #[test]
    fn required_option_whitespace_is_empty() {
        let bi = backend(
            Vec::new(),
            vec![option_value(
                "base_url",
                Some("Base URL"),
                true,
                Some("   "),
            )],
        );
        let map = HashMap::new();
        assert_eq!(unmet_requirements(&map, &bi), vec!["Base URL"]);
    }

    /// Multiple unmet requirements are returned in declaration order
    /// (secrets first, then options) — keeps the inline warnings stable
    /// across renders rather than depending on `HashMap` iteration order.
    #[test]
    fn returns_in_declaration_order() {
        let bi = backend(
            vec![
                secret("alpha_key", Some("Alpha key"), true),
                secret("beta_key", Some("Beta key"), true),
            ],
            vec![option_value("base_url", Some("Base URL"), true, None)],
        );
        let map = HashMap::new();
        assert_eq!(
            unmet_requirements(&map, &bi),
            vec!["Alpha key", "Beta key", "Base URL"]
        );
    }
}

/// How ready one stage is. Each app draws it as a colored dot: on its pipeline
/// step, and for transcription and synthesis on their feature's pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelStatus {
    /// No backend is selected. Gray.
    None,
    /// A backend is selected but at least one required secret/option is unset.
    /// Red.
    Blocked,
    /// Backend selected and fully configured, but no model is up yet: none
    /// loaded, or one still downloading or loading. Yellow.
    NotLoaded,
    /// A model is loaded and ready. Green.
    Ready,
    /// The model is out of memory until its next use, which loads it: ready
    /// as far as the user is concerned. Green, as [`Self::Ready`] is.
    Idle,
    /// The stage is switched on but its model is not running: it failed to
    /// come up, or the last load failed. Red.
    Failed,
    /// The synthesis model is loaded, or loads on its next use, but has no
    /// voice to speak in: a cloning model before a voice is chosen. Red, as
    /// [`Self::Blocked`] is: the next use would fail.
    NoVoice,
}

/// The [`ModelStatus`] of transcription, from only what the rule depends on.
/// Two arms surface as [`ModelStatus::None`]: there's no
/// active backend at all, OR the daemon reports one but its catalog entry
/// is gone (uninstalled while running). Both are "no backend" from the
/// user's perspective.
#[must_use]
pub fn classify_model_status<S: std::hash::BuildHasher>(
    active_backend: Option<&str>,
    backends: &[BackendInfo],
    secret_configured: &std::collections::HashMap<(String, String), bool, S>,
    current_model: &str,
    current_source: &str,
) -> ModelStatus {
    let Some(active_source) = active_backend else {
        return ModelStatus::None;
    };
    let Some(backend) = backends.iter().find(|b| b.source.as_str() == active_source) else {
        return ModelStatus::None;
    };
    if !unmet_requirements(secret_configured, backend).is_empty() {
        return ModelStatus::Blocked;
    }
    let model_loaded = !current_model.is_empty() && current_source == active_source;
    if model_loaded {
        ModelStatus::Ready
    } else {
        ModelStatus::NotLoaded
    }
}

/// The [`ModelStatus`] of any stage, from its parts.
///
/// `status` is the stage's own reading: [`classify_model_status`] for
/// transcription, [`classify_block_status`] for the others. A download or
/// load in flight reads as [`NotLoaded`](ModelStatus::NotLoaded) whatever
/// that says, since nothing is up yet but nothing is wrong either; a failed
/// one reads as [`Failed`](ModelStatus::Failed) until the next attempt. A
/// model out of memory until its next use (`idle`) reads as ready, and a
/// synthesis model with nothing to speak in (`no_voice`, see
/// [`speaks_in_nothing`]) reads as [`NoVoice`](ModelStatus::NoVoice).
#[must_use]
pub fn stage_status(
    stage: Stage,
    operation: &ModelOperationState,
    status: ModelStatus,
    idle: bool,
    no_voice: bool,
) -> ModelStatus {
    match operation {
        ModelOperationState::Provisioning { .. } | ModelOperationState::Loading { .. } => {
            return ModelStatus::NotLoaded;
        }
        ModelOperationState::Error { .. } => return ModelStatus::Failed,
        ModelOperationState::Ready => {}
    }
    let status = idle_when(status, idle);
    if stage == Stage::Synthesis {
        return without_voice(status, no_voice);
    }
    status
}

/// Why a stage cannot be used right now. Each app says it in its own words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotReady {
    /// The synthesis model has no voice to speak in.
    NoVoice,
    /// Text to speech's model is not up, or not set up.
    TextToSpeech,
    /// Speech to text's model is not up, or not set up.
    SpeechToText,
}

/// Why `stage`, whose status is `status`, cannot be used right now, or `None`
/// when it can: its model is loaded, or out of memory until its next use,
/// which loads it.
///
/// For the pages that start work on a stage, Notes and Clips, which keep their
/// button off and say this instead of letting a take or a clip fail. Anything
/// short of ready counts: no backend, one not set up, a model loading, not
/// loaded or failed, and for synthesis no voice to speak in.
#[must_use]
pub const fn not_ready(stage: Stage, status: ModelStatus) -> Option<NotReady> {
    match (status, stage) {
        (ModelStatus::Ready | ModelStatus::Idle, _) => None,
        (ModelStatus::NoVoice, _) => Some(NotReady::NoVoice),
        (_, Stage::Synthesis) => Some(NotReady::TextToSpeech),
        (_, _) => Some(NotReady::SpeechToText),
    }
}

/// A stage's [`ModelStatus`] from its block alone (`GET /{feature}/pipeline/{n}`
/// and its model), for an app that reads only that: the header of the macOS
/// app, so far.
///
/// It says what the block can: no backend ([`None`](ModelStatus::None)),
/// a model chosen but not up ([`NotLoaded`](ModelStatus::NotLoaded)), loaded
/// ([`Ready`](ModelStatus::Ready)), asleep ([`Idle`](ModelStatus::Idle)), and
/// for a stage with a switch, on but not running
/// ([`Failed`](ModelStatus::Failed)). It can't see a backend left unset or a
/// voice missing: [`stage_status`] can, from the fuller state the settings app
/// holds.
#[must_use]
pub fn stage_block_status(
    stage: Stage,
    block: &crate::daemon::client::v1::pipeline::StageState,
) -> ModelStatus {
    let status = match stage.block() {
        None if block.source.is_none() => ModelStatus::None,
        None if block.loaded => ModelStatus::Ready,
        None => ModelStatus::NotLoaded,
        Some(_) => classify_block_status(block, &[], &std::collections::HashMap::new()),
    };
    idle_when(
        status,
        block.state == crate::daemon::client::ModelState::Idle,
    )
}

/// Whether the synthesis stage's model has nothing to speak in, as far as the
/// app knows.
///
/// The voice the app holds is for the model on the synthesis card, which can
/// be a staged model rather than the one in the stage. Only a resolution for
/// the stage's own model counts: for any other, the answer is unknown, and an
/// unknown is not a warning.
#[must_use]
pub fn speaks_in_nothing(
    block: &crate::daemon::client::v1::pipeline::StageState,
    voice: &crate::state::VoiceState,
) -> bool {
    let (Some(source), Some(model)) = (block.source.as_deref(), block.model.as_deref()) else {
        return false;
    };
    let Some((voice_source, voice_model)) = voice.target.as_ref() else {
        return false;
    };
    voice_source == source
        && voice_model == model
        && voice
            .resolution
            .as_ref()
            .is_some_and(crate::state::VoiceResolution::has_no_voice)
}

/// `status` for a synthesis stage, when its model has `no_voice`. Only a stage
/// that would read as ready changes: one that is not set up, not running or
/// not loaded already says what is wrong first.
fn without_voice(status: ModelStatus, no_voice: bool) -> ModelStatus {
    match status {
        ModelStatus::Ready | ModelStatus::Idle if no_voice => ModelStatus::NoVoice,
        other => other,
    }
}

/// `status` for a stage whose model is out of memory until its next use, when
/// `idle`. Only the states that say nothing is up become
/// [`Idle`](ModelStatus::Idle): a backend that is not set up stays
/// [`Blocked`](ModelStatus::Blocked), since the next use would fail.
fn idle_when(status: ModelStatus, idle: bool) -> ModelStatus {
    match status {
        ModelStatus::NotLoaded | ModelStatus::Failed if idle => ModelStatus::Idle,
        other => other,
    }
}

/// The [`ModelStatus`] of a stage the app reads back from the daemon, from
/// only what the rule depends on.
///
/// Unlike transcription, such a stage carries a switch: `enabled` with
/// nothing `loaded` is a stage meant to be running that is not, which is the
/// one state the card warns about.
#[must_use]
pub fn classify_block_status<S: std::hash::BuildHasher>(
    block: &crate::daemon::client::v1::pipeline::StageState,
    backends: &[BackendInfo],
    secret_configured: &std::collections::HashMap<(String, String), bool, S>,
) -> ModelStatus {
    let Some(source) = block.source.as_deref() else {
        return ModelStatus::None;
    };
    if let Some(backend) = backends.iter().find(|b| b.source == source)
        && !unmet_requirements(secret_configured, backend).is_empty()
    {
        return ModelStatus::Blocked;
    }
    match (block.enabled, block.loaded) {
        (true, true) => ModelStatus::Ready,
        (true, false) => ModelStatus::Failed,
        (false, _) => ModelStatus::NotLoaded,
    }
}

#[cfg(test)]
mod model_status_tests {
    //! Pin the status-dot rule. The states are: no backend selected → gray;
    //! backend selected but requirements unmet → red; ready but no model
    //! loaded → yellow; model loaded → green; out of memory until its next
    //! use → green; switched on but not running → red; a synthesis model with
    //! nothing to speak in → red.
    use super::*;
    use crate::daemon::backends::{BackendInfo, BackendModel, BackendSecret};
    use crate::daemon::client::v1::pipeline::StageState;
    use std::collections::HashMap;

    fn backend_with_required_secret() -> BackendInfo {
        BackendInfo {
            source: "github.com/super-stt/openai".to_string(),
            description: String::new(),
            name: "OpenAI".to_string(),
            version: "1.0.0".to_string(),
            kind: "wasm".to_string(),
            allowed_hosts: Vec::new(),
            installed_accel: Vec::new(),
            models: vec![BackendModel {
                name: "whisper-1".to_string(),
                provider: String::new(),
                supported_devices: vec!["none".to_string()],
                estimated_vram_bytes: 0,
                multilingual: false,
                supported_languages: Vec::new(),
                primary_language: String::new(),
                realtime: false,
                role: "transcription".into(),
                accepts_context: false,
                clones_voices: false,
            }],
            secrets: vec![BackendSecret {
                name: "openai_api_key".to_string(),
                label: Some("OpenAI API key".to_string()),
                description: String::new(),
                required: true,
            }],
            options: Vec::new(),
            supports: Vec::new(),
        }
    }

    /// No active backend → gray dot regardless of what else is in state.
    #[test]
    fn no_active_backend_is_none() {
        let backends = vec![backend_with_required_secret()];
        let map = HashMap::new();
        assert_eq!(
            classify_model_status(None, &backends, &map, "", ""),
            ModelStatus::None,
        );
    }

    /// Active backend whose catalog entry is gone (e.g. uninstalled while
    /// running) still reads as "no backend" — the daemon's state is stale,
    /// and the dot shouldn't lie about readiness.
    #[test]
    fn active_backend_missing_from_catalog_is_none() {
        let map = HashMap::new();
        assert_eq!(
            classify_model_status(Some("github.com/super-stt/openai"), &[], &map, "", "",),
            ModelStatus::None,
        );
    }

    /// Active backend with an unmet required secret → red, even if a model
    /// from another backend happens to be loaded (which shouldn't really
    /// happen after `set_active_backend` unloads on switch, but the dot
    /// should still reflect the current backend's state).
    #[test]
    fn unmet_requirement_is_blocked() {
        let backends = vec![backend_with_required_secret()];
        let map = HashMap::new();
        assert_eq!(
            classify_model_status(Some("github.com/super-stt/openai"), &backends, &map, "", "",),
            ModelStatus::Blocked,
        );
    }

    /// All requirements satisfied but no model loaded for the active backend
    /// → yellow.
    #[test]
    fn requirements_met_no_model_is_not_loaded() {
        let backends = vec![backend_with_required_secret()];
        let mut map = HashMap::new();
        map.insert(
            (backends[0].source.clone(), "openai_api_key".to_string()),
            true,
        );
        assert_eq!(
            classify_model_status(Some("github.com/super-stt/openai"), &backends, &map, "", "",),
            ModelStatus::NotLoaded,
        );
    }

    /// A loaded model from a *different* source than the active backend is
    /// not "this backend ready" — the dot stays yellow until the user picks
    /// a model from the active backend.
    #[test]
    fn loaded_model_from_other_source_is_not_loaded() {
        let backends = vec![backend_with_required_secret()];
        let mut map = HashMap::new();
        map.insert(
            (backends[0].source.clone(), "openai_api_key".to_string()),
            true,
        );
        assert_eq!(
            classify_model_status(
                Some("github.com/super-stt/openai"),
                &backends,
                &map,
                "voxtral-mini-latest",
                "github.com/super-stt/mistral",
            ),
            ModelStatus::NotLoaded,
        );
    }

    /// Requirements met *and* a model from this backend is loaded → green.
    #[test]
    fn loaded_model_from_active_backend_is_ready() {
        let backends = vec![backend_with_required_secret()];
        let mut map = HashMap::new();
        map.insert(
            (backends[0].source.clone(), "openai_api_key".to_string()),
            true,
        );
        assert_eq!(
            classify_model_status(
                Some("github.com/super-stt/openai"),
                &backends,
                &map,
                "whisper-1",
                "github.com/super-stt/openai",
            ),
            ModelStatus::Ready,
        );
    }

    fn block(source: Option<&str>, enabled: bool, loaded: bool) -> StageState {
        StageState {
            source: source.map(ToString::to_string),
            name: None,
            enabled,
            model: source.map(|_| "whisper-1".to_string()),
            loaded,
            state: if loaded {
                crate::daemon::client::ModelState::Loaded
            } else {
                crate::daemon::client::ModelState::NotLoaded
            },
            device: None,
        }
    }

    const OPENAI: &str = "github.com/super-stt/openai";

    #[test]
    fn an_empty_block_has_no_backend() {
        assert_eq!(
            classify_block_status(&block(None, false, false), &[], &HashMap::new()),
            ModelStatus::None,
        );
    }

    /// A block needs its backend set up before anything else about it counts.
    #[test]
    fn a_block_whose_backend_is_not_set_up_is_blocked() {
        let backends = vec![backend_with_required_secret()];
        assert_eq!(
            classify_block_status(&block(Some(OPENAI), true, true), &backends, &HashMap::new()),
            ModelStatus::Blocked,
        );
    }

    #[test]
    fn a_switched_on_block_follows_its_model() {
        let backends = vec![backend_with_required_secret()];
        let mut map = HashMap::new();
        map.insert((OPENAI.to_string(), "openai_api_key".to_string()), true);
        assert_eq!(
            classify_block_status(&block(Some(OPENAI), true, true), &backends, &map),
            ModelStatus::Ready,
        );
        // Meant to run but not running: the state the card warns about.
        assert_eq!(
            classify_block_status(&block(Some(OPENAI), true, false), &backends, &map),
            ModelStatus::Failed,
        );
    }

    /// Unloaded on purpose is waiting, not wrong.
    #[test]
    fn a_switched_off_block_is_not_loaded() {
        assert_eq!(
            classify_block_status(&block(Some(OPENAI), false, false), &[], &HashMap::new()),
            ModelStatus::NotLoaded,
        );
    }

    /// A model out of memory until its next use is ready, whether the stage
    /// would read as switched on and not running, or as nothing loaded.
    #[test]
    fn an_idle_stage_is_ready_and_idle() {
        assert_eq!(idle_when(ModelStatus::Failed, true), ModelStatus::Idle);
        assert_eq!(idle_when(ModelStatus::NotLoaded, true), ModelStatus::Idle);
        assert_eq!(idle_when(ModelStatus::Failed, false), ModelStatus::Failed);
        assert_eq!(
            idle_when(ModelStatus::NotLoaded, false),
            ModelStatus::NotLoaded
        );
    }

    /// The next use of a backend that is not set up would fail, so it is not
    /// idle, and neither is a stage already running or with no backend.
    #[test]
    fn only_a_stage_with_nothing_up_is_idle() {
        for status in [ModelStatus::Blocked, ModelStatus::Ready, ModelStatus::None] {
            assert_eq!(idle_when(status, true), status);
        }
    }

    fn voice_state(
        target: Option<(&str, &str)>,
        effective: Option<&str>,
        kinds: &[&str],
    ) -> crate::state::VoiceState {
        crate::state::VoiceState {
            resolution: Some(crate::state::VoiceResolution {
                effective: effective.map(ToString::to_string),
                kinds: kinds.iter().map(ToString::to_string).collect(),
                ..Default::default()
            }),
            choices: Vec::new(),
            target: target.map(|(s, m)| (s.to_string(), m.to_string())),
        }
    }

    /// A cloning model with no voice chosen cannot speak, so its stage is not
    /// ready, loaded or idle: the bug was a green dot over the card's warning.
    #[test]
    fn a_synthesis_model_with_no_voice_is_not_ready() {
        let stage = block(Some(OPENAI), true, true);
        let voice = voice_state(Some((OPENAI, "whisper-1")), None, &["cloned"]);
        assert!(speaks_in_nothing(&stage, &voice));
        assert_eq!(
            without_voice(ModelStatus::Ready, true),
            ModelStatus::NoVoice
        );
        assert_eq!(without_voice(ModelStatus::Idle, true), ModelStatus::NoVoice);
    }

    /// A voice in effect, or a model that speaks in described voices, has
    /// something to speak in.
    #[test]
    fn a_voice_or_a_description_is_something_to_speak_in() {
        let stage = block(Some(OPENAI), true, true);
        let chosen = voice_state(Some((OPENAI, "whisper-1")), Some("voice:1"), &["cloned"]);
        assert!(!speaks_in_nothing(&stage, &chosen));
        let described = voice_state(Some((OPENAI, "whisper-1")), None, &["described"]);
        assert!(!speaks_in_nothing(&stage, &described));
    }

    /// The app holds the voice of the model on the card. When that is another
    /// model, or none was read yet, the stage's own voice is unknown, and an
    /// unknown is not a warning.
    #[test]
    fn another_models_voice_says_nothing_about_this_stage() {
        let stage = block(Some(OPENAI), true, true);
        let other = voice_state(Some((OPENAI, "tts-2")), None, &["cloned"]);
        assert!(!speaks_in_nothing(&stage, &other));
        let unread = voice_state(None, None, &["cloned"]);
        assert!(!speaks_in_nothing(&stage, &unread));
        assert!(!speaks_in_nothing(&block(None, false, false), &unread));
    }

    /// What is already wrong with a stage is said first: a backend not set up,
    /// a model not running, or nothing loaded.
    #[test]
    fn no_voice_only_replaces_ready() {
        for status in [
            ModelStatus::None,
            ModelStatus::Blocked,
            ModelStatus::NotLoaded,
            ModelStatus::Failed,
        ] {
            assert_eq!(without_voice(status, true), status);
        }
        assert_eq!(without_voice(ModelStatus::Ready, false), ModelStatus::Ready);
    }

    #[test]
    fn work_in_flight_or_failed_wins_over_the_stages_reading() {
        for stage in Stage::ALL {
            let loading = ModelOperationState::Loading {
                target_model: "m".to_string(),
                status_message: String::new(),
                load: None,
            };
            assert_eq!(
                stage_status(stage, &loading, ModelStatus::Ready, false, false),
                ModelStatus::NotLoaded
            );
            let failed = ModelOperationState::Error {
                message: "no".to_string(),
            };
            assert_eq!(
                stage_status(stage, &failed, ModelStatus::Ready, true, false),
                ModelStatus::Failed
            );
        }
    }

    #[test]
    fn only_synthesis_reads_the_voice() {
        let ready = ModelOperationState::Ready;
        assert_eq!(
            stage_status(Stage::Synthesis, &ready, ModelStatus::Ready, false, true),
            ModelStatus::NoVoice
        );
        assert_eq!(
            stage_status(
                Stage::Transcription,
                &ready,
                ModelStatus::Ready,
                false,
                true
            ),
            ModelStatus::Ready
        );
        assert_eq!(
            stage_status(Stage::Synthesis, &ready, ModelStatus::Failed, true, false),
            ModelStatus::Idle
        );
    }

    #[test]
    fn not_ready_names_the_feature_or_the_voice() {
        assert_eq!(not_ready(Stage::Transcription, ModelStatus::Idle), None);
        assert_eq!(not_ready(Stage::Synthesis, ModelStatus::Ready), None);
        assert_eq!(
            not_ready(Stage::Synthesis, ModelStatus::NoVoice),
            Some(NotReady::NoVoice)
        );
        assert_eq!(
            not_ready(Stage::Synthesis, ModelStatus::NotLoaded),
            Some(NotReady::TextToSpeech)
        );
        for stage in [Stage::Transcription, Stage::PostProcessing] {
            assert_eq!(
                not_ready(stage, ModelStatus::Blocked),
                Some(NotReady::SpeechToText)
            );
        }
    }
}

#[cfg(test)]
mod stage_block_status_tests {
    use super::{ModelStatus, Stage, stage_block_status};
    use crate::daemon::client::ModelState;
    use crate::daemon::client::v1::pipeline::StageState;

    fn block(source: Option<&str>, enabled: bool, state: ModelState) -> StageState {
        StageState {
            source: source.map(str::to_string),
            enabled,
            model: source.map(|_| "m".to_string()),
            loaded: state == ModelState::Loaded,
            state,
            ..StageState::default()
        }
    }

    #[test]
    fn transcription_reads_no_backend_not_loaded_loaded_or_asleep() {
        let t = Stage::Transcription;
        assert_eq!(
            stage_block_status(t, &block(None, true, ModelState::NotLoaded)),
            ModelStatus::None
        );
        assert_eq!(
            stage_block_status(t, &block(Some("b"), true, ModelState::NotLoaded)),
            ModelStatus::NotLoaded
        );
        assert_eq!(
            stage_block_status(t, &block(Some("b"), true, ModelState::Loaded)),
            ModelStatus::Ready
        );
        assert_eq!(
            stage_block_status(t, &block(Some("b"), true, ModelState::Idle)),
            ModelStatus::Idle
        );
    }

    #[test]
    fn synthesis_on_but_not_running_has_failed_unless_asleep() {
        let s = Stage::Synthesis;
        assert_eq!(
            stage_block_status(s, &block(Some("b"), true, ModelState::NotLoaded)),
            ModelStatus::Failed
        );
        assert_eq!(
            stage_block_status(s, &block(Some("b"), true, ModelState::Idle)),
            ModelStatus::Idle
        );
        assert_eq!(
            stage_block_status(s, &block(Some("b"), false, ModelState::NotLoaded)),
            ModelStatus::NotLoaded
        );
        assert_eq!(
            stage_block_status(s, &block(None, false, ModelState::NotLoaded)),
            ModelStatus::None
        );
    }
}
