// SPDX-License-Identifier: GPL-3.0-only

//! Message types for the Super Voice application.
//!
//! The top-level [`Message`] groups its variants into per-area sub-enums (one
//! per `handle_*_messages` handler). Dispatch (`core/app/update.rs`) is then an
//! exhaustive `match` that hands each sub-enum to its handler, and each handler
//! `match`es its sub-enum exhaustively — so a newly added variant is a compile
//! error at both ends instead of silently falling through to `Task::none()`.

use super_voice_shared::models::assistants::{AssistantId, AssistantList, NewKey};
use super_voice_shared::models::notification_method::NotificationMethod;
use super_voice_shared::models::read_method::ReadMethod;
use super_voice_shared::models::shortcuts::{ShortcutId, ShortcutList};

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
    NotificationMethod(NotificationMethodMessage),
    IdleUnload(IdleUnloadMessage),
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

    /// A scoped settings/backend save failed. Stored in `AppModel::action_error`
    /// and rendered as an inline banner on the page named by `scope`, instead of
    /// hijacking the UI (Tier 1 #13) or being dropped to the log (Tier 1 #15).
    /// Handled inline in `dispatch` (no dedicated handler).
    SettingActionFailed {
        scope: crate::state::ErrorScope,
        message: String,
    },
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
    /// A window finished opening, so its `NSWindow` can be given the native
    /// frame. See `core::app::macos`.
    #[cfg(target_os = "macos")]
    WindowOpened(cosmic::iced::window::Id),
    /// Where the native title bar put the traffic lights, so the toolbar can
    /// line up with them.
    #[cfg(target_os = "macos")]
    TitleBarMeasured(crate::core::app::macos::TitleBar),
    /// A window was resized, perhaps into or out of full screen.
    #[cfg(target_os = "macos")]
    WindowResized(cosmic::iced::window::Id),
    /// Whether the window is in full screen, checked after a resize.
    #[cfg(target_os = "macos")]
    FullScreenChecked(bool),
    /// The system's look changed: light or dark, or the accent color.
    #[cfg(target_os = "macos")]
    SystemLook(crate::core::app::macos::appearance::Look),
    /// The title bar was pressed: move the window.
    #[cfg(target_os = "macos")]
    DragWindow,
    /// The title bar was double-clicked: zoom the window.
    #[cfg(target_os = "macos")]
    ZoomWindow,
    /// The toolbar's sidebar button, standing in for libcosmic's header toggle.
    #[cfg(target_os = "macos")]
    ToggleSidebar,
    /// Where the bundle's daemon agent stands, looked up while the daemon is
    /// unreachable. See `core::app::macos::bundle`.
    #[cfg(target_os = "macos")]
    DaemonAgent(super_voice_shared::launch_agents::Status),
    /// Register the bundle's agents, for a daemon agent that is not.
    #[cfg(target_os = "macos")]
    RegisterAgents,
    /// Open Login Items, where a daemon agent the user switched off goes back on.
    #[cfg(target_os = "macos")]
    OpenLoginItems,
}

/// Daemon connection, connection-time settings loads, and the SSE event stream.
#[derive(Debug, Clone)]
pub enum DaemonMessage {
    DaemonConnectionResult(super_voice_shared::daemon::http_client::HttpResult<()>),
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
    // Registry / Download-tab messages
    /// User clicked Install on a Download-tab card.
    InstallBackend(String),
    /// Daemon accepted the install request.
    InstallAccepted { source: String, install_id: String },
    /// Install POST failed (couldn't start).
    InstallFailedToStart { source: String, error: String },
    /// SSE: registry.install.progress
    InstallProgress {
        install_id: String,
        source: String,
        phase: super_voice_shared::registry::events::InstallPhase,
        bytes_done: Option<u64>,
        bytes_total: Option<u64>,
    },
    /// SSE: registry.install.completed
    InstallCompleted { source: String },
    /// SSE: registry.install.failed
    InstallFailed {
        install_id: String,
        source: String,
        phase: super_voice_shared::registry::events::InstallPhase,
        error: super_voice_shared::registry::events::InstallError,
        /// The daemon's reason in words, when it has one.
        reason: Option<String>,
    },
    /// User clicked Update on an Installed-tab card.
    UpdateBackend(String),
    /// User chose Uninstall in a card's menu: ask before doing it.
    AskUninstall(String),
    /// User answered the uninstall question with Cancel.
    CancelUninstall,
    /// User confirmed the uninstall.
    UninstallBackend(String),
    /// An uninstall request failed; carries the backend `source` and a
    /// human-readable error to surface on the installed card.
    UninstallFailed { source: String, error: String },
    /// User clicked Retry on the Download-tab empty state, or any other refresh trigger.
    RefreshRegistry,
    /// Initial fetch of /registry/backend/list succeeded.
    RegistryListLoaded(super_voice_shared::registry::RegistryListResponse),
    /// Initial fetch of /registry/backend/list failed.
    RegistryListFailed(String),
    /// The catalog listed, but the refresh asked for before it failed: the
    /// list is the copy the daemon saved, and may be out of date.
    RegistryListStale(super_voice_shared::registry::RegistryListResponse, String),
    /// User typed in the search box.
    RegistrySearchChanged(String),
    /// User toggled "show incompatible".
    RegistryIncludeIncompatible(bool),
    /// User chose an online filter.
    RegistryOnlineFilter(Option<bool>),
    /// User chose which feature a Browse entry must serve.
    RegistryFeatureFilter(crate::state::registry::FeatureFilter),
    /// User chose which kind of model a Browse entry must serve.
    RegistryRoleFilter(crate::state::registry::RoleFilter),
    /// User chose an online filter for the Installed tab.
    InstalledOnlineFilter(Option<bool>),
    /// User chose which feature an installed backend must serve.
    InstalledFeatureFilter(crate::state::registry::FeatureFilter),
    /// User chose which kind of model an installed backend must serve.
    InstalledRoleFilter(crate::state::registry::RoleFilter),
    /// Toggle the per-row overflow ("⋯") menu on an installed-backend card,
    /// keyed by backend `source`. Opening one closes any other.
    ToggleInstalledMenu(String),
    /// Dismiss any open installed-backend overflow menu (click-outside).
    CloseInstalledMenu,
    /// User clicked "+ Import from dir" on the Download tab. Opens an async
    /// folder picker; if the user picks one, the path comes back as
    /// [`ModelsPageMessage::ImportBackendFromDirPicked`].
    ImportBackendFromDir,
    /// Async folder picker resolved. `None` means the user cancelled — no-op.
    ImportBackendFromDirPicked(Option<String>),
    /// User typed in the Custom-repo URL field in the Download tab.
    RegistryCustomRepoInputChanged(String),
    /// Toggle the Browse toolbar's overflow ("⋯") menu.
    ToggleBrowseMenu,
    /// Dismiss the Browse toolbar's overflow menu (click-outside).
    CloseBrowseMenu,
    /// User picked an install source in the Add-a-backend drawer. Switching
    /// clears the preview: it described the other source.
    AddSourceChanged(crate::state::registry::AddSource),
    /// User typed in the drawer's folder-path field.
    AddFolderInputChanged(String),
    /// User pressed Check in the drawer, or submitted the URL field. Resolves
    /// the source without installing it.
    CheckAddSource,
    /// A preview resolved. Carries the backend the source would install.
    AddPreviewLoaded(Box<super_voice_shared::registry::PreviewResponse>),
    /// A preview failed. Carries the daemon's message, shown verbatim.
    AddPreviewFailed(String),
    /// One frame of the drawer's "reading" spinner. Only fires while a check
    /// is in flight; the subscription that emits it is gated on that.
    AddPreviewSpin,
    /// User pressed Install in the drawer, committing to the previewed backend.
    InstallPreviewed,
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
    /// Resolution block for `(source, model)`, from
    /// `GET /tts/pipeline/{stage}/model/{model}/voice`.
    ModelVoiceLoaded {
        source: String,
        model: String,
        block: crate::state::VoiceResolution,
    },
    /// The voices `(source, model)` can be pinned to.
    ///
    /// A separate message from the block for the same reason the language pair
    /// is split: choosing a voice rewrites the block and leaves this list as it
    /// was, so folding the two would blank the picker on every pick.
    ModelVoicesListed {
        source: String,
        model: String,
        voices: Vec<crate::state::VoiceChoice>,
    },
    /// The user picked a voice for the shown model, or cleared it back to the
    /// model's own default (`None`).
    ModelVoiceSelected(Option<String>),
    /// A voice request failed; carries what to show.
    VoiceError(String),
}

/// The Shortcuts page.
#[derive(Debug, Clone)]
pub enum ShortcutsMessage {
    /// Ask the daemon where each shortcut stands.
    Refresh,
    /// The daemon's answer to a refresh.
    Loaded(Result<ShortcutList, String>),
    /// Add this shortcut.
    Add(ShortcutId),
    /// Remove this shortcut.
    Remove(ShortcutId),
    /// The daemon's answer to an add or a remove: the list, or why not.
    Changed(Result<ShortcutList, String>),
}

/// The Assistants page, and its two dialogs: Connect, and a key for another
/// assistant.
#[derive(Debug, Clone)]
pub enum AssistantsMessage {
    /// Ask the daemon where each assistant stands.
    Refresh,
    /// The daemon's answer to a refresh.
    Loaded(Result<AssistantList, String>),
    /// The daemon's answer to a change: the list, or what to show.
    Changed(Result<AssistantList, String>),
    /// Open the Connect dialog for this assistant.
    AskConnect(AssistantId),
    /// Open a connected assistant's own page: its switches and sessions.
    Open(AssistantId),
    /// Back from an assistant's page to the list.
    Close,
    /// The Connect dialog's Cancel.
    CancelConnect,
    /// The Connect dialog's Connect.
    Connect,
    /// The daemon's answer to a connect: the list, or why not.
    Connected(AssistantId, Result<AssistantList, String>),
    /// Disconnect this assistant.
    Disconnect(AssistantId),
    /// The daemon's answer to a disconnect: the list, or why not.
    Disconnected(AssistantId, Result<AssistantList, String>),
    /// Whether Super Voice says aloud when this assistant needs the user.
    SetAnnounceWhenNeeded(AssistantId, bool),
    /// Whether this assistant's `listen` calls are heard with the context
    /// they bring from the conversation.
    SetConversationContext(AssistantId, bool),
    /// Whether this assistant's plugin shows Talk above its prompt.
    SetShowTalk(AssistantId, bool),
    /// Whether this assistant's plugin sends a prompt dictated into it by
    /// itself, after a countdown.
    SetAutosendDictatedPrompts(AssistantId, bool),
    /// Open the dialog that makes a key for another assistant.
    AskKey,
    /// The key's name, as typed.
    KeyNameChanged(String),
    /// The key dialog's Create.
    CreateKey,
    /// The daemon's answer to Create: the key, or why not.
    KeyCreated(Result<NewKey, String>),
    /// The key dialog's Cancel, or Done once the key is shown.
    CloseKey,
    /// Revoke the key made by hand with this id.
    RemoveKey(String),
}

/// The Read aloud tab: where the shortcut reads from, and its test.
#[derive(Debug, Clone)]
pub enum ReadMethodMessage {
    Changed(ReadMethod),
    Loaded(ReadMethod),
    Error(String),
    /// Read what the setting names now and speak it, as the shortcut does.
    Test,
    /// The daemon could not read or speak it; carries its message.
    TestFailed(String),
}

/// Notification-method setting.
#[derive(Debug, Clone)]
pub enum NotificationMethodMessage {
    Changed(NotificationMethod),
    Loaded(NotificationMethod),
    Error(String),
}

/// Idle-unload setting: seconds an unused model stays loaded, `0` for always.
#[derive(Debug, Clone)]
pub enum IdleUnloadMessage {
    Changed(u64),
    Loaded(u64),
    Error(String),
}

/// Backend catalog + per-backend secret/option configuration.
/// Secrets are managed via the daemon's secrets endpoints; options go to the
/// daemon config via the client.
#[derive(Debug, Clone)]
pub enum BackendMessage {
    BackendsLoaded(Vec<BackendInfo>),
    /// Re-fetch the backend catalog (e.g. after a secret/option save) so the
    /// UI reflects the new effective values.
    BackendsReload,
    BackendsError(String),
    /// Daemon-sourced configured flags for a backend's secrets, received after
    /// `BackendsLoaded`. Folds `(name, configured)` into `backend_secret_configured`.
    BackendSecretsConfigured {
        source: String,
        items: Vec<(String, bool)>,
    },
    BackendSecretInputChanged {
        source: String,
        name: String,
        value: String,
    },
    BackendSecretSaved {
        source: String,
        name: String,
    },
    /// Daemon confirmed that a backend secret was written successfully.
    /// Triggers input-buffer clearance and a catalog reload.
    BackendSecretStored {
        source: String,
        name: String,
    },
    BackendSecretRemoved {
        source: String,
        name: String,
    },
    BackendOptionInputChanged {
        source: String,
        name: String,
        value: String,
    },
    BackendOptionSaved {
        source: String,
        name: String,
    },
    /// A `type = "bool"` option's switch was flipped. A switch has no Save
    /// button to press, so this both records the new value and writes it.
    BackendOptionToggled {
        source: String,
        name: String,
        value: bool,
    },
    /// A value was picked from an option's dropdown. Like the switch, a
    /// dropdown has nothing to press afterwards, so choosing writes.
    BackendOptionChosen {
        source: String,
        name: String,
        value: String,
    },
    BackendOptionReset {
        source: String,
        name: String,
    },
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
    /// Global Primary Language loaded from the daemon (None = unset).
    PrimaryLanguageLoaded(Option<String>),
    /// The tags the global setting accepts, from `GET /settings/language/list`.
    PrimaryLanguagesLoaded(Vec<String>),
    /// User picked a global language (None = clear → DELETE).
    PrimaryLanguageSelected(Option<String>),
    /// Per-model resolution block (`/stt/pipeline/{stage}/model/{model}/language`)
    /// loaded from the daemon for `(source, model)`.
    /// The languages a model can be pinned to, from
    /// `GET /stt/pipeline/{stage}/model/{model}/language/list`.
    ///
    /// Separate from the resolution block: the block changes with every pick,
    /// the offer cannot change for a given model, so a pick does not re-fetch
    /// a list that has not moved.
    ModelLanguagesLoaded {
        stage: Stage,
        source: String,
        model: String,
        languages: Vec<String>,
    },
    ModelLanguageLoaded {
        /// The stage the block was read through.
        stage: Stage,
        source: String,
        model: String,
        block: crate::state::LanguageResolution,
    },
    /// User picked a per-model override.
    /// `choice = None` → Follow global (DELETE override);
    /// `choice = Some("auto")` → Auto-detect;
    /// `choice = Some(tag)` → explicit BCP-47 tag.
    ModelLanguageSelected {
        /// The stage the model is addressed through: it resolves the bare
        /// model name against the backend filling it.
        stage: Stage,
        source: String,
        model: String,
        choice: Option<String>,
    },
    LanguageError(String),
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

/// How a beta-opt-in toggle ended. Three cases because each leaves the UI
/// somewhere different: only a failed *write* may snap the toggle back — a
/// failed re-check means the setting did change and only the candidate
/// version is unknown, so reverting the switch would misreport the daemon.
#[derive(Debug, Clone)]
pub enum BetaOptinOutcome {
    /// Saved, and the re-check returned a fresh status (`None` = the daemon
    /// predates `/v1/update`).
    Applied(Option<super_voice_shared::models::self_update::SelfUpdateStatus>),
    /// The setting write itself failed; nothing changed daemon-side.
    WriteFailed(String),
    /// Saved, but the follow-up re-check failed. The toggle stands; only the
    /// banner reports it.
    CheckFailed(String),
}

/// Self-update: status load/check, the two settings toggles, the apply-flow
/// run (installer download + spawn + JSON progress stream), and the
/// `UpdateAvailable` SSE-driven refetch.
#[derive(Debug, Clone)]
pub enum UpdateMessage {
    StatusLoaded(Option<super_voice_shared::models::self_update::SelfUpdateStatus>),
    StatusError(String),
    CheckNow,
    AutoCheckLoaded(bool),
    AutoCheckToggled(bool),
    BetaOptinToggled(bool),
    /// The beta-opt-in write and its follow-up re-check settled. Ends the
    /// lock the toggle took when it was pressed; `enabled` is the value that
    /// was requested, so a failed write knows what to snap back from.
    BetaOptinApplied {
        enabled: bool,
        outcome: BetaOptinOutcome,
    },
    /// Open the Updates page (from the header bar's "Update available"
    /// badge). Routed through the nav model so the page's status refetch
    /// happens exactly as it does when the sidebar entry is clicked.
    OpenUpdatesPage,
    /// A settings-toggle write (`AutoCheckToggled`/`BetaOptinToggled`)
    /// failed. Distinct from `StatusError` (a fetch/check failure) so the
    /// banner names the right verb ("update a setting" vs. "fetch status").
    SettingError(String),
    StartUpdate,
    CancelUpdate,
    RunEvent(crate::core::app::updater::UpdateRunEvent),
    RestartApp,
    AvailableEventReceived,
    /// User dismissed a finished (`Done`/`Failed`) run's panel without
    /// restarting — clears it so the page returns to the idle CTA (or a
    /// future `StartUpdate` is no longer blocked). No-op on an in-flight run;
    /// that goes through `CancelUpdate` instead.
    DismissRun,
}

/// The Contexts tab, and which context each backend uses (set from the
/// Models page's Configure sheet).
#[derive(Debug, Clone)]
pub enum ContextsMessage {
    /// Something the page's logic handles: a press, or the answer to work it
    /// started. See `super_voice_app_core::pages::contexts`.
    Page(super_voice_app_core::pages::contexts::Input),
    /// A keystroke in the editor's prompt. Carries the widget's own action,
    /// since the editor owns the cursor; the app applies it to its buffer.
    PromptAction(cosmic::widget::text_editor::Action),

    /// Point one backend at a context of its own, or at none. `None` means "no
    /// context at all"; going back to following the active one is
    /// [`Self::BackendFollowsActive`].
    BackendPinned { source: String, id: Option<String> },
    /// Put one backend back on the active context.
    BackendFollowsActive(String),
    /// What a backend uses now, as the daemon reports it.
    BackendContextLoaded {
        source: String,
        state: crate::daemon::client::BackendContext,
    },
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
    NotificationMethod => NotificationMethodMessage,
    IdleUnload => IdleUnloadMessage,
    Backend => BackendMessage,
    Contexts => ContextsMessage,
    Notes => NotesMessage,
    Clips => ClipsMessage,
    Language => LanguageMessage,
    Recording => RecordingMessage,
    Update => UpdateMessage,
}
