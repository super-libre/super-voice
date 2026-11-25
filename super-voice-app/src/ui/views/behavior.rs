// SPDX-License-Identifier: GPL-3.0-only
//! Each feature's Behavior tab, the last of its tabs: every setting of the
//! feature, then one Test section with every test of it.
//!
//! The settings were spread over a tab each (speech to text's Recording and
//! Input Simulation, text to speech's Speech and Read aloud), each with a test
//! of its own. The sections are still drawn by those modules; this puts them
//! together.

use cosmic::Element;
use cosmic::widget::settings;

use super::common::error_banner;
use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::ui::messages::Message;

/// One Test section from the rows of every test on the tab.
fn test_section<'a>(rows: impl IntoIterator<Item = Element<'a, Message>>) -> Element<'a, Message> {
    rows.into_iter()
        .fold(
            settings::section().title(crate::fl!("test")),
            settings::Section::add,
        )
        .into()
}

/// The failures the tab's settings and tests raised, each on its own banner.
fn banners<'a>(app: &'a AppModel, scopes: &[ErrorScope]) -> Vec<Element<'a, Message>> {
    scopes
        .iter()
        .filter_map(|scope| app.action_error_for(*scope))
        .map(error_banner)
        .collect()
}

/// Speech to text's Behavior tab: Recording, Typing and Sounds, then one Test
/// section that records and types.
pub fn speech_to_text(app: &AppModel) -> Element<'_, Message> {
    let mut blocks = banners(app, &[ErrorScope::Recording, ErrorScope::InputSimulation]);
    let behavior = &app.behavior;
    blocks.push(super::recording::settings_section(
        behavior.stop_mode,
        behavior.preview_typing,
    ));
    blocks.push(super::input_simulation::typing_section(
        behavior.write_method,
        behavior.typed_with,
    ));
    blocks.push(super::recording::sounds_section(
        &behavior.audio_themes,
        &behavior.audio_theme,
        behavior.volume,
    ));
    let mut rows = super::recording::test_rows(
        &app.recording_status,
        &app.transcription_text,
        &app.preview_text,
        app.preview_source,
        app.audio_level,
        app.is_speech_detected,
    );
    rows.extend(super::input_simulation::test_rows(
        &behavior.typing_test_text,
        behavior.typing_countdown,
    ));
    blocks.push(test_section(rows));
    settings::view_column(blocks).into()
}

/// Text to speech's Behavior tab: Read aloud, where the session has it, then
/// one Test section that speaks typed text and reads aloud.
pub fn text_to_speech(app: &AppModel) -> Element<'_, Message> {
    let mut blocks = banners(app, &[ErrorScope::Speech, ErrorScope::ReadAloud]);
    // Reading aloud reads the Wayland session, so only Linux has it.
    let reads_aloud = cfg!(target_os = "linux");
    if reads_aloud {
        blocks.push(super::read_aloud::settings_section(app.read_method));
    }
    let mut rows = super::speech::test_rows(
        &app.speech.status,
        &app.speech.test_text,
        app.speech.level,
        app.speech.is_playing,
    );
    if reads_aloud {
        rows.push(super::read_aloud::test_row(
            app.read_method,
            &app.speech.status,
        ));
    }
    blocks.push(test_section(rows));
    settings::view_column(blocks).into()
}
