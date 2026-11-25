// SPDX-License-Identifier: GPL-3.0-only
//! Made-up state for `just app-shot`, which never reaches the daemon.
//!
//! By default: Whisper's `large-v3-turbo` ready on CUDA for transcription,
//! S1-mini's `s1-mini-q4_k_m` picked for post-processing and not loaded, and
//! Kokoro's `kokoro-82m` downloading for synthesis. Scenes:
//!
//! - `empty`: post-processing, or synthesis, has no backend.
//! - `failed`: its last load failed.
//! - `no-fit`: its model needs more than the GPU has free.
//! - `openai`: transcription is the `OpenAI` backend, its API key not set (speech to text).
//! - `configure`: the Configure sheet open over transcription's backend
//!   (speech to text), or synthesis's (text to speech, as `configure-tts`).
//! - `build`: asked whether to download Whisper's Vulkan build.

use gpui_kit::{Context, Window};
use serde_json::json;
use super_voice_app_core::daemon::backends::BackendInfo;
use super_voice_app_core::daemon::client::v1::contexts::ContextCatalog;
use super_voice_app_core::daemon::client::{BackendContext, ModelState, StageState};
use super_voice_app_core::pages::model_language::{self as language, ModelKey};
use super_voice_app_core::pages::models::{self as page, Input};
use super_voice_app_core::pages::readiness::Feature;
use super_voice_app_core::state::Stage;
use super_voice_app_core::state::language::LanguageResolution;
use super_voice_app_core::state::model_operations::ModelOperationState;
use super_voice_app_core::state::voice::{VoiceChoice, VoiceResolution, VoiceState};
use super_voice_shared::models::protocol::{DownloadProgress, GpuInfo};

use super::ModelsPage;

const GIB: u64 = 1024 * 1024 * 1024;
const WHISPER: &str = "github.com/super-libre/super-stt-whisper";
const OPENAI: &str = "github.com/super-libre/super-stt-openai";
const S1_MINI: &str = "github.com/super-libre/super-stt-s1-mini";
const KOKORO: &str = "github.com/super-libre/super-tts-kokoro";

fn backend(value: serde_json::Value) -> BackendInfo {
    serde_json::from_value(value).expect("a made-up backend")
}

fn whisper() -> BackendInfo {
    backend(json!({
        "source": WHISPER, "name": "Whisper", "version": "0.2.1",
        "description": "Local Whisper speech to text.",
        "installed_accel": ["cpu", "cuda"], "supports": ["gpu", "cpu"],
        "models": [
            { "name": "large-v3-turbo", "supported_devices": ["cuda", "vulkan", "cpu"],
              "estimated_vram_bytes": 3_328_599_655_u64, "multilingual": true },
            { "name": "base", "supported_devices": ["cuda", "vulkan", "cpu"],
              "estimated_vram_bytes": 412_316_860_u64, "multilingual": true },
        ],
        "secrets": [],
        "options": [
            { "name": "beam_size", "label": "Beam size", "description": "Wider is more accurate, and slower.",
              "choices": ["1", "3", "5"], "default": "5", "value": "5" },
            { "name": "vad", "label": "Skip silence", "description": "Leave out the quiet parts before transcribing.",
              "type": "bool", "default": "true" },
        ],
    }))
}

fn openai() -> BackendInfo {
    backend(json!({
        "source": OPENAI, "name": "OpenAI", "version": "0.5.0",
        "description": "OpenAI's hosted speech to text.",
        "supports": ["online", "dictation_context"],
        "models": [{ "name": "gpt-4o-transcribe", "supported_devices": ["none"] }],
        "secrets": [{ "name": "api_key", "label": "API key", "description": "Your OpenAI API key.", "required": true }],
        "options": [
            { "name": "stream", "label": "Stream", "description": "Send audio as it is recorded.",
              "type": "bool", "default": "true" },
            { "name": "speed", "label": "Speed", "description": "Faster, or more accurate.",
              "choices": ["Fast", "Balanced", "Accurate"], "default": "Balanced", "value": "Balanced" },
            { "name": "base_url", "label": "Base URL", "description": "Where requests go.",
              "default": "https://api.openai.com/v1", "value": "https://gateway.example.com/v1" },
        ],
    }))
}

fn s1_mini(vram: u64) -> BackendInfo {
    backend(json!({
        "source": S1_MINI, "name": "S1-mini", "version": "0.3.0",
        "description": "Cleans up dictated text on this computer.",
        "installed_accel": ["cpu", "cuda"], "supports": ["gpu", "cpu", "dictation_context"],
        "models": [
            { "name": "s1-mini-q4_k_m", "role": "post_processor", "supported_devices": ["cuda", "cpu"],
              "estimated_vram_bytes": vram, "accepts_context": true },
            { "name": "s1-mini-q8_0", "role": "post_processor", "supported_devices": ["cuda", "cpu"],
              "estimated_vram_bytes": 4_509_715_660_u64, "accepts_context": true },
        ],
        "secrets": [], "options": [],
    }))
}

fn kokoro() -> BackendInfo {
    backend(json!({
        "source": KOKORO, "name": "Kokoro", "version": "1.1.0",
        "description": "Small local text to speech with built-in voices.",
        "installed_accel": ["cpu", "cuda"], "supports": ["gpu", "cpu"],
        "models": [{ "name": "kokoro-82m", "role": "synthesis", "supported_devices": ["cuda", "cpu"],
                     "estimated_vram_bytes": 429_496_729_u64, "multilingual": true }],
        "secrets": [], "options": [],
    }))
}

fn qwen_tts() -> BackendInfo {
    backend(json!({
        "source": "github.com/super-libre/super-tts-qwen", "name": "Qwen TTS", "version": "0.2.0",
        "description": "Qwen3 text to speech, with cloned voices.",
        "installed_accel": ["cuda"], "supports": ["gpu"],
        "models": [{ "name": "qwen3-tts-1.7b", "role": "synthesis", "supported_devices": ["cuda"],
                     "estimated_vram_bytes": 22_548_578_304_u64, "clones_voices": true }],
        "secrets": [], "options": [],
    }))
}

fn view(source: &str, name: &str, model: &str, loaded: bool) -> StageState {
    StageState {
        source: Some(source.into()),
        name: Some(name.into()),
        enabled: loaded,
        model: Some(model.into()),
        loaded,
        state: if loaded {
            ModelState::Loaded
        } else {
            ModelState::NotLoaded
        },
        device: None,
    }
}

/// Take `input`, and drop what it asks the daemon.
fn take(page: &mut ModelsPage, input: Input) {
    page::update(&mut page.models, input);
}

/// `model` of `source` in `stage`, staged on `device`.
fn fill(
    page: &mut ModelsPage,
    stage: Stage,
    backends: Vec<BackendInfo>,
    view: StageState,
    device: &str,
) {
    let (source, model) = (
        view.source.clone().unwrap_or_default(),
        view.model.clone().unwrap_or_default(),
    );
    take(page, Input::BackendsLoaded(stage, Ok(backends)));
    take(page, Input::ViewLoaded(stage, Ok(view)));
    let devices: Vec<String> = page
        .models
        .catalog
        .backend(stage, &source)
        .and_then(|b| b.models.iter().find(|m| m.name == model))
        .map_or_else(Vec::new, |m| m.supported_devices.clone())
        .into_iter()
        .filter(|d| d != "none")
        .collect();
    take(
        page,
        Input::DevicesLoaded {
            stage,
            source: source.clone(),
            model: model.clone(),
            result: Ok((devices, Some(device.into()))),
        },
    );
    // Its language, English by default.
    let key = ModelKey::new(stage, source, model);
    language::update(
        &mut page.languages,
        Some(&mut page.focus),
        language::Input::Focus(key.clone()),
    );
    language::update(
        &mut page.languages,
        Some(&mut page.focus),
        language::Input::Offered(
            key.clone(),
            vec![
                "auto".into(),
                "en".into(),
                "es".into(),
                "fr".into(),
                "de".into(),
            ],
        ),
    );
    language::update(
        &mut page.languages,
        Some(&mut page.focus),
        language::Input::Loaded(
            key,
            LanguageResolution {
                effective: Some("en".into()),
                source: "default".into(),
                primary: "en".into(),
            },
        ),
    );
}

pub fn pretend(
    page: &mut ModelsPage,
    scenes: &[&str],
    window: &mut Window,
    cx: &mut Context<ModelsPage>,
) {
    let has = |scene: &str| scenes.contains(&scene);
    take(
        page,
        Input::GpusLoaded(vec![GpuInfo {
            name: "NVIDIA GeForce RTX 3090".into(),
            vendor: "nvidia".into(),
            total_bytes: 24 * GIB,
            free_bytes: Some(19_220_840_653),
            used_bytes: Some(6_548_963_123),
            arch_target: Some("sm_86".into()),
        }]),
    );
    match page.feature {
        Feature::SpeechToText => speech_to_text(page, &has),
        Feature::TextToSpeech => text_to_speech(page, &has),
    }
    let configure = match page.feature {
        Feature::SpeechToText => has("configure"),
        Feature::TextToSpeech => has("configure-tts"),
    };
    if configure {
        let stage = page.stages()[0];
        if let Some(source) = page.models.view(stage).source.clone() {
            page.open_configure(source.clone(), window, cx);
            let backends = vec![whisper(), openai(), s1_mini(2 * GIB), kokoro()];
            page.sheet.contexts = serde_json::from_value::<ContextCatalog>(json!({
                "active": "coding",
                "contexts": [{ "id": "coding", "name": "Coding" }, { "id": "email", "name": "Email" }],
            }))
            .expect("made-up contexts");
            page.sheet.state.backend_contexts.insert(
                source.clone(),
                serde_json::from_value::<BackendContext>(json!({
                    "mode": "active", "context": { "id": "coding", "name": "Coding" },
                }))
                .expect("a made-up backend context"),
            );
            super_voice_app_core::pages::configure::update(
                &mut page.sheet.state,
                &backends,
                super_voice_app_core::pages::configure::Input::Open(source.clone()),
            );
            super_voice_app_core::pages::configure::update(
                &mut page.sheet.state,
                &backends,
                super_voice_app_core::pages::configure::Input::BackendsLoaded,
            );
            page.sheet.backends = backends;
            page.sheet.reading = None;
            page.sheet.loaded = true;
        }
    }
    if has("build") && page.feature == Feature::SpeechToText {
        take(
            page,
            Input::PickDevice(Stage::Transcription, "vulkan".into()),
        );
        if let Some(pending) = page.models.build_question().cloned() {
            page.ask_build(
                pending.stage,
                &pending.source,
                &pending.framework,
                window,
                cx,
            );
        }
    }
    cx.notify();
}

fn speech_to_text(page: &mut ModelsPage, has: &dyn Fn(&str) -> bool) {
    if has("openai") {
        fill(
            page,
            Stage::Transcription,
            vec![whisper(), openai()],
            view(OPENAI, "OpenAI", "gpt-4o-transcribe", false),
            "none",
        );
    } else {
        fill(
            page,
            Stage::Transcription,
            vec![whisper(), openai()],
            view(WHISPER, "Whisper", "large-v3-turbo", true),
            "cuda",
        );
        take(
            page,
            Input::BackendDevicesLoaded {
                stage: Stage::Transcription,
                source: WHISPER.into(),
                result: Ok(vec!["cuda".into(), "vulkan".into(), "cpu".into()]),
            },
        );
    }
    let post = Stage::PostProcessing;
    if has("empty") {
        take(
            page,
            Input::BackendsLoaded(post, Ok(vec![s1_mini(2 * GIB)])),
        );
        take(page, Input::ViewLoaded(post, Ok(StageState::default())));
        return;
    }
    let vram = if has("no-fit") {
        22 * GIB
    } else {
        2_684_354_560
    };
    fill(
        page,
        post,
        vec![s1_mini(vram)],
        view(S1_MINI, "S1-mini", "s1-mini-q4_k_m", false),
        "cuda",
    );
    if has("failed") {
        page.models.operations.set(
            post,
            ModelOperationState::Error {
                message: "CUDA error: out of memory while loading s1-mini-q4_k_m (needed 2.5 GiB)."
                    .into(),
            },
        );
    }
}

fn text_to_speech(page: &mut ModelsPage, has: &dyn Fn(&str) -> bool) {
    let stage = Stage::Synthesis;
    if has("empty") {
        take(
            page,
            Input::BackendsLoaded(stage, Ok(vec![kokoro(), qwen_tts()])),
        );
        take(page, Input::ViewLoaded(stage, Ok(StageState::default())));
        return;
    }
    if has("no-fit") {
        fill(
            page,
            stage,
            vec![kokoro(), qwen_tts()],
            view(
                "github.com/super-libre/super-tts-qwen",
                "Qwen TTS",
                "qwen3-tts-1.7b",
                false,
            ),
            "cuda",
        );
        return;
    }
    fill(
        page,
        stage,
        vec![kokoro(), qwen_tts()],
        view(KOKORO, "Kokoro", "kokoro-82m", false),
        "cuda",
    );
    page.voice = VoiceState {
        resolution: Some(VoiceResolution {
            effective: Some("af_heart".into()),
            model_override: None,
            default: Some("af_heart".into()),
            kinds: vec!["preset".into()],
        }),
        choices: ["af_heart", "af_bella", "am_michael", "bf_emma"]
            .into_iter()
            .map(|id| VoiceChoice {
                id: id.into(),
                label: id.into(),
                kind: "preset".into(),
            })
            .collect(),
        target: Some((KOKORO.into(), "kokoro-82m".into())),
    };
    if has("failed") {
        page.models.operations.set(
            stage,
            ModelOperationState::Error {
                message: "The download failed: huggingface.co answered 503.".into(),
            },
        );
        return;
    }
    let progress: DownloadProgress = serde_json::from_value(json!({
        "model_name": "kokoro-82m", "source": KOKORO,
        "stage": 1, "current_file": "kokoro-v1_0.safetensors", "file_index": 1,
        "total_files": 3, "bytes_downloaded": 723_517_440_u64,
        "total_bytes": 1_702_887_424_u64, "percentage": 42.0,
        "status": "downloading", "started_at": "",
    }))
    .expect("a download's progress");
    page.models.operations.start(
        stage,
        ModelOperationState::Provisioning {
            target_model: "kokoro-82m".into(),
            progress: Box::new(progress),
        },
    );
}
