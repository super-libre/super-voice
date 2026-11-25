// SPDX-License-Identifier: GPL-3.0-only
//! The Models page's stage cards for an app written in another language: a
//! store that runs the core's logic for the cards (`pages::models`) on the
//! shared runtime, takes the daemon's download and load reports as they come,
//! reads a stage again as the daemon's events say it changed, and tells the app
//! whenever a card changed, in the user's words.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use super_voice_app_core::events::Event;
use super_voice_app_core::pages::models::{
    self as page, Card, Effect, Input, ModelsState, Notice, STAGES, Shown,
};
use super_voice_app_core::rules::backends::{
    backend_is_online, backend_supports_cpu, backend_supports_gpu,
};
use super_voice_app_core::rules::gpu::accel_name;
use super_voice_app_core::rules::load::{Size, size};
use super_voice_app_core::state::Stage;
use super_voice_app_core::state::model_operations::ModelOperationState;
use super_voice_shared::models::protocol::{DownloadProgress, LoadProgress, load_progress};
use tokio::sync::broadcast::error::RecvError;

use crate::RUNTIME;
use crate::behavior::Choice;
use crate::status::PipelineStage;

/// How often a run of the daemon's events reads the stages again.
const SETTLE: Duration = Duration::from_millis(800);

/// What the app is told. Called from the store's own threads.
#[uniffi::export(with_foreign)]
pub trait ModelsObserver: Send + Sync {
    /// The cards changed; draw these.
    fn changed(&self, page: ModelsPage);
}

/// What the user did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ModelsAction {
    /// Read every stage: the page was opened.
    Reload,
    /// A backend was picked; `None` leaves the stage empty.
    PickBackend {
        stage: PipelineStage,
        source: Option<String>,
    },
    /// A model was picked: staged for Load.
    PickModel {
        stage: PipelineStage,
        model: String,
    },
    /// A device was picked: staged, or the running model moved onto it.
    PickDevice {
        stage: PipelineStage,
        device: String,
    },
    Load {
        stage: PipelineStage,
    },
    /// Bring the running model back up.
    ReloadModel {
        stage: PipelineStage,
    },
    Unload {
        stage: PipelineStage,
    },
    CancelDownload {
        stage: PipelineStage,
    },
}

/// A backend, as a stage's picker or card shows it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CardBackend {
    /// Its source, which is its id.
    pub source: String,
    pub name: String,
    pub version: String,
    /// One line about it; empty when it has none.
    pub description: String,
    /// Its repository's web address.
    pub repository: String,
    /// What it runs its models on.
    pub chips: BackendChips,
    /// How many of its models can fill the stage.
    pub models: u32,
}

/// What a backend runs its models on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct BackendChips {
    pub gpu: bool,
    pub cpu: bool,
    /// Its models run on someone else's servers.
    pub online: bool,
}

/// What a card shows of its model.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum CardModel {
    /// Nothing fills the stage.
    Empty,
    /// The model is up; `idle`: it is out of memory until its next use.
    Running { model: String, idle: bool },
    /// The model staged for Load, if any.
    Staging { model: Option<String> },
}

/// The lines a card adds under its rows, in the user's words.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CardNotes {
    /// How much memory the staged model needs.
    pub memory: Option<String>,
    /// The framework the running model fell back to.
    pub fallback: Option<String>,
    /// Why the staged model can't load here, or may not fit.
    pub warning: Option<String>,
    /// Each required setting not set yet: Configure first.
    pub unmet: Vec<String>,
}

/// The stage's download or load, in the user's words.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum CardProgress {
    None,
    /// Files being verified or downloaded, with Cancel.
    Download {
        line: String,
        /// 0 to 1.
        fraction: f32,
        /// "120.0 / 640.0 MB"; empty when the size isn't known.
        bytes: String,
    },
    /// The backend loading the model, with its step.
    Load {
        title: String,
        step: String,
        /// 0 to 1, when the backend can say.
        fraction: Option<f32>,
    },
    /// A change under way, with nothing more to say yet.
    Working {
        line: String,
    },
    Failed {
        message: String,
    },
}

/// One stage's card.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct StageCard {
    pub stage: PipelineStage,
    /// The backend filling it; `None` when none does.
    pub source: Option<String>,
    /// The backends that can fill it.
    pub backends: Vec<CardBackend>,
    /// The backend filling it, when it is one of those.
    pub backend: Option<CardBackend>,
    pub model: CardModel,
    /// The backend's models for the stage.
    pub models: Vec<String>,
    /// What the shown model can run on here, each a framework's id with its
    /// name; empty when it has no local device, or before the daemon says.
    pub devices: Vec<Choice>,
    /// The device picked for the shown model.
    pub device: Option<String>,
    pub notes: CardNotes,
    pub progress: CardProgress,
    /// The staged model and device are ready to load.
    pub can_load: bool,
    /// The card has a Reload button for its running model.
    pub reloadable: bool,
    /// A change is running; the card waits.
    pub busy: bool,
}

/// Everything the cards show.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ModelsPage {
    pub cards: Vec<StageCard>,
    /// What to tell the user, in the user's language.
    pub notice: Option<String>,
}

#[derive(Default)]
struct Held {
    state: ModelsState,
    notice: Option<String>,
}

/// The Models page's store.
#[derive(uniffi::Object)]
pub struct ModelsStore {
    held: Mutex<Held>,
    observer: Arc<dyn ModelsObserver>,
    /// The daemon said a stage changed since the stages were last read.
    stirred: AtomicBool,
}

#[uniffi::export]
impl ModelsStore {
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn ModelsObserver>) -> Arc<Self> {
        crate::identify();
        let store = Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
            stirred: AtomicBool::new(false),
        });
        follow(Arc::downgrade(&store));
        store
    }

    pub fn page(&self) -> ModelsPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: ModelsAction) {
        self.dispatch(match action {
            ModelsAction::Reload => Input::Reload,
            ModelsAction::PickBackend { stage, source } => Input::PickBackend(stage.into(), source),
            ModelsAction::PickModel { stage, model } => Input::PickModel(stage.into(), model),
            ModelsAction::PickDevice { stage, device } => Input::PickDevice(stage.into(), device),
            ModelsAction::Load { stage } => Input::Load(stage.into()),
            ModelsAction::ReloadModel { stage } => Input::ReloadModel(stage.into()),
            ModelsAction::Unload { stage } => Input::Unload(stage.into()),
            ModelsAction::CancelDownload { stage } => Input::CancelDownload(stage.into()),
        });
    }
}

impl ModelsStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn dispatch(self: Arc<Self>, input: Input) {
        let (page, effects) = {
            let mut held = self.lock();
            let outcome = page::update(&mut held.state, input);
            if outcome.clear_notice {
                held.notice = None;
            }
            if let Some(notice) = outcome.notice {
                held.notice = Some(notice_text(notice));
            }
            (snapshot(&held), outcome.effects)
        };
        self.observer.changed(page);
        for effect in effects {
            let store = Arc::clone(&self);
            RUNTIME.spawn(async move {
                let input = match effect {
                    Effect::ReadView(stage) => Some(page::run_view(stage).await),
                    Effect::ReadBackends(stage) => Some(page::run_backends(stage).await),
                    Effect::ReadSecrets(source) => Some(page::run_secrets(source).await),
                    Effect::ReadGpus => Some(page::run_gpus().await),
                    Effect::ReadDevices {
                        stage,
                        source,
                        model,
                    } => Some(page::run_devices(stage, source, model).await),
                    Effect::Poll(stage) => Some(page::run_poll(stage).await),
                    change => page::run_change(change).await,
                };
                if let Some(input) = input {
                    store.dispatch(input);
                }
            });
        }
    }
}

/// Take the daemon's download and load reports as they come, and read the
/// stages again as its other events say they changed, at most every
/// [`SETTLE`], and everything when the daemon is back, until `store` is
/// dropped.
fn follow(store: Weak<ModelsStore>) {
    let mut events = crate::events::listen();
    let listening = store.clone();
    RUNTIME.spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            };
            let Some(store) = listening.upgrade() else {
                return;
            };
            if let Some((stage, progress)) = super_voice_app_core::events::download_progress(&event)
            {
                store.dispatch(Input::Progress(stage, progress));
                continue;
            }
            match event {
                Event::Connected => store.dispatch(Input::Reload),
                Event::Status(_) | Event::Install(_) => {
                    store.stirred.store(true, Ordering::Relaxed);
                }
                _ => {}
            }
        }
    });
    RUNTIME.spawn(async move {
        loop {
            tokio::time::sleep(SETTLE).await;
            let Some(store) = store.upgrade() else {
                return;
            };
            if store.stirred.swap(false, Ordering::Relaxed) {
                for stage in STAGES {
                    Arc::clone(&store).dispatch(Input::Changed(stage));
                }
            }
        }
    });
}

fn snapshot(held: &Held) -> ModelsPage {
    let state = &held.state;
    ModelsPage {
        cards: STAGES
            .iter()
            .map(|&stage| stage_card(state, &page::card(state, stage)))
            .collect(),
        notice: held.notice.clone(),
    }
}

fn stage_card(cards: &ModelsState, card: &Card<'_>) -> StageCard {
    let stage = card.stage;
    let backends: Vec<CardBackend> = cards
        .catalog
        .backends(stage)
        .unwrap_or_default()
        .iter()
        .map(|b| card_backend(cards, stage, b))
        .collect();
    let model = match &card.shown {
        Shown::Empty => CardModel::Empty,
        Shown::Running { model, idle } => CardModel::Running {
            model: model.clone(),
            idle: *idle,
        },
        Shown::Staging { model } => CardModel::Staging {
            model: model.clone(),
        },
    };
    let shown_model = match &card.shown {
        Shown::Running { model, .. } => model.as_str(),
        Shown::Staging { model } => model.as_deref().unwrap_or_default(),
        Shown::Empty => "",
    };
    StageCard {
        stage: pipeline_stage(stage),
        source: card.source.clone(),
        backend: card.backend.map(|b| card_backend(cards, stage, b)),
        backends,
        model,
        models: card.models.clone(),
        devices: card
            .devices
            .unwrap_or_default()
            .iter()
            .map(String::as_str)
            .map(device)
            .collect(),
        device: card.device.map(ToString::to_string),
        notes: CardNotes {
            memory: card.memory.map(|bytes| {
                crate::text_with("model-needs-memory", args([("size", size_text(bytes))]))
            }),
            fallback: card.fallback.as_deref().map(|framework| {
                crate::text_with(
                    "running-on-framework",
                    args([("framework", accel_name(framework))]),
                )
            }),
            warning: warning(card, shown_model),
            unmet: card
                .unmet
                .iter()
                .map(|label| {
                    crate::text_with("required-setting", args([("label", (*label).to_string())]))
                })
                .collect(),
        },
        progress: progress(card.operation),
        can_load: card.can_load,
        // A reload is a model brought back up where it runs: the cards with
        // one in the settings app.
        reloadable: stage != Stage::Transcription
            && matches!(card.shown, Shown::Running { idle: false, .. }),
        busy: card.busy,
    }
}

fn card_backend(
    cards: &ModelsState,
    stage: Stage,
    backend: &super_voice_app_core::daemon::backends::BackendInfo,
) -> CardBackend {
    CardBackend {
        source: backend.source.clone(),
        name: backend.name.clone(),
        version: backend.version.clone(),
        description: backend.description.clone(),
        repository: format!("https://{}", backend.source),
        chips: BackendChips {
            gpu: backend_supports_gpu(backend),
            cpu: backend_supports_cpu(backend),
            online: backend_is_online(backend),
        },
        models: u32::try_from(cards.catalog.models(stage, &backend.source).len())
            .unwrap_or(u32::MAX),
    }
}

fn device(id: &str) -> Choice {
    Choice {
        id: id.to_string(),
        name: accel_name(id),
    }
}

/// Why the staged model can't load here, or may not fit on the GPU.
fn warning(card: &Card<'_>, model: &str) -> Option<String> {
    if card.no_device {
        return Some(crate::text_with(
            "model-device-unavailable",
            args([("model", model.to_string())]),
        ));
    }
    card.vram_shortfall.map(|(needed, available)| {
        crate::text_with(
            "model-memory-warning",
            args([("value0", gib(needed)), ("value1", gib(available))]),
        )
    })
}

/// The stage's download or load, in words.
// reason: display-only; the imprecision is cosmetic
#[allow(clippy::cast_precision_loss)]
fn progress(operation: &ModelOperationState) -> CardProgress {
    match operation {
        ModelOperationState::Ready => CardProgress::None,
        ModelOperationState::Provisioning {
            target_model,
            progress,
        } => download(target_model, progress),
        ModelOperationState::Loading {
            target_model,
            load: Some(load),
            ..
        } => loading(target_model, load),
        ModelOperationState::Loading { target_model, .. } => CardProgress::Working {
            line: crate::text_with(
                "loading-model",
                args([("target_model", target_model.clone())]),
            ),
        },
        ModelOperationState::Error { message } if message.is_empty() => CardProgress::Failed {
            message: crate::text("model-switch-failed"),
        },
        ModelOperationState::Error { message } => CardProgress::Failed {
            message: message.clone(),
        },
    }
}

// reason: display-only; the imprecision is cosmetic
#[allow(clippy::cast_precision_loss)]
fn download(model: &str, progress: &DownloadProgress) -> CardProgress {
    let fraction = if (0.0..=100.0).contains(&progress.percentage) {
        progress.percentage / 100.0
    } else {
        0.0
    };
    // `verifying` is checking files already on disk; everything else is
    // bytes coming off the network.
    let verb = crate::text(if progress.status == "verifying" {
        "verifying"
    } else {
        "downloading"
    });
    let line = crate::text_with(
        "provision-model-progress",
        args([
            ("value0", verb),
            ("value1", model.to_string()),
            ("value2", (progress.file_index + 1).to_string()),
            ("value3", progress.total_files.to_string()),
            ("value4", format!("{:.1}", progress.percentage)),
        ]),
    );
    let bytes = if progress.total_bytes > 0 {
        let mb = |b: u64| format!("{:.1}", b as f64 / (1024.0 * 1024.0));
        crate::text_with(
            "download-size",
            args([
                ("mb", mb(progress.bytes_downloaded)),
                ("total", mb(progress.total_bytes)),
            ]),
        )
    } else {
        String::new()
    };
    CardProgress::Download {
        line,
        fraction,
        bytes,
    }
}

fn loading(model: &str, load: &LoadProgress) -> CardProgress {
    // "Initial setup" when the backend says this load pays a one-time cost,
    // so the user knows the wait won't repeat.
    let title = if load.phase.as_deref() == Some(load_progress::phase::INITIAL_SETUP) {
        crate::text("initial-setup")
    } else {
        crate::text_with("loading-model", args([("target_model", model.to_string())]))
    };
    let step = crate::text(match load.step.as_deref() {
        Some(load_progress::step::LOADING_WEIGHTS) => "loading-weights",
        Some(load_progress::step::BUILDING_KERNELS) => "building-kernels",
        Some(load_progress::step::WARMING_UP) => "warming-up",
        _ => "loading",
    });
    CardProgress::Load {
        title,
        step,
        fraction: load.progress.map(|f| f.clamp(0.0, 1.0)),
    }
}

/// A size in the user's words: "640 MB", "1.5 GB".
fn size_text(bytes: u64) -> String {
    match size(bytes) {
        Size::Mb(size) => crate::text_with("size-mb", args([("size", size)])),
        Size::Gb(size) => crate::text_with("size-gb", args([("size", size)])),
    }
}

/// Bytes as GiB, to one decimal.
// reason: display-only; the imprecision is cosmetic
#[allow(clippy::cast_precision_loss)]
fn gib(bytes: u64) -> String {
    crate::text_with(
        "memory-size",
        args([(
            "gib",
            format!("{:.1}", bytes as f64 / (1024.0 * 1024.0 * 1024.0)),
        )]),
    )
}

fn args<const N: usize>(pairs: [(&str, String); N]) -> HashMap<String, String> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn pipeline_stage(stage: Stage) -> PipelineStage {
    match stage {
        Stage::Transcription => PipelineStage::Transcription,
        Stage::PostProcessing => PipelineStage::PostProcessing,
        Stage::Synthesis => PipelineStage::Synthesis,
    }
}

/// A stage's name, under the settings app's key.
fn stage_name(stage: Stage) -> String {
    crate::text(match stage {
        Stage::Transcription => "transcription",
        Stage::PostProcessing => "post-processing",
        Stage::Synthesis => "synthesis",
    })
}

/// What the cards tell the user, in the app's words.
fn notice_text(notice: Notice) -> String {
    let (key, stage, error) = match notice {
        Notice::CouldNotRead(stage, e) => ("stage-could-not-read", stage, e),
        Notice::CouldNotChange(stage, e) => ("stage-could-not-change", stage, e),
    };
    crate::text_with(key, args([("stage", stage_name(stage)), ("error", error)]))
}
