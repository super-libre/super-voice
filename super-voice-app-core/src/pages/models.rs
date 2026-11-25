// SPDX-License-Identifier: GPL-3.0-only
//! The Models page's stage cards: which backend fills each stage, which of
//! its models runs, on what, and loading or unloading it. Read from the
//! daemon's view of each stage, so what the card shows is what the daemon has.
//!
//! Each change is sent, and the stage read again when it is done: a load can
//! take minutes (a download first), and the daemon's events say how far it is
//! in the meantime. A stage with a change in flight is busy, and its card
//! waits.
//!
//! The settings app still runs its cards on its own handlers; this is what
//! the macOS app's run on.

use std::collections::{HashMap, HashSet};

use crate::daemon::backends::BackendInfo;
use crate::daemon::client::v1::pipeline::device::ModelDevice;
use crate::daemon::client::{self, StageState};
use crate::state::Stage;
use crate::state::stage_catalog::StageCatalog;

/// The stages a card is shown for.
pub const STAGES: [Stage; 3] = [
    Stage::Transcription,
    Stage::PostProcessing,
    Stage::Synthesis,
];

/// The Models page's cards.
#[derive(Debug, Default)]
pub struct ModelsState {
    /// Each stage as the daemon last reported it.
    pub views: HashMap<Stage, StageState>,
    /// The backends that can fill each stage, and their models.
    pub catalog: StageCatalog,
    /// The device each stage's model runs on, and the ones it could.
    pub devices: HashMap<Stage, ModelDevice>,
    /// The stages with a change in flight.
    pub busy: HashSet<Stage>,
}

/// What happened.
#[derive(Debug, Clone)]
pub enum Input {
    /// Read every stage: the page was opened, or the daemon is back.
    Reload,
    /// Read `stage` again: the daemon said it changed.
    Changed(Stage),
    ViewLoaded(Stage, Result<StageState, String>),
    BackendsLoaded(Stage, Result<Vec<BackendInfo>, String>),
    DeviceLoaded(Stage, Result<ModelDevice, String>),
    /// A backend was picked for `stage`; `None` leaves the stage empty.
    PickBackend(Stage, Option<String>),
    /// A model was picked and loaded.
    PickModel(Stage, String),
    /// A device was picked for the stage's model.
    PickDevice(Stage, String),
    /// Load the stage's model.
    Load(Stage),
    Unload(Stage),
    /// A change finished, or why not.
    Done(Stage, Result<(), String>),
}

/// What the app should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Read the stage: [`run_view`].
    ReadView(Stage),
    /// Read which backends can fill it: [`run_backends`].
    ReadBackends(Stage),
    /// Read its model's devices: [`run_device`].
    ReadDevice(Stage, String),
    SetBackend(Stage, Option<String>),
    SetModel {
        stage: Stage,
        model: String,
        source: Option<String>,
    },
    SetDevice {
        stage: Stage,
        model: String,
        device: String,
    },
    Unload(Stage),
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    CouldNotRead(Stage, String),
    CouldNotChange(Stage, String),
}

/// What an input came to.
#[derive(Debug, Default)]
pub struct Outcome {
    pub effects: Vec<Effect>,
    /// Take the notice down.
    pub clear_notice: bool,
    pub notice: Option<Notice>,
}

impl Outcome {
    fn effects(effects: Vec<Effect>) -> Self {
        Self {
            effects,
            ..Self::default()
        }
    }

    fn notice(notice: Notice) -> Self {
        Self {
            notice: Some(notice),
            ..Self::default()
        }
    }

    /// Start a change on `stage`, with the notice down.
    fn change(cards: &mut ModelsState, stage: Stage, effect: Effect) -> Self {
        if !cards.busy.insert(stage) {
            // One change at a time per stage: the card waits.
            return Self::default();
        }
        Self {
            effects: vec![effect],
            clear_notice: true,
            notice: None,
        }
    }
}

/// Take in `input`, and say what to do.
pub fn update(cards: &mut ModelsState, input: Input) -> Outcome {
    match input {
        Input::Reload => Outcome::effects(
            STAGES
                .iter()
                .flat_map(|stage| [Effect::ReadView(*stage), Effect::ReadBackends(*stage)])
                .collect(),
        ),
        Input::Changed(stage) => Outcome::effects(vec![Effect::ReadView(stage)]),
        Input::ViewLoaded(stage, Ok(view)) => {
            // The device belongs to the model the stage runs now.
            let device = view
                .model
                .clone()
                .map(|model| Effect::ReadDevice(stage, model));
            if view.model.is_none() {
                cards.devices.remove(&stage);
            }
            cards.views.insert(stage, view);
            Outcome::effects(device.into_iter().collect())
        }
        Input::ViewLoaded(stage, Err(e)) => Outcome::notice(Notice::CouldNotRead(stage, e)),
        Input::BackendsLoaded(stage, Ok(backends)) => {
            cards.catalog.record(stage, backends);
            Outcome::default()
        }
        Input::BackendsLoaded(stage, Err(e)) => {
            log::warn!("Could not read the {stage} stage's backends: {e}");
            Outcome::default()
        }
        Input::DeviceLoaded(stage, Ok(device)) => {
            cards.devices.insert(stage, device);
            Outcome::default()
        }
        Input::DeviceLoaded(stage, Err(e)) => {
            log::warn!("Could not read the {stage} model's device: {e}");
            cards.devices.remove(&stage);
            Outcome::default()
        }
        Input::PickBackend(stage, source) => {
            Outcome::change(cards, stage, Effect::SetBackend(stage, source))
        }
        Input::PickModel(stage, model) => {
            let source = cards.views.get(&stage).and_then(|v| v.source.clone());
            Outcome::change(
                cards,
                stage,
                Effect::SetModel {
                    stage,
                    model,
                    source,
                },
            )
        }
        Input::PickDevice(stage, device) => {
            let Some(model) = cards.views.get(&stage).and_then(|v| v.model.clone()) else {
                return Outcome::default();
            };
            Outcome::change(
                cards,
                stage,
                Effect::SetDevice {
                    stage,
                    model,
                    device,
                },
            )
        }
        Input::Load(stage) => {
            let Some(view) = cards.views.get(&stage) else {
                return Outcome::default();
            };
            let Some(model) = view.model.clone() else {
                return Outcome::default();
            };
            let source = view.source.clone();
            Outcome::change(
                cards,
                stage,
                Effect::SetModel {
                    stage,
                    model,
                    source,
                },
            )
        }
        Input::Unload(stage) => Outcome::change(cards, stage, Effect::Unload(stage)),
        Input::Done(stage, result) => {
            cards.busy.remove(&stage);
            let mut outcome = Outcome::effects(vec![Effect::ReadView(stage)]);
            if let Err(e) = result {
                outcome.notice = Some(Notice::CouldNotChange(stage, e));
            }
            outcome
        }
    }
}

/// The models `source` serves in `stage`, as the card offers them.
#[must_use]
pub fn models(cards: &ModelsState, stage: Stage, source: &str) -> Vec<String> {
    cards.catalog.models(stage, source)
}

pub async fn run_view(stage: Stage) -> Input {
    Input::ViewLoaded(
        stage,
        client::get_stage_view(stage)
            .await
            .map_err(|e| e.to_string()),
    )
}

pub async fn run_backends(stage: Stage) -> Input {
    Input::BackendsLoaded(
        stage,
        client::list_stage_backends(stage)
            .await
            .map_err(|e| e.to_string()),
    )
}

pub async fn run_device(stage: Stage, model: String) -> Input {
    Input::DeviceLoaded(
        stage,
        client::get_model_device(stage, model)
            .await
            .map_err(|e| e.to_string()),
    )
}

/// Run `effect`, a change: the stage is read again when it is done.
pub async fn run_change(effect: Effect) -> Option<Input> {
    let (stage, result) = match effect {
        Effect::SetBackend(stage, Some(source)) => {
            (stage, client::set_stage_backend(stage, source).await)
        }
        Effect::SetBackend(stage, None) => (stage, client::clear_stage_backend(stage).await),
        Effect::SetModel {
            stage,
            model,
            source,
        } => (stage, client::set_stage_model(stage, model, source).await),
        Effect::SetDevice {
            stage,
            model,
            device,
        } => (stage, client::set_model_device(stage, model, device).await),
        Effect::Unload(stage) => (stage, client::unload_stage_model(stage).await),
        Effect::ReadView(_) | Effect::ReadBackends(_) | Effect::ReadDevice(..) => return None,
    };
    Some(Input::Done(stage, result.map_err(|e| e.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(model: Option<&str>) -> StageState {
        StageState {
            source: Some("whisper".into()),
            name: Some("Whisper".into()),
            enabled: true,
            model: model.map(String::from),
            loaded: false,
            state: crate::daemon::client::ModelState::NotLoaded,
            device: None,
        }
    }

    /// One change at a time per stage; the stage is read again when it is
    /// done, with the daemon's reason when it failed.
    #[test]
    fn a_stage_takes_one_change_at_a_time() {
        let mut state = ModelsState::default();
        state.views.insert(Stage::Transcription, view(Some("base")));
        let load = update(&mut state, Input::Load(Stage::Transcription));
        assert_eq!(
            load.effects,
            [Effect::SetModel {
                stage: Stage::Transcription,
                model: "base".into(),
                source: Some("whisper".into()),
            }]
        );
        assert!(
            update(&mut state, Input::Unload(Stage::Transcription))
                .effects
                .is_empty()
        );
        let done = update(
            &mut state,
            Input::Done(Stage::Transcription, Err("no memory".into())),
        );
        assert_eq!(done.effects, [Effect::ReadView(Stage::Transcription)]);
        assert_eq!(
            done.notice,
            Some(Notice::CouldNotChange(
                Stage::Transcription,
                "no memory".into()
            ))
        );
        assert!(state.busy.is_empty());
    }

    /// A stage's model brings its devices; without one, the devices go.
    #[test]
    fn a_stages_model_brings_its_devices() {
        let mut state = ModelsState::default();
        let read = update(
            &mut state,
            Input::ViewLoaded(Stage::Synthesis, Ok(view(Some("kokoro-82m")))),
        );
        assert_eq!(
            read.effects,
            [Effect::ReadDevice(Stage::Synthesis, "kokoro-82m".into())]
        );
        state
            .devices
            .insert(Stage::Synthesis, ModelDevice::default());
        update(
            &mut state,
            Input::ViewLoaded(Stage::Synthesis, Ok(view(None))),
        );
        assert!(!state.devices.contains_key(&Stage::Synthesis));
    }

    /// Nothing to load without a model picked.
    #[test]
    fn load_needs_a_model() {
        let mut state = ModelsState::default();
        state.views.insert(Stage::Transcription, view(None));
        assert!(
            update(&mut state, Input::Load(Stage::Transcription))
                .effects
                .is_empty()
        );
    }
}
