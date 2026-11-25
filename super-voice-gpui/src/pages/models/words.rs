// SPDX-License-Identifier: GPL-3.0-only
//! What the Models pages say, from the core's state: a step's status, its
//! download or load, its model's memory, and the notices. Words only; the
//! cards draw them.

use gpui_kit::SharedString;
use super_voice_app_core::pages::model_language::{Button as LanguageButton, Resolved};
use super_voice_app_core::pages::models::{Memory, Notice};
use super_voice_app_core::pages::readiness::Feature;
use super_voice_app_core::rules::gpu::{accel_name, gib};
use super_voice_app_core::rules::status::ModelStatus;
use super_voice_app_core::state::Stage;
use super_voice_app_core::state::model_operations::{LoadCause, ModelOperationState};
use super_voice_app_core::state::registry::InstallStatus;
use super_voice_shared::models::protocol::{DownloadProgress, LoadProgress, load_progress};
use super_voice_shared::registry::events::InstallPhase;

use crate::{t, t_with};

/// The stage's name: "Transcription".
pub fn stage_name(stage: Stage) -> SharedString {
    t(stage_key(stage))
}

const fn stage_key(stage: Stage) -> &'static str {
    match stage {
        Stage::Transcription => "transcription",
        Stage::PostProcessing => "post-processing",
        Stage::Synthesis => "synthesis",
    }
}

/// Where a palette command acts: "Speech to text · Transcription".
pub fn step_detail(feature: Feature, stage: Stage) -> SharedString {
    t_with(
        "models-step-detail",
        &[("feature", &t(feature.key())), ("step", &stage_name(stage))],
    )
}

/// The word on a step's pill. A model on its way says whether it is
/// downloading or loading.
pub fn status(status: ModelStatus, operation: &ModelOperationState) -> SharedString {
    t(match status {
        ModelStatus::None => "readiness-no-backend",
        ModelStatus::Blocked => "status-needs-setup",
        ModelStatus::NotLoaded => match operation {
            ModelOperationState::Provisioning { .. } => "downloading",
            ModelOperationState::Loading { .. } => "loading",
            _ => "not-loaded",
        },
        ModelStatus::Ready => "ready",
        ModelStatus::Idle => "idle",
        ModelStatus::Failed => "failed",
        ModelStatus::NoVoice => "status-no-voice",
    })
}

/// What an empty step is for, and where to find a backend for it.
pub fn empty_step(stage: Stage, installed: bool) -> SharedString {
    t(match (stage, installed) {
        (Stage::Transcription, true) => "select-a-backend-to-start-transcribing",
        (Stage::PostProcessing, true) => "optional-clean-up-text-before-it-is",
        (Stage::Synthesis, true) => "select-a-backend-to-start-speaking",
        (Stage::Transcription, false) => "no-installed-backend-provides-a-transcription-model",
        (Stage::PostProcessing, false) => "no-installed-backend-provides-a-post-processor",
        (Stage::Synthesis, false) => "no-installed-backend-provides-text-to-speech",
    })
}

/// Bytes as GiB, in words: "3.1 GiB".
pub fn gibibytes(bytes: u64) -> SharedString {
    t_with("memory-size", &[("gib", &gib(bytes))])
}

/// A model's memory, in words: "Needs about 3.1 GiB of memory. It fits:
/// 17.9 GiB free."
pub fn memory(memory: Memory) -> SharedString {
    let needs = |bytes| t_with("model-needs-memory", &[("size", &gibibytes(bytes))]);
    match memory {
        Memory::Uses(bytes) => t_with("model-uses-memory", &[("size", &gibibytes(bytes))]),
        Memory::Needs(bytes) => needs(bytes),
        Memory::Fits { needed, free } => format!(
            "{} {}",
            needs(needed),
            t_with("model-fits", &[("free", &gibibytes(free))])
        )
        .into(),
        Memory::MayNotFit { needed, free } => format!(
            "{} {}",
            needs(needed),
            t_with("model-may-not-fit", &[("free", &gibibytes(free))])
        )
        .into(),
    }
}

/// A step's download, load, or why it failed, as its card shows it.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// Files being verified or downloaded, with Cancel.
    Download {
        line: SharedString,
        /// "690.0 / 1624.0 MB"; empty when the size isn't known.
        bytes: SharedString,
        /// 0 to 100.
        percent: f32,
    },
    /// The backend loading the model: what, its step, and how far when it
    /// can say (0 to 100).
    Load {
        title: SharedString,
        step: SharedString,
        percent: Option<f32>,
    },
    /// A build the user agreed to, downloading or installing. No Cancel: an
    /// install runs to its end.
    Build {
        line: SharedString,
        bytes: SharedString,
        /// `None` while its size isn't known.
        percent: Option<f32>,
    },
    /// The last download, load or move failed: the daemon's words.
    Failed(SharedString),
}

/// What `operation` shows on its card, or `None` when nothing is under way.
pub fn progress(operation: &ModelOperationState) -> Option<Progress> {
    match operation {
        ModelOperationState::Ready => None,
        ModelOperationState::Provisioning {
            target_model,
            progress,
        } => Some(download(target_model, progress)),
        ModelOperationState::Loading {
            target_model,
            load: Some(load),
            ..
        } => Some(loading(target_model, load)),
        ModelOperationState::Loading {
            target_model,
            cause,
            load: None,
        } => Some(Progress::Load {
            title: t_with("loading-model", &[("target_model", target_model)]),
            step: load_cause(cause),
            percent: None,
        }),
        ModelOperationState::Error { message } if message.is_empty() => {
            Some(Progress::Failed(t("model-switch-failed")))
        }
        ModelOperationState::Error { message } => Some(Progress::Failed(message.clone().into())),
    }
}

/// What a load says while the backend says nothing of its own.
fn load_cause(cause: &LoadCause) -> SharedString {
    match cause {
        LoadCause::Switch => t("progress-starting-switch"),
        LoadCause::Reload => t("progress-reloading-model"),
        LoadCause::Move { device } => {
            t_with("progress-loading-on", &[("device", &accel_name(device))])
        }
        LoadCause::Reported => t("progress-loading-model"),
    }
}

/// Megabytes, to one decimal.
// reason: display-only; the imprecision is cosmetic
#[allow(clippy::cast_precision_loss)]
fn mb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0))
}

fn download(model: &str, progress: &DownloadProgress) -> Progress {
    // `verifying` is checking files already on disk; everything else is
    // bytes coming off the network.
    let verb = t(if progress.status == "verifying" {
        "verifying"
    } else {
        "downloading"
    });
    let line = t_with(
        "provision-model",
        &[
            ("verb", &verb),
            ("model", model),
            ("file", &(progress.file_index + 1).to_string()),
            ("files", &progress.total_files.to_string()),
        ],
    );
    let bytes = if progress.total_bytes > 0 {
        t_with(
            "download-size",
            &[
                ("mb", &mb(progress.bytes_downloaded)),
                ("total", &mb(progress.total_bytes)),
            ],
        )
    } else {
        SharedString::default()
    };
    Progress::Download {
        line,
        bytes,
        percent: progress.percentage.clamp(0.0, 100.0),
    }
}

fn loading(model: &str, load: &LoadProgress) -> Progress {
    // "Initial setup" when the backend says this load pays a one-time cost,
    // so the user knows the wait won't repeat.
    let title = if load.phase.as_deref() == Some(load_progress::phase::INITIAL_SETUP) {
        t("initial-setup")
    } else {
        t_with("loading-model", &[("target_model", model)])
    };
    let step = t(match load.step.as_deref() {
        Some(load_progress::step::LOADING_WEIGHTS) => "loading-weights",
        Some(load_progress::step::BUILDING_KERNELS) => "building-kernels",
        Some(load_progress::step::WARMING_UP) => "warming-up",
        _ => "loading",
    });
    Progress::Load {
        title,
        step,
        percent: load.progress.map(|f| f.clamp(0.0, 1.0) * 100.0),
    }
}

/// A build for `framework` the user agreed to, as far as its install has come.
// reason: display-only; the imprecision is cosmetic
#[allow(clippy::cast_precision_loss)]
pub fn build(framework: &str, install: Option<&InstallStatus>) -> Progress {
    let name = accel_name(framework);
    let (phase, done, total) = install.map_or((InstallPhase::Resolving, 0, None), |s| {
        (s.phase, s.bytes_done, s.bytes_total.filter(|t| *t > 0))
    });
    let downloading = matches!(phase, InstallPhase::Resolving | InstallPhase::Downloading);
    match (downloading, total) {
        (true, Some(total)) => Progress::Build {
            line: t_with("downloading-the-build", &[("framework", &name)]),
            bytes: t_with("download-size", &[("mb", &mb(done)), ("total", &mb(total))]),
            percent: Some((done as f32 / total as f32 * 100.0).clamp(0.0, 100.0)),
        },
        (true, None) => Progress::Build {
            line: t_with("downloading-the-build", &[("framework", &name)]),
            bytes: SharedString::default(),
            percent: None,
        },
        (false, _) => Progress::Build {
            line: t_with("installing-the-build", &[("framework", &name)]),
            bytes: SharedString::default(),
            percent: Some(100.0),
        },
    }
}

/// A percent, to the whole: "42%".
// reason: display-only; the value is 0 to 100
#[allow(clippy::cast_possible_truncation)]
pub fn percent(value: f32) -> SharedString {
    t_with(
        "percent-value",
        &[("value", &format!("{}", value.round() as i32))],
    )
}

/// A model's language, as its picker's trigger shows it.
pub fn language(button: &LanguageButton) -> SharedString {
    use super_voice_app_core::languages::friendly_name;
    match button {
        LanguageButton::Unread => t("language"),
        LanguageButton::Fixed(tag) | LanguageButton::Pickable(Resolved::Override(tag)) => {
            friendly_name(tag).into()
        }
        LanguageButton::Pickable(Resolved::Global(tag)) => {
            t_with("language-global", &[("value0", &friendly_name(tag))])
        }
        LanguageButton::Pickable(Resolved::Default(tag)) => {
            t_with("language-default", &[("value0", &friendly_name(tag))])
        }
    }
}

/// What the cards tell the user.
pub fn notice(notice: Notice) -> SharedString {
    match notice {
        Notice::CouldNotRead(stage, error) => t_with(
            "stage-could-not-read",
            &[("stage", &stage_name(stage)), ("error", &error)],
        ),
        Notice::CouldNotChange(stage, error) => t_with(
            "stage-could-not-change",
            &[("stage", &stage_name(stage)), ("error", &error)],
        ),
        Notice::DeviceSwitchFailed(_, error) => {
            t_with("device-switch-failed", &[("error", &error)])
        }
        Notice::BuildFailed {
            framework, error, ..
        } => t_with(
            "build-download-failed",
            &[("framework", &accel_name(&framework)), ("error", &error)],
        ),
        Notice::Stalled(_) => t("model-load-stalled"),
    }
}

/// The stage a notice is about.
pub const fn notice_stage(notice: &Notice) -> Stage {
    match notice {
        Notice::CouldNotRead(stage, _)
        | Notice::CouldNotChange(stage, _)
        | Notice::DeviceSwitchFailed(stage, _)
        | Notice::BuildFailed { stage, .. }
        | Notice::Stalled(stage) => *stage,
    }
}

/// Every key the Models pages show, for the test that each is in the
/// catalog.
#[cfg(test)]
pub const KEYS: &[&str] = &[
    "models-stt-description",
    "models-tts-description",
    "microphone",
    "your-voice",
    "typed",
    "typed-with-the-write-method",
    "text",
    "text-from-your-app-or-assistant",
    "speakers",
    "read-aloud",
    "transcription",
    "post-processing",
    "synthesis",
    "configure-backend",
    "select-a-backend-menu",
    "open-library",
    "show-in-library",
    "reload-this-model",
    "deselect-this-backend",
    "step-more-actions",
    "readiness-no-backend",
    "status-needs-setup",
    "downloading",
    "verifying",
    "loading",
    "not-loaded",
    "ready",
    "idle",
    "failed",
    "status-no-voice",
    "select-a-backend-to-start-transcribing",
    "optional-clean-up-text-before-it-is",
    "select-a-backend-to-start-speaking",
    "no-installed-backend-provides-a-transcription-model",
    "no-installed-backend-provides-a-post-processor",
    "no-installed-backend-provides-text-to-speech",
    "memory-size",
    "model-needs-memory",
    "model-uses-memory",
    "model-fits",
    "model-may-not-fit",
    "model-idle-note",
    "model-device-unavailable",
    "running-on-framework",
    "required-setting",
    "loading-model",
    "model-switch-failed",
    "progress-starting-switch",
    "progress-reloading-model",
    "progress-loading-on",
    "progress-loading-model",
    "provision-model",
    "download-size",
    "initial-setup",
    "loading-weights",
    "building-kernels",
    "warming-up",
    "downloading-the-build",
    "installing-the-build",
    "percent-value",
    "model-loads-after-download",
    "download-the-build-question",
    "backend-needs-its-build",
    "build-moves-the-model",
    "download",
    "cancel",
    "model",
    "runs-on",
    "language",
    "voice",
    "select-model",
    "select",
    "load-model",
    "unload",
    "language-global",
    "language-default",
    "follow-global",
    "voice-default",
    "voice-cloned",
    "no-cloned-voice-chosen",
    "no-built-in-voices-pick-a-cloned-voice",
    "no-built-in-voices-record-one",
    "stage-could-not-read",
    "stage-could-not-change",
    "device-switch-failed",
    "build-download-failed",
    "model-load-stalled",
    "language-could-not-save",
    "voice-could-not-save",
    "palette-load-model",
    "palette-unload-model",
    "palette-reload-model",
    "unload-all-models",
    "models-step-detail",
    "backend-configuration",
    "loading-progress",
    "this-backend-has-nothing-to-configure",
    "dictation-context",
    "configure-required",
    "configure-options",
    "follow-active-context",
    "follow-the-active-context-none-right-now",
    "no-context",
    "sending-context",
    "nothing-is-sent-to-this-backend-right",
    "configured",
    "remove",
    "save",
    "reset",
    "done",
    "enter-api-key",
    "setting-default",
    "setting-hint-default",
    "backend-setting-could-not-save",
    "backend-context-change-failed",
    "backends-could-not-load",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_word_is_in_the_catalog() {
        for key in KEYS {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }

    /// Checking files on disk is never called a download.
    #[test]
    fn verifying_says_so() {
        let progress: DownloadProgress = serde_json::from_value(serde_json::json!({
            "model_name": "m", "source": "s", "stage": 1, "current_file": "f",
            "file_index": 0, "total_files": 2, "bytes_downloaded": 0,
            "total_bytes": 0, "percentage": 10.0, "status": "verifying", "started_at": "",
        }))
        .expect("a progress");
        let Progress::Download { line, bytes, .. } = download("m", &progress) else {
            panic!("a download");
        };
        assert!(line.contains(t("verifying").as_ref()), "{line}");
        assert!(bytes.is_empty());
    }

    #[test]
    fn a_failure_without_words_says_the_switch_failed() {
        let failed = progress(&ModelOperationState::Error {
            message: String::new(),
        });
        assert_eq!(failed, Some(Progress::Failed(t("model-switch-failed"))));
    }
}
