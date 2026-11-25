// SPDX-License-Identifier: GPL-3.0-only
//! The Models page's stage cards: which backend fills each stage, which of
//! its models to run and on what, and loading, reloading or unloading it,
//! with the download and the load shown as they go.
//!
//! A card stages a model and a device first, then Load commits both, as the
//! app's cards do. What the card shows as running comes from the
//! daemon's view of the stage, read again whenever the daemon says a stage
//! changed: the daemon is the one that knows.
//!
//! A load can take minutes (a download first). The daemon reports how far it
//! is on its `download_progress` events ([`Input::Progress`]), and the request
//! that started it answers once it is done. One change at a time per stage:
//! while one runs, the card waits.
//!
//! A framework picked whose build isn't installed asks first ([`Effect::OfferBuild`]),
//! then installs the build through the Library's install, then moves the model
//! onto it. A download or load the daemon stops reporting on fails after
//! [`STALL_TIMEOUT`], on the app's [`Input::Tick`].
//!
//! Both apps' cards run on this page.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use super_voice_shared::models::protocol::{DownloadProgress, GpuInfo};

use crate::daemon::backends::BackendInfo;
use crate::daemon::client::{self, ModelState, StageState};
use crate::rules::load::{StagedLoad, staged_load_device};
use crate::state::Stage;
use crate::state::device_offers::{DeviceOffers, build_installed, staged_device};
use crate::state::model_operations::{LoadCause, ModelOperationState, ModelOperations, Progressed};
use crate::state::stage_catalog::StageCatalog;
use crate::state::staged_picks::StagedPicks;

/// The stages a card is shown for.
pub const STAGES: [Stage; 3] = [
    Stage::Transcription,
    Stage::PostProcessing,
    Stage::Synthesis,
];

/// How long a download or load may go without a word from the daemon before
/// its card says it stalled. Generous, so a load onto the device that reports
/// nothing for a while, or a slow download, isn't cut off.
pub const STALL_TIMEOUT: Duration = Duration::from_mins(5);

mod heard;
mod summary;
pub use heard::heard;
pub use summary::{Memory, memory, stage_status};

/// A framework picked for a stage's model whose backend doesn't have that
/// build installed.
///
/// Picking one asks first, every time, because it is a download. Once the
/// user agrees the build is installed in place of the one there, keeping the
/// downloaded models, and the model moves onto the framework when the install
/// is done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingBuild {
    pub stage: Stage,
    /// The backend whose build this is.
    pub source: String,
    /// The framework picked, as the daemon names it (`cuda`).
    pub framework: String,
    /// The user agreed and the build is downloading. Until then the app asks.
    pub downloading: bool,
}

/// The Models page's cards.
#[derive(Debug, Default)]
pub struct ModelsState {
    /// Each stage as the daemon last reported it.
    pub views: HashMap<Stage, StageState>,
    /// The backends that can fill each stage, and their models.
    pub catalog: StageCatalog,
    /// The model and device each card has staged for Load.
    pub picks: StagedPicks,
    /// What each staged model can run on here.
    pub offers: DeviceOffers,
    /// Each stage's download or load, as far as it has come, or why it failed.
    pub operations: ModelOperations,
    /// Whether each backend's secrets are stored, by (source, name).
    pub secrets: HashMap<(String, String), bool>,
    /// The machine's GPUs, for whether a staged model looks too big.
    pub gpus: Vec<GpuInfo>,
    /// The stages with a request in flight that isn't a load.
    pub busy: HashSet<Stage>,
    /// A framework picked whose build isn't installed: being asked about, or
    /// downloading.
    pub pending_build: Option<PendingBuild>,
}

impl ModelsState {
    /// `stage` as the daemon last reported it; empty before it has.
    #[must_use]
    pub fn view(&self, stage: Stage) -> &StageState {
        static EMPTY: std::sync::OnceLock<StageState> = std::sync::OnceLock::new();
        self.views
            .get(&stage)
            .unwrap_or_else(|| EMPTY.get_or_init(StageState::default))
    }

    /// The build downloading for `stage`, once the user agreed to it.
    #[must_use]
    pub fn build_downloading(&self, stage: Stage) -> Option<&PendingBuild> {
        self.pending_build
            .as_ref()
            .filter(|p| p.downloading && p.stage == stage)
    }

    /// The build the app asks about, until the user answers.
    #[must_use]
    pub fn build_question(&self) -> Option<&PendingBuild> {
        self.pending_build.as_ref().filter(|p| !p.downloading)
    }
}

/// What happened.
#[derive(Debug, Clone)]
pub enum Input {
    /// Read every stage: the page was opened, or the daemon's events are
    /// live again.
    Reload,
    /// The daemon is back after it was gone: what was under way went with the
    /// session it belonged to. Read every stage.
    Reconnected,
    /// Read which backends can fill each stage again: one was installed,
    /// updated or removed, or its settings changed.
    ReloadBackends,
    /// Read the machine's GPUs again, for their memory in use.
    RefreshGpus,
    /// Read `stage` again: the daemon said it changed.
    Changed(Stage),
    /// The daemon said `stage` settled: its model came up (`loaded`), or left
    /// memory. A load it reported is over; the stage is read again.
    Ready {
        stage: Stage,
        loaded: bool,
    },
    /// The daemon said it started loading `model` in `stage`, whoever asked.
    LoadStarted {
        stage: Stage,
        model: String,
    },
    /// The daemon couldn't move `stage`'s model to another device; it is back
    /// on the one it had.
    DeviceSwitchFailed(Stage, String),
    /// Time passed: a download or load the daemon stopped reporting on fails.
    Tick,
    ViewLoaded(Stage, Result<StageState, String>),
    BackendsLoaded(Stage, Result<Vec<BackendInfo>, String>),
    SecretsLoaded {
        source: String,
        items: Vec<(String, bool)>,
    },
    GpusLoaded(Vec<GpuInfo>),
    /// What a stage's backend runs its models on, or why not.
    BackendDevicesLoaded {
        stage: Stage,
        source: String,
        result: Result<Vec<String>, String>,
    },
    /// What the daemon offers a model on, and the device it has; or why not.
    DevicesLoaded {
        stage: Stage,
        source: String,
        model: String,
        result: Result<(Vec<String>, Option<String>), String>,
    },
    /// How the stage's download or load is coming along.
    Progress(Stage, DownloadProgress),
    /// The daemon's answer to a poll: `None` when nothing is under way.
    Polled(Stage, Option<DownloadProgress>),
    /// A backend was picked; `None` leaves the stage empty.
    PickBackend(Stage, Option<String>),
    /// A model was picked: staged for Load.
    PickModel(Stage, String),
    /// A device was picked: staged, or the running model moved onto it. One
    /// whose build isn't installed asks first ([`Effect::OfferBuild`]).
    PickDevice(Stage, String),
    /// The user agreed to download the build asked about.
    ConfirmBuild,
    /// The user declined it. Nothing changes.
    CancelBuild,
    /// `source` finished installing: a build the user asked for moves the
    /// model onto its framework.
    BuildInstalled {
        source: String,
    },
    /// `source` failed to install.
    BuildFailed {
        source: String,
        error: String,
    },
    /// Load the staged model on the staged device.
    Load(Stage),
    /// Bring the running model back up on the device it has.
    ReloadModel(Stage),
    Unload(Stage),
    /// Stop the stage's download.
    CancelDownload(Stage),
    /// A request finished, or why not.
    Done(Stage, Change, Result<(), String>),
}

/// Which request an [`Input::Done`] answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Backend,
    /// A load, a reload, or a running model moved to another device: the
    /// stage's operation ends with it.
    Load,
    /// A device set for a model that isn't running.
    Device,
    Unload,
    Cancel,
}

/// What the app should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Read the stage: [`run_view`].
    ReadView(Stage),
    /// Read which backends can fill it: [`run_backends`].
    ReadBackends(Stage),
    /// Read which of a backend's secrets are stored: [`run_secrets`].
    ReadSecrets(String),
    /// Read the machine's GPUs: [`run_gpus`].
    ReadGpus,
    /// Read what a stage's backend runs its models on: [`run_backend_devices`].
    ReadBackendDevices {
        stage: Stage,
        source: String,
    },
    /// Read what a model can run on: [`run_devices`].
    ReadDevices {
        stage: Stage,
        source: String,
        model: String,
    },
    /// Ask how the stage's download or load is coming along: [`run_poll`].
    Poll(Stage),
    SetBackend(Stage, Option<String>),
    /// Set the model's device first when `device` is `Some`, then load it.
    Load {
        stage: Stage,
        model: String,
        source: String,
        device: Option<String>,
    },
    /// Set a model's device. `running`: it is up, and moves onto it now.
    SetDevice {
        stage: Stage,
        model: String,
        device: String,
        running: bool,
    },
    ReloadModel(Stage),
    Unload(Stage),
    CancelDownload(Stage),
    /// The card shows `model` now: read its language, and for synthesis its
    /// voice.
    Focus {
        stage: Stage,
        source: String,
        model: String,
    },
    /// The synthesis model changed, or came up or went: what the Voices tab
    /// offers follows it.
    VoicesChanged,
    /// Ask whether to download `source`'s build for `framework`
    /// ([`ModelsState::build_question`]). An app that has the build on disk
    /// already may answer [`Input::ConfirmBuild`] at once: that isn't a
    /// download.
    OfferBuild {
        stage: Stage,
        source: String,
        framework: String,
    },
    /// Install `source`'s build for `framework`, through the Library's
    /// install: [`run_install_build`]. Its end comes back as
    /// [`Input::BuildInstalled`] or [`Input::BuildFailed`].
    InstallBuild {
        source: String,
        framework: String,
    },
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    CouldNotRead(Stage, String),
    CouldNotChange(Stage, String),
    /// The daemon couldn't move the stage's model to another device.
    DeviceSwitchFailed(Stage, String),
    /// The build the user asked for didn't install.
    BuildFailed {
        stage: Stage,
        framework: String,
        error: String,
    },
    /// The stage's download or load stopped reporting; its card shows it
    /// failed.
    Stalled(Stage),
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

    /// Start `effect`, with the notice down.
    fn start(effect: Effect) -> Self {
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
        Input::Reload => reload(),
        Input::Reconnected => {
            cards.operations.reset();
            cards.busy.clear();
            reload()
        }
        Input::ReloadBackends => Outcome::effects(STAGES.map(Effect::ReadBackends).to_vec()),
        Input::RefreshGpus => Outcome::effects(vec![Effect::ReadGpus]),
        Input::Changed(stage) => Outcome::effects(vec![Effect::ReadView(stage)]),
        input @ (Input::Ready { .. }
        | Input::LoadStarted { .. }
        | Input::DeviceSwitchFailed(..)
        | Input::Tick) => daemon_said(cards, input),
        input @ (Input::BackendsLoaded(..)
        | Input::SecretsLoaded { .. }
        | Input::GpusLoaded(_)
        | Input::BackendDevicesLoaded { .. }
        | Input::DevicesLoaded { .. }) => answered(cards, input),
        input @ (Input::ConfirmBuild
        | Input::CancelBuild
        | Input::BuildInstalled { .. }
        | Input::BuildFailed { .. }) => build(cards, input),
        Input::ViewLoaded(stage, Ok(view)) => Outcome::effects(view_loaded(cards, stage, view)),
        Input::ViewLoaded(stage, Err(e)) => Outcome::notice(Notice::CouldNotRead(stage, e)),
        Input::Progress(stage, progress) | Input::Polled(stage, Some(progress)) => {
            progressed(cards, stage, &progress);
            Outcome::default()
        }
        Input::Polled(stage, None) => {
            // Nothing is downloading. A load reports its own end, so only a
            // download the card still shows is over.
            if matches!(
                cards.operations.get(stage),
                ModelOperationState::Provisioning { .. }
            ) {
                cards.operations.set_ready(stage);
            }
            Outcome::default()
        }
        Input::PickBackend(stage, source) => pick_backend(cards, stage, source),
        Input::PickModel(stage, model) => pick_model(cards, stage, model),
        Input::PickDevice(stage, device) => pick_framework(cards, stage, device),
        Input::Load(stage) => load(cards, stage),
        Input::ReloadModel(stage) => {
            let Some(model) = running_model(cards, stage).map(ToString::to_string) else {
                return Outcome::default();
            };
            if !ready(cards, stage) {
                return Outcome::default();
            }
            cards
                .operations
                .start(stage, loading(model, LoadCause::Reload));
            Outcome::start(Effect::ReloadModel(stage))
        }
        Input::Unload(stage) => {
            if !cards.busy.insert(stage) {
                return Outcome::default();
            }
            cards.operations.set_ready(stage);
            Outcome::start(Effect::Unload(stage))
        }
        Input::CancelDownload(stage) => Outcome::start(Effect::CancelDownload(stage)),
        Input::Done(stage, change, result) => done(cards, stage, change, result),
    }
}

/// What the daemon said on its own: a load started or ended, a device move
/// failed; or time passed.
fn daemon_said(cards: &mut ModelsState, input: Input) -> Outcome {
    match input {
        Input::Ready { stage, loaded } => {
            // An unload says nothing of a load the user started meanwhile.
            if loaded && cards.operations.get(stage).is_pending() {
                cards.operations.set_ready(stage);
            }
            Outcome::effects(vec![Effect::ReadView(stage)])
        }
        Input::LoadStarted { stage, model } => {
            // A load this app started already says so, maybe with more.
            if !cards.operations.get(stage).is_pending() {
                cards
                    .operations
                    .start(stage, loading(model, LoadCause::Reported));
            }
            Outcome::default()
        }
        Input::DeviceSwitchFailed(stage, error) => {
            show_failure(cards, stage, &error);
            let mut effects = vec![Effect::ReadView(stage)];
            effects.extend(restage_device(cards, stage));
            Outcome {
                effects,
                clear_notice: false,
                notice: Some(Notice::DeviceSwitchFailed(stage, error)),
            }
        }
        Input::Tick => {
            // Worded by the app: the notice says which stage stalled.
            let stalled = cards.operations.fail_stalled(STALL_TIMEOUT, "");
            for stage in &stalled {
                log::warn!(
                    "The {stage} stage's download or load stalled: no word from the daemon for {}s",
                    STALL_TIMEOUT.as_secs()
                );
            }
            stalled.first().map_or_else(Outcome::default, |stage| {
                Outcome::notice(Notice::Stalled(*stage))
            })
        }
        _ => Outcome::default(),
    }
}

/// What the daemon answered to a read the page asked for.
fn answered(cards: &mut ModelsState, input: Input) -> Outcome {
    match input {
        Input::BackendsLoaded(stage, Ok(backends)) => {
            // Each backend's secrets decide whether its card can load at all.
            let effects = backends
                .iter()
                .filter(|b| !b.secrets.is_empty())
                .map(|b| Effect::ReadSecrets(b.source.clone()))
                .collect();
            cards.catalog.record(stage, backends);
            Outcome::effects(effects)
        }
        Input::BackendsLoaded(stage, Err(e)) => {
            log::warn!("Could not read the {stage} stage's backends: {e}");
            Outcome::default()
        }
        Input::SecretsLoaded { source, items } => {
            for (name, stored) in items {
                cards.secrets.insert((source.clone(), name), stored);
            }
            Outcome::default()
        }
        Input::GpusLoaded(gpus) => {
            cards.gpus = gpus;
            Outcome::default()
        }
        Input::BackendDevicesLoaded {
            stage,
            source,
            result,
        } => {
            match result {
                // An answer about a backend the stage no longer has would evict
                // the current one's.
                Ok(devices) if cards.view(stage).source.as_deref() == Some(source.as_str()) => {
                    cards.offers.record(stage, source, None, devices);
                }
                Ok(_) => {}
                // The chips fall back to the catalog's own reading.
                Err(e) => log::warn!("Could not read the devices for {source}: {e}"),
            }
            Outcome::default()
        }
        Input::DevicesLoaded {
            stage,
            source,
            model,
            result,
        } => {
            devices_loaded(cards, stage, source, &model, result);
            Outcome::default()
        }
        _ => Outcome::default(),
    }
}

/// A framework's build: the user's answer, and the install's end.
fn build(cards: &mut ModelsState, input: Input) -> Outcome {
    match input {
        Input::ConfirmBuild => confirm_build(cards),
        Input::CancelBuild => {
            if cards.build_question().is_some() {
                cards.pending_build = None;
            }
            Outcome::default()
        }
        Input::BuildInstalled { source } => build_installed_for(cards, &source),
        Input::BuildFailed { source, error } => {
            let Some(pending) = take_build(cards, &source) else {
                return Outcome::default();
            };
            // The build that was there is untouched.
            Outcome::notice(Notice::BuildFailed {
                stage: pending.stage,
                framework: pending.framework,
                error,
            })
        }
        _ => Outcome::default(),
    }
}

/// Read every stage, what can fill it, and the GPUs.
fn reload() -> Outcome {
    let mut effects: Vec<Effect> = STAGES
        .iter()
        .flat_map(|stage| [Effect::ReadView(*stage), Effect::ReadBackends(*stage)])
        .collect();
    effects.push(Effect::ReadGpus);
    Outcome::effects(effects)
}

/// Show a failure on `stage`'s card, where the user is looking: the
/// daemon's words, or the app's when it words them itself. The user's home
/// directory is collapsed back to `$HOME`.
pub fn show_failure(cards: &mut ModelsState, stage: Stage, message: &str) {
    let home = std::env::var("HOME").unwrap_or_default();
    cards.operations.set(
        stage,
        ModelOperationState::Error {
            message: crate::rules::load::sanitize_home(message, &home),
        },
    );
}

/// Whether `stage` is free to start a change.
fn ready(cards: &ModelsState, stage: Stage) -> bool {
    let settled = cards.operations.is_ready(stage)
        || matches!(
            cards.operations.get(stage),
            ModelOperationState::Error { .. }
        );
    settled && !cards.busy.contains(&stage)
}

/// A load of `model` just starting: the card says so until the daemon reports
/// how far it is.
fn loading(model: String, cause: LoadCause) -> ModelOperationState {
    ModelOperationState::Loading {
        target_model: model,
        cause,
        load: None,
    }
}

/// The model `stage` has up, as the daemon reports it.
#[must_use]
pub fn running_model(cards: &ModelsState, stage: Stage) -> Option<&str> {
    let view = cards.views.get(&stage)?;
    if view.loaded {
        view.model.as_deref()
    } else {
        None
    }
}

/// The model `stage` keeps out of memory until its next use, when nothing is
/// under way on it.
#[must_use]
pub fn idle_model(cards: &ModelsState, stage: Stage) -> Option<&str> {
    let view = cards.views.get(&stage)?;
    if view.state == ModelState::Idle && cards.operations.is_ready(stage) {
        view.model.as_deref()
    } else {
        None
    }
}

/// Take the daemon's view of `stage`. Its selection is staged like any pick,
/// unless the user has one of their own on the same backend, so the card offers
/// the model the daemon remembers and asks what it runs on.
fn view_loaded(cards: &mut ModelsState, stage: Stage, view: StageState) -> Vec<Effect> {
    let mut effects = Vec::new();
    let previous = cards.view(stage);
    let selection_changed = previous.selection() != view.selection();
    let source_changed = previous.source != view.source;
    // What the Voices tab offers follows the synthesis model.
    let voices_changed =
        stage == Stage::Synthesis && (selection_changed || previous.loaded != view.loaded);
    let mut focus = selection_changed;
    match view.selection() {
        Some((model, source)) => {
            let own = cards
                .picks
                .get(stage)
                .is_some_and(|pick| pick.source == source);
            if !own {
                cards
                    .picks
                    .stage_model(stage, source.clone(), model.clone());
                effects.push(Effect::ReadDevices {
                    stage,
                    source: source.clone(),
                    model: model.clone(),
                });
                focus = true;
            }
            if focus {
                effects.push(Effect::Focus {
                    stage,
                    source,
                    model,
                });
            }
        }
        None => {
            // A backend picked and no model: a pick on another backend is
            // stale.
            if cards.picks.get(stage).map(|pick| pick.source.as_str()) != view.source.as_deref() {
                cards.picks.clear(stage);
            }
        }
    }
    // The card's chips are the daemon's answer for the backend there now.
    if let Some(source) = &view.source
        && (source_changed || cards.offers.backend(stage, source).is_none())
    {
        effects.push(Effect::ReadBackendDevices {
            stage,
            source: source.clone(),
        });
    }
    if cards.operations.get(stage).is_pending() {
        effects.push(Effect::Poll(stage));
    } else {
        adopt_running_device(cards, stage, &view);
    }
    if voices_changed {
        effects.push(Effect::VoicesChanged);
    }
    cards.views.insert(stage, view);
    effects
}

/// Show the device the running model is on as its pick, when another client
/// moved it. Only for the model the card has picked, and only a device the
/// picker offers.
fn adopt_running_device(cards: &mut ModelsState, stage: Stage, view: &StageState) {
    let (Some(model), Some(source), Some(device)) = (
        view.model.as_deref().filter(|_| view.loaded),
        view.source.as_deref(),
        view.device.as_ref().map(|d| d.preference.as_str()),
    ) else {
        return;
    };
    if cards.picks.model(stage, source) != Some(model) {
        return;
    }
    let offered = cards
        .offers
        .model(stage, source, model)
        .is_some_and(|devices| devices.iter().any(|d| d == device));
    if offered {
        cards.picks.set_device(stage, Some(device.to_string()));
    }
}

/// Take what the daemon offers `model` on. An answer about a backend or a model
/// the card no longer has staged is dropped: a pick made since wins.
fn devices_loaded(
    cards: &mut ModelsState,
    stage: Stage,
    source: String,
    model: &str,
    result: Result<(Vec<String>, Option<String>), String>,
) {
    let (devices, current) = match result {
        Ok(answer) => answer,
        Err(e) => {
            // Nothing is staged onto a device the daemon never offered, so a
            // failed read leaves the picker hidden and Load disabled.
            log::warn!("Could not read the devices for {model}: {e}");
            return;
        }
    };
    if cards.picks.model(stage, &source) != Some(model) {
        return;
    }
    cards
        .offers
        .record(stage, source, Some(model.to_string()), devices.clone());
    // The model's own device when the picker can show it, otherwise the first
    // offered, and nothing when none is, which keeps Load disabled.
    cards
        .picks
        .set_device(stage, staged_device(&devices, current));
}

fn progressed(cards: &mut ModelsState, stage: Stage, progress: &DownloadProgress) {
    if cards.operations.apply_progress(stage, progress) == Progressed::Finished
        && progress.status == "cancelled"
    {
        cards.operations.set_ready(stage);
    }
}

fn pick_backend(cards: &mut ModelsState, stage: Stage, source: Option<String>) -> Outcome {
    // Picking what is already there is not a change: sending it would unload
    // the running model for nothing.
    let current = cards.views.get(&stage).and_then(|v| v.source.clone());
    if current == source || !ready(cards, stage) || !cards.busy.insert(stage) {
        return Outcome::default();
    }
    cards.picks.clear(stage);
    cards.operations.set_ready(stage);
    Outcome::start(Effect::SetBackend(stage, source))
}

fn pick_model(cards: &mut ModelsState, stage: Stage, model: String) -> Outcome {
    let Some(source) = cards.views.get(&stage).and_then(|v| v.source.clone()) else {
        return Outcome::default();
    };
    cards
        .picks
        .stage_model(stage, source.clone(), model.clone());
    // A new pick is a fresh start: the last one's failure no longer applies.
    if matches!(
        cards.operations.get(stage),
        ModelOperationState::Error { .. }
    ) {
        cards.operations.set_ready(stage);
    }
    Outcome {
        effects: vec![
            Effect::ReadDevices {
                stage,
                source: source.clone(),
                model: model.clone(),
            },
            // So the card shows the model's language and voice before Load.
            Effect::Focus {
                stage,
                source,
                model,
            },
        ],
        clear_notice: true,
        notice: None,
    }
}

/// A framework picked in `stage`'s Runs on row. One whose build is installed
/// is chosen at once; one whose build isn't asks first, since getting it
/// means installing it over the one there.
fn pick_framework(cards: &mut ModelsState, stage: Stage, device: String) -> Outcome {
    // One build at a time: the pick waits for it.
    if cards.build_downloading(stage).is_some() {
        return Outcome::default();
    }
    let source = cards.view(stage).source.clone();
    let installed = source
        .as_deref()
        .and_then(|source| cards.catalog.backend(stage, source))
        .is_none_or(|b| build_installed(&device, &b.installed_accel));
    match source {
        Some(source) if !installed => {
            cards.pending_build = Some(PendingBuild {
                stage,
                source: source.clone(),
                framework: device.clone(),
                downloading: false,
            });
            Outcome::effects(vec![Effect::OfferBuild {
                stage,
                source,
                framework: device,
            }])
        }
        _ => pick_device(cards, stage, device),
    }
}

/// The user agreed, or the build is on disk: install it.
fn confirm_build(cards: &mut ModelsState) -> Outcome {
    let Some(pending) = cards.pending_build.as_mut().filter(|p| !p.downloading) else {
        return Outcome::default();
    };
    pending.downloading = true;
    Outcome::start(Effect::InstallBuild {
        source: pending.source.clone(),
        framework: pending.framework.clone(),
    })
}

/// The build the user agreed to for `source`, now its install has ended.
fn take_build(cards: &mut ModelsState, source: &str) -> Option<PendingBuild> {
    cards
        .pending_build
        .take_if(|p| p.downloading && p.source == source)
}

/// `source` installed. When that was a build the user asked for, move the
/// model onto its framework: a running model reloads onto it, one not
/// running has it staged for its Load. The catalog is read again, for the
/// build it has now.
fn build_installed_for(cards: &mut ModelsState, source: &str) -> Outcome {
    let Some(pending) = take_build(cards, source) else {
        return Outcome::default();
    };
    // The stage moved on to another backend while the build downloaded.
    if cards.view(pending.stage).source.as_deref() != Some(source) {
        return Outcome::default();
    }
    let mut outcome = pick_device(cards, pending.stage, pending.framework);
    outcome.effects.push(Effect::ReadBackends(pending.stage));
    outcome
}

/// Choose the device `stage`'s model runs on. Staged while nothing is up,
/// for Load to commit. Once the picked model is running the choice goes to
/// the daemon, which reloads it onto the new device in place; for a model
/// that is idle, the daemon keeps it for the next load.
fn pick_device(cards: &mut ModelsState, stage: Stage, device: String) -> Outcome {
    if cards.picks.device(stage) == Some(device.as_str()) {
        return Outcome::default();
    }
    cards.picks.set_device(stage, Some(device.clone()));
    let Some(picked) = cards.picks.get(stage).map(|pick| pick.model.clone()) else {
        return Outcome::default();
    };
    let running = running_model(cards, stage) == Some(picked.as_str());
    let idle = idle_model(cards, stage) == Some(picked.as_str());
    if !(running || idle) || !ready(cards, stage) {
        return Outcome::default();
    }
    if running {
        cards.operations.start(
            stage,
            loading(
                picked.clone(),
                LoadCause::Move {
                    device: device.clone(),
                },
            ),
        );
    } else {
        cards.busy.insert(stage);
    }
    Outcome::start(Effect::SetDevice {
        stage,
        model: picked,
        device,
        running,
    })
}

/// Commit `stage`'s staged pick: set the model's device, then load it.
fn load(cards: &mut ModelsState, stage: Stage) -> Outcome {
    let Some(pick) = cards.picks.get(stage).cloned() else {
        return Outcome::default();
    };
    if !ready(cards, stage) {
        return Outcome::default();
    }
    let backends = cards.catalog.backends(stage).unwrap_or_default();
    // A backend can be uninstalled between staging and the click.
    let StagedLoad::Load { device_to_set } =
        staged_load_device(backends, &pick.source, &pick.model, pick.device.as_deref())
    else {
        return Outcome::default();
    };
    cards
        .operations
        .start(stage, loading(pick.model.clone(), LoadCause::Switch));
    Outcome::start(Effect::Load {
        stage,
        model: pick.model,
        source: pick.source,
        device: device_to_set,
    })
}

fn done(
    cards: &mut ModelsState,
    stage: Stage,
    change: Change,
    result: Result<(), String>,
) -> Outcome {
    cards.busy.remove(&stage);
    let mut effects = vec![Effect::ReadView(stage)];
    let mut notice = None;
    match (change, result) {
        (Change::Load | Change::Cancel | Change::Unload, Ok(())) => {
            cards.operations.set_ready(stage);
        }
        (Change::Backend | Change::Device, Ok(())) => {}
        // A failed load, move or unload shows on the card itself, where the
        // user is looking.
        (Change::Load | Change::Cancel | Change::Unload, Err(e)) => {
            show_failure(cards, stage, &e);
            // The device the daemon kept is the one to show again.
            effects.extend(restage_device(cards, stage));
        }
        (Change::Backend | Change::Device, Err(e)) => {
            effects.extend(restage_device(cards, stage));
            notice = Some(Notice::CouldNotChange(stage, e));
        }
    }
    Outcome {
        effects,
        clear_notice: false,
        notice,
    }
}

/// Read the staged model's devices again, which stages the device the daemon
/// has for it.
fn restage_device(cards: &ModelsState, stage: Stage) -> Option<Effect> {
    let pick = cards.picks.get(stage)?;
    Some(Effect::ReadDevices {
        stage,
        source: pick.source.clone(),
        model: pick.model.clone(),
    })
}

/// What a stage's card shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shown {
    /// Nothing fills the stage.
    Empty,
    /// The backend's model is up, or out of memory until its next use
    /// (`idle`).
    Running { model: String, idle: bool },
    /// The model staged for Load, if any.
    Staging { model: Option<String> },
}

/// What a stage's card shows, worked out from the page's state.
#[derive(Debug, Clone)]
pub struct Card<'a> {
    pub stage: Stage,
    /// The backend filling the stage, when it is one of those that can.
    pub backend: Option<&'a BackendInfo>,
    /// The backend's source, even when it isn't in the catalog yet.
    pub source: Option<String>,
    /// The labels of required secrets and options not set yet. While any are,
    /// the card offers no model: Configure is the way forward.
    pub unmet: Vec<&'a str>,
    pub shown: Shown,
    /// The models the backend serves in this stage.
    pub models: Vec<String>,
    /// What the shown model can run on here; `None` before the daemon says.
    pub devices: Option<&'a [String]>,
    /// The device picked for the shown model.
    pub device: Option<&'a str>,
    /// The framework a running model fell back to, when not the one picked.
    pub fallback: Option<String>,
    /// How much memory the staged model says it needs.
    pub memory: Option<u64>,
    /// The staged model and device are ready to load.
    pub can_load: bool,
    /// The staged model can't run on anything this install has.
    pub no_device: bool,
    /// The staged model looks too big for the GPU: (needed, available).
    pub vram_shortfall: Option<(u64, u64)>,
    /// The stage's download or load, or why it failed.
    pub operation: &'a ModelOperationState,
    /// A request is in flight; the card waits.
    pub busy: bool,
    /// The build the user agreed to for this stage, while it downloads.
    pub build: Option<&'a PendingBuild>,
}

/// `stage`'s card.
#[must_use]
pub fn card(cards: &ModelsState, stage: Stage) -> Card<'_> {
    let view = cards.views.get(&stage);
    let source = view.and_then(|v| v.source.clone());
    let backend = source
        .as_deref()
        .and_then(|source| cards.catalog.backend(stage, source));
    let unmet = backend.map_or_else(Vec::new, |b| {
        crate::rules::status::unmet_requirements(&cards.secrets, b)
    });
    let picked = source
        .as_deref()
        .and_then(|source| cards.picks.model(stage, source));
    let shown = match (
        &source,
        running_model(cards, stage),
        idle_model(cards, stage),
    ) {
        (None, ..) => Shown::Empty,
        (Some(_), Some(model), _) => Shown::Running {
            model: model.to_string(),
            idle: false,
        },
        (Some(_), None, Some(model)) => Shown::Running {
            model: model.to_string(),
            idle: true,
        },
        (Some(_), None, None) => Shown::Staging {
            model: picked.map(ToString::to_string),
        },
    };
    let shown_model = match &shown {
        Shown::Running { model, .. } => Some(model.as_str()),
        Shown::Staging { model } => model.as_deref(),
        Shown::Empty => None,
    };
    let devices = match (source.as_deref(), shown_model) {
        (Some(source), Some(model)) => cards.offers.model(stage, source, model),
        _ => None,
    };
    // The picked device belongs to the shown model only when it is the pick.
    let device = (shown_model.is_some() && shown_model == picked)
        .then(|| cards.picks.device(stage))
        .flatten();
    let build = cards.build_downloading(stage);
    // Not while a switch or a build is under way: the model is still on the
    // old framework because it hasn't moved yet.
    let fallback = match &shown {
        Shown::Running { idle: false, .. }
            if cards.operations.is_ready(stage) && build.is_none() =>
        {
            view.and_then(StageState::running_device)
                .and_then(|running| crate::rules::load::fallback(device, running))
                .map(ToString::to_string)
        }
        _ => None,
    };
    let staging = matches!(shown, Shown::Staging { .. });
    let memory = backend
        .zip(shown_model.filter(|_| staging))
        .and_then(|(b, model)| b.models.iter().find(|m| m.name == model))
        .map(|m| m.estimated_vram_bytes)
        .filter(|bytes| *bytes > 0);
    let online = backend
        .zip(shown_model)
        .is_some_and(|(b, model)| crate::rules::backends::model_is_online(b, model));
    let no_device = staging && !online && devices.is_some_and(<[String]>::is_empty);
    let can_load = staging
        && unmet.is_empty()
        && shown_model.is_some()
        && (device.is_some() || online)
        && ready(cards, stage);
    let vram_shortfall = backend.filter(|_| staging && !no_device).and_then(|b| {
        crate::rules::memory::staged_vram_shortfall_in(
            stage,
            cards.operations.get(stage),
            &cards.picks,
            b,
            &cards.gpus,
        )
    });
    Card {
        stage,
        backend,
        source,
        unmet,
        models: backend.map_or_else(Vec::new, |b| cards.catalog.models(stage, &b.source)),
        shown,
        devices,
        device,
        fallback,
        memory,
        can_load,
        no_device,
        vram_shortfall,
        operation: cards.operations.get(stage),
        busy: cards.busy.contains(&stage),
        build,
    }
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

pub async fn run_secrets(source: String) -> Input {
    let items = client::list_backend_secrets(source.clone())
        .await
        .unwrap_or_else(|e| {
            log::warn!("Could not read which of {source}'s secrets are stored: {e}");
            Vec::new()
        });
    Input::SecretsLoaded { source, items }
}

/// What `source`, `stage`'s backend, runs its models on: the union over the
/// models it serves there.
pub async fn run_backend_devices(stage: Stage, source: String) -> Input {
    Input::BackendDevicesLoaded {
        stage,
        result: client::list_stage_devices(stage)
            .await
            .map_err(|e| e.to_string()),
        source,
    }
}

/// Install `source`'s build for `framework`, as the Library installs: its
/// progress and its end are the Library's to follow.
pub async fn run_install_build(source: String, framework: String) -> crate::pages::library::Input {
    use crate::pages::library::Input;
    match crate::daemon::registry::install_by_source(&source, Some(&framework)).await {
        Ok(accepted) => Input::InstallAccepted {
            source,
            install_id: accepted.install_id,
        },
        Err(e) => Input::InstallFailedToStart {
            source,
            error: e.to_string(),
        },
    }
}

pub async fn run_gpus() -> Input {
    Input::GpusLoaded(client::get_gpu_info().await.unwrap_or_default())
}

/// What `model` can run on, and the device it has: one read for both, so the
/// picker and the device it starts on land together.
pub async fn run_devices(stage: Stage, source: String, model: String) -> Input {
    let result = async {
        let devices = client::list_model_devices(stage, model.clone()).await?;
        // The recorded device is the nicety: a model never loaded has none.
        let current = client::get_model_device(stage, model.clone())
            .await
            .ok()
            .map(|d| d.device);
        Ok::<_, super_voice_shared::daemon::http_client::HttpError>((devices, current))
    }
    .await
    .map_err(|e| e.to_string());
    Input::DevicesLoaded {
        stage,
        source,
        model,
        result,
    }
}

pub async fn run_poll(stage: Stage) -> Input {
    Input::Polled(
        stage,
        client::get_download_status(stage).await.ok().flatten(),
    )
}

/// Run `effect`, a change: the stage is read again when it is done.
pub async fn run_change(effect: Effect) -> Option<Input> {
    let (stage, change, result) = match effect {
        Effect::SetBackend(stage, Some(source)) => (
            stage,
            Change::Backend,
            client::set_stage_backend(stage, source).await,
        ),
        Effect::SetBackend(stage, None) => (
            stage,
            Change::Backend,
            client::clear_stage_backend(stage).await,
        ),
        Effect::Load {
            stage,
            model,
            source,
            device,
        } => {
            let result = async {
                // The device is the model's own, set before the load so the
                // load picks it up.
                if let Some(device) = device {
                    client::set_model_device(stage, model.clone(), device).await?;
                }
                client::set_stage_model(stage, model, Some(source)).await
            }
            .await;
            (stage, Change::Load, result)
        }
        Effect::SetDevice {
            stage,
            model,
            device,
            running,
        } => (
            stage,
            if running {
                Change::Load
            } else {
                Change::Device
            },
            client::set_model_device(stage, model, device).await,
        ),
        Effect::ReloadModel(stage) => {
            (stage, Change::Load, client::reload_stage_model(stage).await)
        }
        Effect::Unload(stage) => (
            stage,
            Change::Unload,
            client::unload_stage_model(stage).await,
        ),
        Effect::CancelDownload(stage) => {
            (stage, Change::Cancel, client::cancel_download(stage).await)
        }
        Effect::ReadView(_)
        | Effect::ReadBackends(_)
        | Effect::ReadSecrets(_)
        | Effect::ReadGpus
        | Effect::ReadBackendDevices { .. }
        | Effect::ReadDevices { .. }
        | Effect::Poll(_)
        | Effect::Focus { .. }
        | Effect::VoicesChanged
        | Effect::OfferBuild { .. }
        | Effect::InstallBuild { .. } => return None,
    };
    Some(Input::Done(
        stage,
        change,
        result.map_err(|e| e.to_string()),
    ))
}

#[cfg(test)]
mod tests;
