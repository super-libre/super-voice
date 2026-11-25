// SPDX-License-Identifier: GPL-3.0-only

//! Message types for the Super Voice application.
//!
//! The top-level [`Message`] groups its variants into per-area sub-enums (one
//! per `handle_*_messages` handler). Dispatch (`core/app/update.rs`) is then an
//! exhaustive `match` that hands each sub-enum to its handler, and each handler
//! `match`es its sub-enum exhaustively — so a newly added variant is a compile
//! error at both ends instead of silently falling through to `Task::none()`.

use cosmic::widget::segmented_button;

use crate::daemon::backends::BackendInfo;
use crate::state::ContextPage;
use crate::state::{BlockStage, Stage};

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    Shell(ShellMessage),
    Daemon(DaemonMessage),
    Model(ModelMessage),
    ModelsPage(ModelsPageMessage),
    /// Filling a pipeline stage, for every stage.
    Stage(StageMessage),
    Voice(VoiceMessage),
    Speech(SpeechMessage),
    Voices(VoicesMessage),
    Device(DeviceMessage),
    Download(DownloadMessage),
    Behavior(BehaviorMessage),
    /// Where the read-aloud shortcut reads from.
    ReadMethod(ReadMethodMessage),
    /// The Shortcuts page.
    Shortcuts(ShortcutsMessage),
    /// The Assistants page, and its two dialogs.
    Assistants(AssistantsMessage),
    /// The Settings page: the language, failure notices and Sleep When Idle.
    Settings(SettingsMessage),
    Backend(BackendMessage),
    /// The Contexts page: the dictation contexts, and which one is in force.
    Contexts(ContextsMessage),
    /// The Notes tab: notes the user speaks, kept by the app.
    Notes(NotesMessage),
    /// The Clips tab: speech the user makes and keeps.
    Clips(ClipsMessage),
    Language(LanguageMessage),
    Recording(RecordingMessage),
    Update(UpdateMessage),
}

/// Template / shell-chrome messages.
#[derive(Debug, Clone)]
pub enum ShellMessage {
    OpenRepositoryUrl,
    ToggleContextPage(ContextPage),
    /// Show one of a feature section's tabs.
    SelectTab(crate::state::Page),
    /// Open the Library's Browse tab, filtered to the backends that can fill
    /// `stage`. From a stage's picker sheet when nothing installed can.
    BrowseFor(crate::state::Stage),
    LaunchUrl(String),
    /// A window's width changed, in logical pixels. Only the main window's is
    /// kept: it decides what the header has room for.
    WindowWidth(cosmic::iced::window::Id, f32),
    /// Put a command on the clipboard. The app does not run these itself —
    /// they act on the user's session, not on the app — so handing over the
    /// exact command is the most it can usefully do.
    CopyText(String),
    /// Time for the running scene to look for its widget again. See
    /// `core::app::scene`.
    SceneStep,
    /// Where the running scene's widget is drawn; `None` when it is not.
    SceneTarget(Option<cosmic::iced::Rectangle>),
    /// The transcription stage as the daemon has it, read for the running
    /// scene, which puts it in the state its shot needs.
    SceneStt(Result<crate::daemon::client::StageState, String>),
}

/// Daemon connection, connection-time settings loads, and the SSE event stream.
#[derive(Debug, Clone)]
pub enum DaemonMessage {
    DaemonConnected,
    /// The `/events` SSE stream finished (re)subscribing. Distinct from
    /// `DaemonConnected` (which the REST ping loop also fires): it marks the
    /// point at which live events start flowing, so the app re-fetches the
    /// current model to capture any state that changed before the stream was
    /// subscribed (e.g. a model that finished loading during a daemon restart
    /// whose one-shot broadcast would otherwise have been missed).
    EventStreamConnected,
    // Settings loaded from daemon at connection time (replaces the
    // legacy bulk fetch_daemon_config). Each is fetched with its own
    // GET endpoint.
    // Carries the typed error so `classify_daemon_error` can decide
    // blocked-vs-retry on the variant, not the wording.
    DaemonError(super_voice_shared::daemon::http_client::HttpError),
    RefreshDaemonStatus,
    RetryConnection,
    /// User denied (or had been denying via deny cache) settings-scope
    /// consent. Halts the auto-retry loop and surfaces a Retry
    /// affordance to the user.
    WidgetBlocked(String),
    /// User pressed the "Retry authorization" button in the Connection
    /// page. Drops the cached settings token and triggers a fresh
    /// consent flow.
    RetryAuthorization,
    /// An input for the Connection page's logic with no message of its own:
    /// the answer to a status check (`RefreshDaemonStatus`), which only shows
    /// the status and tries nothing again.
    Connection(super_voice_app_core::pages::connection::Input),
    PingTimeout,
    DaemonEventsReceived(Vec<super_voice_shared::models::protocol::NotificationEvent>),
}

/// Model identity: startup load, and the current/loaded model.
#[derive(Debug, Clone)]
pub enum ModelMessage {
    LoadInitialData, // Load device info at startup
    CurrentModelLoaded {
        model: String,
        source: String,
        /// The model stage 1 keeps idle, when nothing is loaded: see
        /// `AppModel::transcription_idle`.
        idle: Option<(String, String)>,
        /// `current_model_epoch` captured when this snapshot was requested.
        /// The handler applies the snapshot only if the epoch is unchanged —
        /// otherwise a live `model_switched` superseded it and wins.
        epoch: u64,
    },
    /// Stage 1 was re-read after its model left memory, for whether the
    /// daemon reports it idle. Only that: the loaded model and the card's
    /// operation are the events' to set. Applied under the same epoch as
    /// [`Self::CurrentModelLoaded`].
    IdleRead {
        idle: Option<(String, String)>,
        epoch: u64,
    },
    ModelChanged {
        model: String,
        source: String,
    },
    /// A `fetch_current_model` snapshot query failed. Carries the
    /// `current_model_epoch` captured when the fetch was issued: the handler
    /// clears the loaded model only if the epoch is unchanged. If a live
    /// `model_switched` advanced the epoch since, the failure is stale and is
    /// logged-and-dropped rather than clobbering the fresher state — the same
    /// guard `CurrentModelLoaded` applies to its success path (audit 2 Tier 1 #8).
    CurrentModelFetchFailed {
        epoch: u64,
        error: String,
    },
}

/// Models-page UI: tabs, active-backend card, GPU readout, backend
/// select/config, and the registry / download-tab install lifecycle.
#[derive(Debug, Clone)]
pub enum ModelsPageMessage {
    /// The Library's logic: the catalog and its filters, and installing,
    /// updating and uninstalling. See `super_voice_app_core::pages::library`.
    Library(super_voice_app_core::pages::library::Input),
    /// Activate a Models-page tab (Installed / Download) in the tab bar.
    ModelsTabActivated(segmented_button::Entity),
    /// Open a pipeline step's card on the Models page.
    SelectPipelineStep(crate::state::Stage),
    /// Open the per-backend configuration sub-view for `source`.
    OpenBackendConfig(String),
    /// Leave the configuration sub-view and return to the backend list.
    CloseBackendConfig,
    /// Periodic tick that re-fetches GPU inventory + memory so the header
    /// readout stays live. No-op when disconnected; on success it emits
    /// [`ModelsPageMessage::GpuInfoLoaded`].
    RefreshGpuInfo,
    /// GPU inventory + memory loaded from the daemon (empty when none detected).
    GpuInfoLoaded(Vec<super_voice_shared::models::protocol::GpuInfo>),
    /// Toggle the per-row overflow ("⋯") menu on an installed-backend card,
    /// keyed by backend `source`. Opening one closes any other.
    ToggleInstalledMenu(String),
    /// Dismiss any open installed-backend overflow menu (click-outside).
    CloseInstalledMenu,
    /// The "Install manually" sheet's logic: its source, the preview, and
    /// the install it starts. See `super_voice_app_core::pages::add_backend`.
    AddBackend(super_voice_app_core::pages::add_backend::Input),
    /// Toggle the Browse toolbar's overflow ("⋯") menu.
    ToggleBrowseMenu,
    /// Dismiss the Browse toolbar's overflow menu (click-outside).
    CloseBrowseMenu,
}

/// The stage-1 model's device + device-switch errors.
#[derive(Debug, Clone)]
pub enum DeviceMessage {
    /// The accelerator the loaded model is on, from the startup read of
    /// `GET /stt/pipeline/1`; `None` when nothing is loaded.
    DeviceInfoLoaded(Option<String>),
    DeviceError(String), // Device switching error
}

/// Model-download progress lifecycle.
#[derive(Debug, Clone)]
pub enum DownloadMessage {
    /// A progress snapshot for `stage`, from a `download_progress` event or a
    /// poll.
    DownloadProgressUpdate {
        stage: Stage,
        progress: Box<super_voice_shared::models::protocol::DownloadProgress>,
    },
    /// Abandon the download the named stage has in flight. Every variant below
    /// that settles a stage's card carries its stage for the same reason: the
    /// two stages download independently, so an outcome that did not say whose
    /// it was would settle whichever card happened to be showing.
    CancelDownload(Stage),
    DownloadCompleted(String), // model name
    DownloadCancelled {
        model: String,
        stage: Stage,
    },
    DownloadError {
        model: String,
        error: String,
        stage: Stage,
    },
    /// Ask every stage still owed an outcome for its progress.
    CheckDownloadStatus,
    /// The named stage has no download in flight after all.
    NoDownloadInProgress(Stage),
}

/// Filling a pipeline stage: select a backend, stage a model and a device,
/// load, unload.
///
/// Every variant carries the `stage` it is about, so one handler serves them
/// all. The stages were two hand-maintained copies of this flow before, and the
/// copies drifted — the newer one lost the re-entrancy guard, the stale-catalog
/// check, the device rollback, and the check that a device was staged at all.
///
/// `stage` values come from `state::device_offers::{Stage::Transcription, Stage::PostProcessing}`,
/// which are the daemon's own pipeline positions.
#[derive(Debug, Clone)]
pub enum StageMessage {
    /// Select the backend filling this stage, without loading a model. Any
    /// model from a different backend is unloaded.
    SelectBackend { stage: Stage, source: String },
    /// Empty the stage, forgetting its model with it.
    DeselectBackend { stage: Stage },
    /// The daemon took the selection (`None` = the stage is empty). Re-announced
    /// after a successful select, which is what asks for the backend's devices.
    BackendSelected {
        stage: Stage,
        source: Option<String>,
        /// The model the daemon remembers for the stage, running or not.
        ///
        /// `None` for a backend the user has just picked, which has no model
        /// yet. Carried on the startup read because a stage keeps its selection
        /// through an unload, and the card offers that model again rather than
        /// coming up empty.
        model: Option<String>,
    },
    /// A select or deselect was refused: restore `prev` and report it on the
    /// stage's card.
    BackendSelectFailed {
        stage: Stage,
        prev: Option<String>,
        message: String,
    },
    /// User picked a model in the card's dropdown. Staged for the Load button —
    /// does *not* load. The device waits on the daemon, which is asked what the
    /// model can run on here and which device it already has.
    StageModel { stage: Stage, model: String },
    /// User picked a device in the card's device dropdown.
    ///
    /// Staged locally while the model is not up. Once it is running the choice
    /// is sent straight through: the daemon reloads the model onto the new
    /// device in place, so switching device no longer means unload, re-pick,
    /// load.
    StageDevice { stage: Stage, device: String },
    /// The user agreed to download the build for the framework they picked
    /// ([`crate::state::models_page::PendingBuild`]).
    ConfirmBuild,
    /// The user declined it. Nothing changes.
    CancelBuild,
    /// A device change the daemon refused or could not apply.
    ///
    /// Distinct from [`StageMessage::LoadFailed`]: the model is still up, on
    /// the device it had, so this rolls the picker back and reports the failure
    /// without dropping the stage's identity the way a failed load does.
    DeviceChangeFailed {
        stage: Stage,
        prev_device: Option<String>,
        message: String,
    },
    /// The daemon answered what can fill `stage`: the backends, each already
    /// narrowed to the models this stage can run.
    ///
    /// Re-asked whenever the installed catalog changes, since installing or
    /// uninstalling a backend changes the answer — and never on selection,
    /// because the answer does not depend on what is selected.
    StageBackendsLoaded {
        stage: Stage,
        backends: Vec<BackendInfo>,
    },
    /// The daemon answered for the staged `model` of `source`: `devices` is what
    /// it can be loaded onto here, `current` the device it already has. `source`
    /// is the backend selected when the question was asked, so an answer a
    /// switch has overtaken can be dropped.
    StagedDevicesLoaded {
        stage: Stage,
        source: String,
        model: String,
        devices: Vec<String>,
        current: Option<String>,
    },
    /// The daemon answered which devices this stage's backend can run its
    /// models on — the union over the models it serves in this stage's role.
    BackendDevicesLoaded {
        stage: Stage,
        source: String,
        devices: Vec<String>,
    },
    /// User clicked Load. Sets the staged model's device, then runs it. A no-op
    /// when nothing is staged or this stage already has an operation in flight.
    Load { stage: Stage },
    /// A load or unload failed: restore `prev_device` and report it on the
    /// stage's card.
    LoadFailed {
        stage: Stage,
        prev_device: String,
        message: String,
    },
    /// User clicked Unload. Drops the model but keeps the backend selected.
    Unload { stage: Stage },
    /// User clicked Reload: bring the running model back up where it is.
    Reload { stage: Stage },
    /// Re-read `stage`'s block: a write succeeded, so read what the daemon
    /// holds rather than assume the write took effect as sent.
    RefreshBlock { stage: BlockStage },
    /// `stage`'s block as the daemon holds it, from the startup load or after
    /// a change.
    BlockLoaded {
        stage: BlockStage,
        block: crate::daemon::client::StageState,
    },
    /// Re-reading `stage`'s block failed; the message is shown on its card.
    BlockFailed { stage: BlockStage, message: String },
}

/// Text to speech's speech test.
#[derive(Debug, Clone)]
pub enum SpeechMessage {
    /// Something the test's logic handles: a press, what the daemon said, or
    /// the answer to work it started. See `super_voice_app_core::pages::speech`.
    Page(super_voice_app_core::pages::speech::Input),
}

/// The Voices tab: the cloned-voice library, and the recording or import that
/// adds to it.
#[derive(Debug, Clone)]
pub enum VoicesMessage {
    /// Something the page's logic handles: a press, or the answer to work it
    /// started. See `super_voice_app_core::pages::voices`.
    Page(super_voice_app_core::pages::voices::Input),
    /// Hear a voice, in the model text to speech speaks in, which the app
    /// knows.
    Preview(String),
    /// One tick of the level meter and the time while recording.
    RecordingTick,
    /// Close the microphone and keep what it heard. Also how a recording that
    /// stopped by itself (the limit, or a device failure) is collected.
    StopRecording,
}

/// The synthesis model's voice.
#[derive(Debug, Clone)]
pub enum VoiceMessage {
    /// Something the voice's logic handles. See
    /// `super_voice_app_core::pages::model_voice`.
    Page(super_voice_app_core::pages::model_voice::Input),
}

/// The Shortcuts page.
#[derive(Debug, Clone)]
pub enum ShortcutsMessage {
    /// Something the page's logic handles. See
    /// `super_voice_app_core::pages::shortcuts`.
    Page(super_voice_app_core::pages::shortcuts::Input),
}

/// The Assistants page, and its two dialogs: Connect, and a key for another
/// assistant.
#[derive(Debug, Clone)]
pub enum AssistantsMessage {
    /// Something the page's logic handles. See
    /// `super_voice_app_core::pages::assistants`.
    Page(super_voice_app_core::pages::assistants::Input),
}

/// The Read aloud tab: where the shortcut reads from, and its test.
#[derive(Debug, Clone)]
pub enum ReadMethodMessage {
    /// Its logic is the core's (`pages::read_aloud`).
    Page(super_voice_app_core::pages::read_aloud::Input),
}

/// The Settings page.
#[derive(Debug, Clone)]
pub enum SettingsMessage {
    /// Something the page's logic handles. See
    /// `super_voice_app_core::pages::settings`.
    Page(super_voice_app_core::pages::settings::Input),
}

/// The installed-backend list, and a backend's Configure sheet.
#[derive(Debug, Clone)]
pub enum BackendMessage {
    BackendsLoaded(Vec<BackendInfo>),
    /// Re-fetch the backend catalog (e.g. after a secret/option save) so the
    /// UI reflects the new effective values.
    BackendsReload,
    BackendsError(String),
    /// Something the Configure sheet's logic handles: its secrets, options
    /// and context. See `super_voice_app_core::pages::configure`.
    Configure(super_voice_app_core::pages::configure::Input),
}

/// Transcription language (global Primary Language + per-model override).
#[derive(Debug, Clone)]
pub enum LanguageMessage {
    /// Open the language search sheet.
    /// `model = None` → global Primary Language sheet.
    /// `model = Some((source, model))` → per-model sheet for that specific model.
    OpenLanguagePicker {
        /// `(stage, source, model)` for a per-model override, `None` for the
        /// global setting. The stage is what resolves the bare model name.
        model: Option<(Stage, String, String)>,
    },
    CloseLanguagePicker,
    LanguagePickerQueryChanged(String),
    /// Something a model's language logic handles: reading it, what it can
    /// be set to, and a pick. See
    /// `super_voice_app_core::pages::model_language`.
    Model(super_voice_app_core::pages::model_language::Input),
}

/// Speech to text's Behavior tab: its settings and the typing test.
#[derive(Debug, Clone)]
pub enum BehaviorMessage {
    /// Something the tab's logic handles: a press, a value the daemon
    /// reported, or the answer to work it started. See
    /// `super_voice_app_core::pages::behavior`.
    Page(super_voice_app_core::pages::behavior::Input),
}

/// Speech to text's recording test, and the microphone every page that
/// records reads.
#[derive(Debug, Clone)]
pub enum RecordingMessage {
    /// Something the test's logic handles. See
    /// `super_voice_app_core::pages::recording_test`.
    Test(super_voice_app_core::pages::recording_test::Input),
    /// The daemon's microphone opened or closed, or its level moved.
    Microphone(super_voice_app_core::events::Event),
}

/// Self-update: status load/check, the two settings toggles, the apply-flow
/// run (installer download + spawn + JSON progress stream), and the
/// `UpdateAvailable` SSE-driven refetch.
#[derive(Debug, Clone)]
pub enum UpdateMessage {
    /// Something the page's logic handles. See
    /// `super_voice_app_core::pages::updates`.
    Page(super_voice_app_core::pages::updates::Input),
    /// Open the Updates page (from the header bar's "Update available"
    /// badge). Routed through the nav model so the page's status refetch
    /// happens exactly as it does when the sidebar entry is clicked.
    OpenUpdatesPage,
    /// Start the app again after an update.
    RestartApp,
}

/// The Contexts tab. Which context each backend uses is set from its
/// Configure sheet ([`BackendMessage::Configure`]).
#[derive(Debug, Clone)]
pub enum ContextsMessage {
    /// Something the page's logic handles: a press, or the answer to work it
    /// started. See `super_voice_app_core::pages::contexts`.
    Page(super_voice_app_core::pages::contexts::Input),
    /// A keystroke in the editor's prompt. Carries the widget's own action,
    /// since the editor owns the cursor; the app applies it to its buffer.
    PromptAction(cosmic::widget::text_editor::Action),
}

/// The Notes tab: the notes the app keeps in its own folder, and the takes
/// that record them through the daemon.
#[derive(Debug, Clone)]
pub enum NotesMessage {
    /// Something the page's logic handles: a press, or the answer to work it
    /// started. See `super_voice_app_core::pages::notes`.
    Page(super_voice_app_core::pages::notes::Input),
    /// A keystroke, motion or click in the open note's editor. Carries the
    /// widget's own action, since the editor owns the cursor. The app applies
    /// it, and tells the page when it changed the text.
    Edit(cosmic::widget::text_editor::Action),
    /// A second went by while recording, for the elapsed time.
    Tick,
}

/// The Clips tab: speech made from text by the daemon, kept, played and
/// exported by the app.
#[derive(Debug, Clone)]
pub enum ClipsMessage {
    /// Something the page's logic handles: a press, or the answer to work it
    /// started. See `super_voice_app_core::pages::clips`.
    Page(super_voice_app_core::pages::clips::Input),
    /// A keystroke, motion or click in the draft's editor, which the app
    /// applies to its own buffer.
    DraftEdit(cosmic::widget::text_editor::Action),
    /// Make a clip of the draft, in the voice text to speech uses, which the
    /// app names.
    Make,
}

macro_rules! message_from {
    ($($variant:ident => $ty:ident),+ $(,)?) => {
        $(
            impl From<$ty> for Message {
                fn from(m: $ty) -> Self {
                    Message::$variant(m)
                }
            }
        )+
    };
}

message_from! {
    Shell => ShellMessage,
    Voice => VoiceMessage,
    Speech => SpeechMessage,
    Voices => VoicesMessage,
    Daemon => DaemonMessage,
    Model => ModelMessage,
    ModelsPage => ModelsPageMessage,
    Device => DeviceMessage,
    Download => DownloadMessage,
    Behavior => BehaviorMessage,
    ReadMethod => ReadMethodMessage,
    Shortcuts => ShortcutsMessage,
    Assistants => AssistantsMessage,
    Settings => SettingsMessage,
    Backend => BackendMessage,
    Contexts => ContextsMessage,
    Notes => NotesMessage,
    Clips => ClipsMessage,
    Language => LanguageMessage,
    Recording => RecordingMessage,
    Update => UpdateMessage,
}
