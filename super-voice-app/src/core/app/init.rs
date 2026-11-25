// SPDX-License-Identifier: GPL-3.0-only

use crate::state::ContextPage;
use crate::ui::icons;
use crate::ui::messages::{Message, ModelMessage};
use cosmic::prelude::*;
use cosmic::widget::nav_bar;

use super::{AppModel, DeviceState};

/// Builds the navigation bar: what both features share, then a section per
/// feature, then the app's own settings, split by dividers. Each feature's
/// pages are tabs of its section.
fn build_nav() -> nav_bar::Model {
    use crate::state::Section;
    let mut nav = nav_bar::Model::default();

    // Models is the primary page (every stage's active backend) — first in the
    // rail and active on launch. Library (manage/install backends) sits
    // directly below.
    nav.insert()
        .text(crate::fl!("models"))
        .data::<Section>(Section::Models)
        .icon(icons::phosphor(icons::BRAIN))
        .activate();

    nav.insert()
        .text(crate::fl!("library"))
        .data::<Section>(Section::Library)
        .icon(icons::phosphor(icons::BOOKS));

    nav.insert()
        .text(crate::fl!("speech-to-text"))
        .data::<Section>(Section::SpeechToText)
        .icon(icons::phosphor(icons::MICROPHONE))
        .divider_above(true);

    nav.insert()
        .text(crate::fl!("text-to-speech"))
        .data::<Section>(Section::TextToSpeech)
        .icon(icons::phosphor(icons::SPEAKER_HIGH));

    // The Shortcuts page: only a Linux desktop takes shortcuts from the
    // daemon. macOS's shortcut is the `voice hotkey` agent's.
    #[cfg(target_os = "linux")]
    nav.insert()
        .text(crate::fl!("shortcuts"))
        .data::<Section>(Section::Shortcuts)
        .icon(icons::phosphor(icons::KEYBOARD))
        .divider_above(true);

    // Shortcuts opens the group on Linux; Assistants does elsewhere.
    nav.insert()
        .text(crate::fl!("assistants"))
        .data::<Section>(Section::Assistants)
        .icon(icons::phosphor(icons::ROBOT))
        .divider_above(!cfg!(target_os = "linux"));

    nav.insert()
        .text(crate::fl!("settings"))
        .data::<Section>(Section::Customization)
        .icon(icons::phosphor(icons::GEAR));

    nav.insert()
        .text(crate::fl!("updates"))
        .data::<Section>(Section::Updates)
        .icon(icons::phosphor(icons::ARROWS_CLOCKWISE));

    nav
}

/// Builds the initial batch of startup tasks (audio themes, daemon ping, data load).
fn initial_load_tasks(
    title_command: Task<cosmic::Action<Message>>,
) -> Task<cosmic::Action<Message>> {
    // Load audio themes on startup (always available)
    let load_themes = Task::perform(
        super_voice_app_core::pages::behavior::run_load_themes(),
        |input| {
            cosmic::Action::App(Message::Behavior(
                crate::ui::messages::BehaviorMessage::Page(input),
            ))
        },
    );

    // Try to ping the daemon on startup
    let initial_ping = crate::core::app::handlers::tasks::ping_task();

    // Load initial data (models + device info) on startup
    let load_initial_data = Task::perform(
        async move {
            // Small delay to let daemon connection establish
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        },
        |()| cosmic::Action::App(Message::Model(ModelMessage::LoadInitialData)),
    );

    Task::batch([title_command, load_themes, initial_ping, load_initial_data])
}

impl AppModel {
    /// Initializes the application with any given flags and startup commands.
    pub(super) fn init_model(
        core: cosmic::Core,
        _flags: crate::Flags,
    ) -> (Self, Task<cosmic::Action<Message>>) {
        let nav = build_nav();

        // Construct the app model with the runtime's core.
        let mut app = AppModel {
            core,
            context_page: ContextPage::default(),
            contexts: crate::state::contexts::ContextsState::default(),
            notes: crate::state::notes::NotesState::default(),
            clips: crate::state::clips::ClipsState::default(),
            nav,
            connection: super_voice_app_core::state::connection::ConnectionState::default(),
            microphone: super_voice_app_core::state::Microphone::default(),
            recording_test: super_voice_app_core::state::recording_test::RecordingTest::default(),
            behavior: crate::state::behavior::BehaviorState::default(),
            udp_restart_counter: 0,
            last_udp_data: std::time::Instant::now(),

            // Initialize model state
            current_model: String::new(),
            current_source: String::new(),
            current_model_epoch: 0,
            transcription_idle: None,
            model_operations: crate::state::model_operations::ModelOperations::opening(crate::fl!(
                "loading-initial-model-state"
            )),

            // Initialize device state
            current_device: String::new(), // Empty until loaded from daemon
            gpu_info: Vec::new(),
            device_state: DeviceState::Ready,
            last_event_timestamp: None,

            // Initialize preview typing state (disabled by default as beta feature)
            // Replaced by the daemon's own state as soon as the settings load
            // completes; the default is the daemon's default too (off).
            post_processor: crate::daemon::client::StageState::default(),
            synthesis: crate::daemon::client::StageState::default(),
            voice: crate::state::VoiceState::default(),
            speech_to_text_tab: crate::state::Page::Notes,
            text_to_speech_tab: crate::state::Page::Clips,
            window_width: 0.0,
            speech: crate::state::SpeechState::default(),
            voices: crate::state::voices::VoicesState::default(),
            staged_picks: crate::state::staged_picks::StagedPicks::default(),
            device_offers: crate::state::device_offers::DeviceOffers::default(),
            stage_catalog: crate::state::stage_catalog::StageCatalog::default(),
            shortcuts: crate::state::shortcuts::ShortcutsState::default(),
            assistants: crate::state::assistants::AssistantsState::default(),
            read_method: super_voice_shared::models::read_method::ReadMethod::default(),
            settings: super_voice_app_core::state::settings::SettingsState::default(),
            library: super_voice_app_core::pages::library::LibraryState::default(),

            // Models page UI state
            models_page: crate::state::models_page::ModelsPageState::default(),

            // Transcription language state
            language: crate::state::language::LanguageState::default(),

            // Backend catalog + per-backend configuration state
            backends: Vec::new(),
            configure: super_voice_app_core::pages::configure::ConfigureState::default(),

            // Registry state
            registry: crate::state::registry::RegistryState::default(),

            // Self-update state
            update: crate::state::update::UpdateState::default(),

            // No pending scoped action error at startup.
            action_error: None,

            scene: super::scene::requested().map(super::scene::Progress::new),

            // Measured once the window opens.
            #[cfg(target_os = "macos")]
            title_bar: super::macos::TitleBar::default(),

            // The look `main` gave the app its theme from.
            #[cfg(target_os = "macos")]
            look: super::macos::appearance::launch_look(),
        };

        // On macOS the native frame and the app's own toolbar replace
        // libcosmic's header bar; see `macos`.
        //
        // libcosmic also draws a window edge — a 1px border, rounded corners,
        // and a transparent backdrop outside them — for compositors that
        // leave that to the client. It skips all three for a maximized
        // window, so the app tells it the window is maximized: the native
        // frame draws the edge. Only Wayland ever resets the flag.
        #[cfg(target_os = "macos")]
        {
            app.core.window.show_headerbar = false;
            app.core.window.is_maximized = true;
        }

        // Opened from `Super Voice.app`, the app is what installs the daemon;
        // see `macos::bundle`.
        #[cfg(target_os = "macos")]
        if super::macos::bundle::in_bundle() {
            super::macos::bundle::request_notification_permission();
            std::thread::spawn(super_voice_app_core::pages::connection::register_agents);
        }

        // Create startup commands
        let title_command = app.update_title();
        let open_scene = app.open_scene();
        (
            app,
            Task::batch([initial_load_tasks(title_command), open_scene]),
        )
    }
}
