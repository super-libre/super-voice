// SPDX-License-Identifier: GPL-3.0-only
//! The Models page's stage cards for an app written in another language: a
//! store that holds each stage as the daemon reports it, runs the core's logic
//! for the cards (`pages::models`) on the shared runtime, reads a stage again
//! as the daemon's events say it changed, and tells the app whenever a card
//! changed.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use super_voice_app_core::events::Event;
use super_voice_app_core::pages::models::{
    self as page, Effect, Input, ModelsState, Notice, STAGES,
};
use super_voice_app_core::rules::gpu::accel_name;
use super_voice_app_core::state::Stage;
use tokio::sync::broadcast::error::RecvError;

use crate::RUNTIME;
use crate::behavior::Choice;
use crate::status::PipelineStage;

/// How often a run of the daemon's events reads the stages again: a load's
/// progress arrives many times a second.
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
    PickModel {
        stage: PipelineStage,
        model: String,
    },
    PickDevice {
        stage: PipelineStage,
        device: String,
    },
    Load {
        stage: PipelineStage,
    },
    Unload {
        stage: PipelineStage,
    },
}

/// One stage's card.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StageCard {
    pub stage: PipelineStage,
    /// The backend filling it; `None` when none does.
    pub source: Option<String>,
    /// The backends that can fill it. Each id is its source.
    pub backends: Vec<Choice>,
    /// The model picked; `None` when none is.
    pub model: Option<String>,
    /// The picked backend's models for the stage.
    pub models: Vec<String>,
    /// The model is up.
    pub loaded: bool,
    /// The device the model runs on, and the ones it could, each a
    /// framework's id with its name; empty when the model has no local device.
    pub device: Option<String>,
    pub devices: Vec<Choice>,
    /// A change is running; the card waits.
    pub busy: bool,
}

/// Everything the cards show.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
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
            ModelsAction::Unload { stage } => Input::Unload(stage.into()),
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
                    Effect::ReadDevice(stage, model) => Some(page::run_device(stage, model).await),
                    change => page::run_change(change).await,
                };
                if let Some(input) = input {
                    store.dispatch(input);
                }
            });
        }
    }
}

/// Read the stages again as the daemon's events say they changed, at most
/// every [`SETTLE`], and everything when the daemon is back, until `store` is
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
            match event {
                Event::Connected => store.dispatch(Input::Reload),
                Event::Status(_) => store.stirred.store(true, Ordering::Relaxed),
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
    let cards = STAGES
        .iter()
        .map(|&stage| {
            let view = state.views.get(&stage);
            let source = view.and_then(|v| v.source.clone());
            let device = state.devices.get(&stage);
            StageCard {
                stage: pipeline_stage(stage),
                backends: state
                    .catalog
                    .backends(stage)
                    .unwrap_or_default()
                    .iter()
                    .map(|b| Choice {
                        id: b.source.clone(),
                        name: b.name.clone(),
                    })
                    .collect(),
                models: source
                    .as_deref()
                    .map_or_default(|source| page::models(state, stage, source)),
                source,
                model: view.and_then(|v| v.model.clone()),
                loaded: view.is_some_and(|v| v.loaded),
                device: device
                    .map(|d| d.device.clone())
                    .filter(|d| !d.is_empty() && d != "none"),
                devices: device.map_or_default(|d| {
                    d.available_devices
                        .iter()
                        .map(|id| Choice {
                            id: id.clone(),
                            name: accel_name(id),
                        })
                        .collect()
                }),
                busy: state.busy.contains(&stage),
            }
        })
        .collect();
    ModelsPage {
        cards,
        notice: held.notice.clone(),
    }
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
    super_voice_app_core::i18n::text(
        match stage {
            Stage::Transcription => "transcription",
            Stage::PostProcessing => "post-processing",
            Stage::Synthesis => "synthesis",
        },
        &[],
    )
}

/// What the cards tell the user, in the app's words.
fn notice_text(notice: Notice) -> String {
    let (key, stage, error) = match notice {
        Notice::CouldNotRead(stage, e) => ("stage-could-not-read", stage, e),
        Notice::CouldNotChange(stage, e) => ("stage-could-not-change", stage, e),
    };
    crate::text_with(
        key,
        HashMap::from([
            ("stage".to_string(), stage_name(stage)),
            ("error".to_string(), error),
        ]),
    )
}
