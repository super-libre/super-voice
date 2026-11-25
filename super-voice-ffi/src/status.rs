// SPDX-License-Identifier: GPL-3.0-only
//! The window header's readouts: the GPU, and how ready each feature's model
//! is; and the Models page's pipeline, read with them. A store that reads them
//! from the daemon every few seconds and tells the app when they change.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super_voice_app_core::daemon::client;
use super_voice_app_core::rules::pipeline::{self, Step, StepModel};
use super_voice_app_core::rules::{gpu, status};
use super_voice_app_core::state::Stage;

use crate::RUNTIME;

/// How often the readouts are read again.
const EVERY: Duration = Duration::from_secs(2);

/// What the app is told. Called from the store's own threads.
#[uniffi::export(with_foreign)]
pub trait StatusObserver: Send + Sync {
    /// The readouts changed; draw these.
    fn changed(&self, header: HeaderStatus);
}

/// How ready a feature's model is: the colour of its dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Readiness {
    /// No model is chosen. Grey.
    None,
    /// The backend isn't set up. Red.
    Blocked,
    /// A model is chosen but not up. Yellow.
    NotLoaded,
    /// The model is loaded. Green.
    Ready,
    /// The model is asleep, and its next use loads it. Green.
    Idle,
    /// The model failed to come up. Red.
    Failed,
    /// The speech model has no voice to speak in. Red.
    NoVoice,
}

impl From<status::ModelStatus> for Readiness {
    fn from(status: status::ModelStatus) -> Self {
        match status {
            status::ModelStatus::None => Self::None,
            status::ModelStatus::Blocked => Self::Blocked,
            status::ModelStatus::NotLoaded => Self::NotLoaded,
            status::ModelStatus::Ready => Self::Ready,
            status::ModelStatus::Idle => Self::Idle,
            status::ModelStatus::Failed => Self::Failed,
            status::ModelStatus::NoVoice => Self::NoVoice,
        }
    }
}

/// How full a GPU's memory is: the colour of its meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MeterLevel {
    /// Green.
    Ok,
    /// Three quarters or more. Yellow.
    High,
    /// Nine tenths or more. Red.
    Full,
}

impl From<gpu::MeterLevel> for MeterLevel {
    fn from(level: gpu::MeterLevel) -> Self {
        match level {
            gpu::MeterLevel::Ok => Self::Ok,
            gpu::MeterLevel::High => Self::High,
            gpu::MeterLevel::Full => Self::Full,
        }
    }
}

/// One GPU, as the header shows it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GpuReadout {
    /// The name without the vendor's prefix.
    pub name: String,
    /// The full name, for the tooltip.
    pub full_name: String,
    pub total_bytes: u64,
    /// `None` when the daemon can't tell how much is in use.
    pub used_bytes: Option<u64>,
    /// How much of the meter is filled, 0 to 1, when the use is known.
    pub fraction: Option<f32>,
    /// The meter's colour, when the use is known.
    pub level: Option<MeterLevel>,
}

/// A stage of a feature's pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PipelineStage {
    Transcription,
    PostProcessing,
    Synthesis,
}

impl From<PipelineStage> for Stage {
    fn from(stage: PipelineStage) -> Self {
        match stage {
            PipelineStage::Transcription => Self::Transcription,
            PipelineStage::PostProcessing => Self::PostProcessing,
            PipelineStage::Synthesis => Self::Synthesis,
        }
    }
}

/// How a stage's model stands.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum StepModelPage {
    /// It runs, on `device` (a framework's name, such as CUDA) when the
    /// daemon says which.
    Running {
        name: String,
        device: Option<String>,
    },
    /// It is out of memory until its next use, which loads it.
    Idle { name: String },
    /// Nothing is up.
    NotLoaded,
}

/// A stage's step on the Models page.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PipelineStep {
    pub stage: PipelineStage,
    /// The colour of its dot.
    pub readiness: Readiness,
    /// The backend that fills it; `None` when none does.
    pub backend: Option<String>,
    /// How its model stands; `NotLoaded` when no backend fills it.
    pub model: StepModelPage,
}

impl PipelineStep {
    fn read(stage: PipelineStage, block: &client::StageState) -> Self {
        let (backend, model) = match pipeline::step(stage.into(), block) {
            Step::Empty => (None, StepModelPage::NotLoaded),
            Step::Filled { backend, model } => (
                Some(backend),
                match model {
                    StepModel::Running { name, device } => StepModelPage::Running {
                        name,
                        device: device.as_deref().map(gpu::accel_name),
                    },
                    StepModel::Idle { name } => StepModelPage::Idle { name },
                    StepModel::NotLoaded => StepModelPage::NotLoaded,
                },
            ),
        };
        Self {
            stage,
            readiness: status::stage_block_status(stage.into(), block).into(),
            backend,
            model,
        }
    }
}

/// Everything the header shows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct HeaderStatus {
    /// Whether the daemon answered. Until it does, the header shows nothing.
    pub connected: bool,
    /// The GPUs, primary first. Empty when the daemon reports none.
    pub gpus: Vec<GpuReadout>,
    pub speech_to_text: Readiness,
    pub text_to_speech: Readiness,
    /// The Models page's steps: transcription, post-processing, synthesis.
    /// Empty until the daemon answers.
    pub pipeline: Vec<PipelineStep>,
}

impl HeaderStatus {
    const DISCONNECTED: Self = Self {
        connected: false,
        gpus: Vec::new(),
        speech_to_text: Readiness::None,
        text_to_speech: Readiness::None,
        pipeline: Vec::new(),
    };
}

/// The header's store. Reads the daemon until it is dropped.
#[derive(uniffi::Object)]
pub struct StatusStore {
    last: Mutex<HeaderStatus>,
    observer: Arc<dyn StatusObserver>,
}

#[uniffi::export]
impl StatusStore {
    /// A store that tells `observer` what the header shows, now and whenever
    /// it changes.
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn StatusObserver>) -> Arc<Self> {
        crate::identify();
        let store = Arc::new(Self {
            last: Mutex::new(HeaderStatus::DISCONNECTED),
            observer,
        });
        let weak = Arc::downgrade(&store);
        RUNTIME.spawn(async move {
            loop {
                let header = read().await;
                let Some(store) = weak.upgrade() else {
                    return;
                };
                store.show(header);
                drop(store);
                tokio::time::sleep(EVERY).await;
            }
        });
        store
    }

    /// What the header shows now.
    pub fn header(&self) -> HeaderStatus {
        self.last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl StatusStore {
    /// Keep `header` and tell the app, when it changed.
    fn show(&self, header: HeaderStatus) {
        {
            let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
            if *last == header {
                return;
            }
            last.clone_from(&header);
        }
        self.observer.changed(header);
    }
}

/// Read the readouts from the daemon: each stage, as the settings app reads
/// it, and the GPUs.
async fn read() -> HeaderStatus {
    let (Ok(transcription), Ok(post_processing), Ok(synthesis)) = (
        client::get_stage_view(Stage::Transcription).await,
        client::get_stage_view(Stage::PostProcessing).await,
        client::get_stage_view(Stage::Synthesis).await,
    ) else {
        return HeaderStatus::DISCONNECTED;
    };
    // The GPUs are a nice-to-have: a daemon that can't say still has stages.
    let gpus = client::get_gpu_info().await.unwrap_or_default();
    HeaderStatus {
        connected: true,
        gpus: gpus.iter().map(readout).collect(),
        speech_to_text: status::stage_block_status(Stage::Transcription, &transcription).into(),
        text_to_speech: status::stage_block_status(Stage::Synthesis, &synthesis).into(),
        pipeline: vec![
            PipelineStep::read(PipelineStage::Transcription, &transcription),
            PipelineStep::read(PipelineStage::PostProcessing, &post_processing),
            PipelineStep::read(PipelineStage::Synthesis, &synthesis),
        ],
    }
}

fn readout(info: &super_voice_shared::models::protocol::GpuInfo) -> GpuReadout {
    GpuReadout {
        name: gpu::short_name(&info.name).to_string(),
        full_name: info.name.clone(),
        total_bytes: info.total_bytes,
        used_bytes: info.used_bytes,
        fraction: info
            .used_bytes
            .map(|used| gpu::meter_fraction(used, info.total_bytes)),
        level: info
            .used_bytes
            .map(|used| gpu::meter_level(used, info.total_bytes).into()),
    }
}
