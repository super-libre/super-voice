// SPDX-License-Identifier: GPL-3.0-only
//! The Library's made-up state for `just app-shot`: Whisper, S1-mini, Kokoro
//! and `OpenAI` installed (Whisper with an update ready, `OpenAI` without its
//! key); Voxtral installing from the catalog, beside Qwen ASR, Qwen TTS and
//! Deepgram.
//!
//! Scenes: `library-installing` picks Voxtral, `library-update` Whisper,
//! `library-add` opens "Install manually" with a repository checked,
//! `library-empty` searches for what isn't there, `library-uninstall` asks
//! before uninstalling Whisper.

use gpui_kit::{Context, Window};
use serde_json::json;
use super_voice_app_core::daemon::backends::BackendInfo;
use super_voice_app_core::pages::add_backend as add;
use super_voice_app_core::pages::library::Input;
use super_voice_app_core::state::registry::RefreshOutcome;
use super_voice_shared::registry::RegistryBackend;
use super_voice_shared::registry::events::{InstallPhase, RegistryEvent};

use super::{LibraryPage, Msg};
use crate::effects::CorePage as _;
use crate::ui::width::{self, Inspector};

const WHISPER: &str = "github.com/super-libre/super-stt-whisper";
const S1_MINI: &str = "github.com/super-libre/super-stt-s1-mini";
const KOKORO: &str = "github.com/super-libre/super-tts-kokoro";
const OPENAI: &str = "github.com/super-libre/super-stt-openai";
const VOXTRAL: &str = "github.com/super-libre/super-stt-voxtral";
const QWEN_ASR: &str = "github.com/super-libre/super-stt-qwen-asr";
const QWEN_TTS: &str = "github.com/super-libre/super-tts-qwen";
const DEEPGRAM: &str = "github.com/super-libre/super-stt-deepgram";

/// An installed backend.
fn installed(source: &str, name: &str, description: &str, models: &[(&str, &str)]) -> BackendInfo {
    let online = source == OPENAI;
    serde_json::from_value(json!({
        "source": source,
        "name": name,
        "description": description,
        "version": match name { "Whisper" => "0.2.1", "S1-mini" => "0.3.0", "Kokoro" => "1.1.0", _ => "0.5.0" },
        "kind": if online { "wasm" } else { "subprocess" },
        "allowed_hosts": if online { vec!["api.openai.com"] } else { vec!["huggingface.co"] },
        "installed_accel": if online { vec![] } else { vec!["cuda", "cpu"] },
        "models": models.iter().map(|(name, role)| json!({
            "name": name,
            "role": role,
            "supported_devices": if online { vec!["none"] } else { vec!["cuda", "cpu"] },
            "accepts_context": online || *role == "post_processor",
        })).collect::<Vec<_>>(),
        "secrets": if online {
            json!([{ "name": "api_key", "label": "API key", "required": true }])
        } else {
            json!([])
        },
        "options": [],
        "supports": if online {
            vec!["online", "dictation_context"]
        } else if models.first().is_some_and(|(_, role)| *role == "post_processor") {
            vec!["gpu", "cpu", "dictation_context"]
        } else {
            vec!["gpu", "cpu"]
        },
    }))
    .expect("an installed backend")
}

/// A catalog entry.
fn entry(
    source: &str,
    name: &str,
    version: &str,
    description: &str,
    models: &[(&str, &str)],
) -> RegistryBackend {
    let online = source == OPENAI || source == DEEPGRAM;
    serde_json::from_value(json!({
        "id": name.to_lowercase(),
        "source": source,
        "version": version,
        "name": name,
        "description": description,
        "license": if online { "MIT" } else { "Apache-2.0" },
        "kind": if online { "wasm" } else { "subprocess" },
        "contract": "v4",
        "online": online,
        "supports_gpu": !online,
        "supports_cpu": !online,
        "supports": if online { vec!["online"] } else { vec!["gpu", "cpu"] },
        "allowed_hosts": match source {
            OPENAI => vec!["api.openai.com"],
            DEEPGRAM => vec!["api.deepgram.com"],
            _ => vec!["huggingface.co"],
        },
        "models": models.iter().map(|(name, role)| json!({
            "name": name,
            "role": role,
            "supported_devices": if online { vec!["none"] } else { vec!["cuda", "cpu"] },
        })).collect::<Vec<_>>(),
        "secrets": if online {
            json!([{ "name": "api_key", "label": "API key", "required": true }])
        } else {
            json!([])
        },
        "options": [],
        "compatibility": {
            "compatible": true,
            "selected_asset": if online {
                json!({ "target": "wasm32-wasip2", "accel": "cpu" })
            } else {
                json!({ "target": "x86_64-linux", "accel": ["cuda", "cpu"] })
            },
        },
        "builds": if online { json!([]) } else { json!([
            { "accel": "cuda", "size": 61_865_984 },
            { "accel": "vulkan", "size": 39_845_888 },
            { "accel": "cpu", "size": 20_971_520 },
        ]) },
        "update_available": false,
    }))
    .expect("a catalog entry")
}

impl LibraryPage {
    pub(super) fn pretend_scenes(
        &mut self,
        scenes: &[&str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.registry.backends = catalog();
        self.registry.generated_at = Some("2026-11-25T09:14:00Z".into());
        self.registry.last_refresh = Some(RefreshOutcome::Ok);
        self.library.backends = backends();
        self.library.missing_keys.insert(OPENAI.into());
        self.pretend_rest(scenes, window, cx);
    }
}

/// The backends installed.
fn backends() -> Vec<BackendInfo> {
    vec![
        installed(
            WHISPER,
            "Whisper",
            "Local Whisper speech to text.",
            &[
                ("large-v3-turbo", "transcription"),
                ("large-v3", "transcription"),
                ("small.en", "transcription"),
            ],
        ),
        installed(
            S1_MINI,
            "S1-mini",
            "Cleans up text before it is typed.",
            &[("s1-mini-q4_k_m", "post_processor")],
        ),
        installed(
            KOKORO,
            "Kokoro",
            "Small local text to speech with built-in voices.",
            &[("kokoro-82m", "synthesis")],
        ),
        installed(
            OPENAI,
            "OpenAI",
            "OpenAI's hosted speech to text.",
            &[
                ("gpt-4o-transcribe", "transcription"),
                ("whisper-1", "transcription"),
            ],
        ),
    ]
}

/// The catalog.
fn catalog() -> Vec<RegistryBackend> {
    let mut whisper = entry(
        WHISPER,
        "Whisper",
        "0.2.2",
        "Local Whisper speech to text.",
        &[("large-v3-turbo", "transcription")],
    );
    whisper.installed_version = Some("0.2.1".into());
    whisper.update_available = true;
    vec![
        whisper,
        entry(
            VOXTRAL,
            "Voxtral",
            "0.4.0",
            "Mistral's Voxtral speech to text, local.",
            &[("voxtral-mini-3b", "transcription")],
        ),
        entry(
            QWEN_ASR,
            "Qwen ASR",
            "0.3.1",
            "Qwen's speech recognition, local, in many languages.",
            &[
                ("qwen3-asr-0.6b", "transcription"),
                ("qwen3-asr-1.7b", "transcription"),
            ],
        ),
        entry(
            QWEN_TTS,
            "Qwen TTS",
            "0.2.0",
            "Qwen's text to speech, local, with voices you clone.",
            &[("qwen3-tts-0.6b", "synthesis")],
        ),
        entry(
            DEEPGRAM,
            "Deepgram",
            "0.1.4",
            "Deepgram's hosted speech to text.",
            &[("nova-3", "transcription")],
        ),
    ]
}

impl LibraryPage {
    fn pretend_rest(&mut self, scenes: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        // Voxtral is 38% through its download.
        for msg in [
            Input::InstallAccepted {
                source: VOXTRAL.into(),
                install_id: "ins_1".into(),
            },
            Input::Event(RegistryEvent::Progress {
                install_id: "ins_1".into(),
                source: VOXTRAL.into(),
                phase: InstallPhase::Downloading,
                bytes_done: Some(23_508_828),
                bytes_total: Some(61_865_984),
            }),
        ] {
            super::page::update(&mut self.registry, &mut self.library, msg);
        }
        let layout = width::inspector(width::content(window));
        let pick = if scenes.contains(&"library-installing") {
            Some(VOXTRAL)
        } else if scenes.contains(&"library-update") || scenes.contains(&"library-uninstall") {
            Some(WHISPER)
        } else if layout == Inspector::Beside && !scenes.contains(&"library-empty") {
            Some(QWEN_ASR)
        } else {
            None
        };
        if let Some(source) = pick {
            self.pick(source, window, cx);
        }
        if scenes.contains(&"library-empty") {
            self.search.update(cx, |state, cx| {
                state.set_value("parakeet", window, cx);
            });
            self.send(Msg::Library(Input::Search("parakeet".into())), window, cx);
        }
        if scenes.contains(&"library-uninstall") {
            super::page::update(
                &mut self.registry,
                &mut self.library,
                Input::AskUninstall(WHISPER.into()),
            );
            self.library.pending_in_use = true;
        }
        if scenes.contains(&"library-add") {
            let url = "https://github.com/super-libre/super-stt-qwen-asr";
            add::update(&mut self.registry, add::Input::Url(url.into()));
            let mut preview = entry(
                QWEN_ASR,
                "Qwen ASR",
                "0.3.1",
                "Qwen's speech recognition, local, in many languages.",
                &[
                    ("qwen3-asr-0.6b", "transcription"),
                    ("qwen3-asr-1.7b", "transcription"),
                ],
            );
            preview.builds.clear();
            add::update(
                &mut self.registry,
                add::Input::Previewed(Ok(Box::new(preview))),
            );
            self.open_add(window, cx);
        }
        self.sync(window, cx);
        cx.notify();
    }
}
