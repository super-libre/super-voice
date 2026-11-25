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
use crate::state::Stage;

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    Shell(ShellMessage),
    Daemon(DaemonMessage),
    /// The Models page's stage cards: which backend and model fills each
    /// stage, and loading it.
    Models(ModelsMessage),
    ModelsPage(ModelsPageMessage),
    Voice(VoiceMessage),
    Speech(SpeechMessage),
    Voices(VoicesMessage),
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

/// The Models page's stage cards.
#[derive(Debug, Clone)]
pub enum ModelsMessage {
    /// Something the cards' logic handles: a press, what the daemon said, or
    /// the answer to work it started. See `super_voice_app_core::pages::models`.
    Page(super_voice_app_core::pages::models::Input),
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
    Models => ModelsMessage,
    ModelsPage => ModelsPageMessage,
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
