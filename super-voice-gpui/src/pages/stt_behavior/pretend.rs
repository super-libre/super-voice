// SPDX-License-Identifier: GPL-3.0-only
//! The Behavior page's made-up state, for `just app-shot`: silence or Stop
//! ends a recording, Auto types (a test came to Built-in), the Gentle sounds
//! at 70 %, and the last test's words. Scenes: `testing`, the recording test
//! running and hearing speech; `countdown`, the typing test counting down;
//! `silent`, the sounds off.

use gpui_kit::{Context, Window};
use super_voice_app_core::pages::behavior::Input;
use super_voice_app_core::state::recording_test::Heard;
use super_voice_app_core::state::{AudioTheme, Microphone};
use super_voice_shared::models::protocol::PreviewSource;
use super_voice_shared::models::recording_stop_mode::RecordingStopMode;
use super_voice_shared::models::write_method::WriteMethod;

use super::BehaviorPage;
use crate::effects::CorePage as _;
use crate::ui::waveform::Levels;

pub(super) fn fill(
    behavior: &mut BehaviorPage,
    scenes: &[&str],
    window: &mut Window,
    cx: &mut Context<BehaviorPage>,
) {
    let themes = vec![
        AudioTheme::Classic,
        AudioTheme::Gentle,
        AudioTheme::Minimal,
        AudioTheme::SciFi,
        AudioTheme::Musical,
        AudioTheme::Nature,
        AudioTheme::Retro,
        AudioTheme::Silent,
    ];
    let theme = if scenes.contains(&"silent") {
        AudioTheme::Silent
    } else {
        AudioTheme::Gentle
    };
    for input in [
        Input::ThemesLoaded(themes),
        Input::ThemeLoaded(AudioTheme::Gentle),
        Input::ThemeLoaded(theme),
        Input::StopModeLoaded(RecordingStopMode::SilenceAndManual),
        Input::PreviewTypingLoaded(false),
        Input::WriteMethodLoaded(WriteMethod::Auto),
        Input::TypingTested(Ok(Some(WriteMethod::BuiltIn))),
        Input::VolumeLoaded(70),
    ] {
        behavior.send(input, window, cx);
    }
    if scenes.contains(&"countdown") {
        behavior.state.typing_countdown = Some(2);
    }
    if scenes.contains(&"testing") {
        behavior.microphone = Microphone {
            recording: true,
            level: 0.6,
            speech: true,
        };
        behavior.test.preview = "Testing one two three, this is how my voice sounds".into();
        behavior.test.preview_source = Some(PreviewSource::Stream);
        behavior.levels = Levels::from_iter((0..120).map(|i: u16| {
            let i = f32::from(i);
            if i > 114. {
                0.
            } else {
                (0.3 + 0.7 * (i / 5.5).sin().abs()) * 0.25f32.mul_add((i * 5.1).sin(), 0.75)
            }
        }));
    } else {
        behavior.test.heard = Some(Heard::Words(
            "Pick up milk, eggs and the good bread on the way home.".into(),
        ));
    }
    // The tests are at the bottom of the page: show them.
    if scenes.contains(&"testing") || scenes.contains(&"countdown") {
        behavior
            .scroll
            .set_offset(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(-10_000.)));
    }
    cx.notify();
}
