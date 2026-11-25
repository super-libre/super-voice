// SPDX-License-Identifier: GPL-3.0-only
//! The window header's readouts: the GPU, and how ready each feature's model
//! is. A store that reads them from the daemon every few seconds and tells the
//! app when they change.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super_voice_app_core::daemon::client;
use super_voice_app_core::rules::{gpu, status};

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

/// Everything the header shows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct HeaderStatus {
    /// Whether the daemon answered. Until it does, the header shows nothing.
    pub connected: bool,
    /// The GPUs, primary first. Empty when the daemon reports none.
    pub gpus: Vec<GpuReadout>,
    pub speech_to_text: Readiness,
    pub text_to_speech: Readiness,
}

impl HeaderStatus {
    const DISCONNECTED: Self = Self {
        connected: false,
        gpus: Vec::new(),
        speech_to_text: Readiness::None,
        text_to_speech: Readiness::None,
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

/// Read the readouts from the daemon: each feature's first stage, as the
/// settings app reads it, and the GPUs.
async fn read() -> HeaderStatus {
    use super_voice_app_core::state::Stage;
    let (Ok(transcription), Ok(synthesis)) = (
        client::get_stage_view(Stage::Transcription).await,
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
