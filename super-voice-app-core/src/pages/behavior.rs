// SPDX-License-Identifier: GPL-3.0-only
//! Speech to text's Behavior tab: what stops a recording, whether its words
//! are typed as they are heard, how they are typed (and a test of that), and
//! the sounds around a recording.
//!
//! A setting changes on screen once the daemon has stored it, so a refused
//! save never leaves a value the daemon doesn't have. The sounds are the
//! exception: a theme or a volume shows at once, as the user picks it, and
//! goes back to the stored one if the daemon refuses it.
//!
//! The recording test shares the microphone's state with the Notes page, and
//! is still the app's own.

use std::time::Duration;

use super_voice_shared::models::recording_stop_mode::RecordingStopMode;
use super_voice_shared::models::write_method::WriteMethod;

use crate::daemon::client::v1::settings::audio_theme::{
    get_current_audio_theme, load_audio_themes, set_and_test_audio_theme, set_audio_theme,
};
use crate::daemon::client::{
    get_preview_typing, get_recording_stop_mode, get_volume, get_write_method, set_preview_typing,
    set_recording_stop_mode, set_volume, set_write_method, test_write_method,
};
use crate::state::AudioTheme;
use crate::state::behavior::BehaviorState;

/// How long the delayed typing test waits: long enough to move to the window
/// it should type into, short enough that nobody wonders whether the button
/// worked.
pub const TYPING_COUNTDOWN_SECONDS: u8 = 3;

/// What happened on the tab: something the user did, a value the daemon
/// reported, or the answer to work an [`Effect`] started.
#[derive(Debug, Clone)]
pub enum Input {
    /// Another stop mode was picked.
    StopModeChanged(RecordingStopMode),
    /// The daemon has this stop mode: read, or just stored.
    StopModeLoaded(RecordingStopMode),
    /// The stop mode could not be stored.
    StopModeFailed(String),

    /// Typing as the words are heard was turned on or off.
    PreviewTypingToggled(bool),
    /// The daemon has this: read, or just stored.
    PreviewTypingLoaded(bool),
    /// It could not be stored.
    PreviewTypingFailed(String),

    /// Another write method was picked.
    WriteMethodChanged(WriteMethod),
    /// The daemon has this write method: read, or just stored.
    WriteMethodLoaded(WriteMethod),
    /// It could not be stored.
    WriteMethodFailed(String),

    /// The typing test's field now holds this.
    TypingTestTextChanged(String),
    /// Type the test sentence now, into the test field.
    TestTyping,
    /// Type it after a countdown, into whatever window has focus then.
    TestTypingLater,
    /// A second of the countdown went by.
    TypingTick,
    /// Call the countdown off.
    CancelTypingTest,
    /// The daemon typed the test, and with what, when it said; or why not.
    TypingTested(Result<Option<WriteMethod>, String>),

    /// The sound themes to pick from were read.
    ThemesLoaded(Vec<AudioTheme>),
    /// The daemon has this theme.
    ThemeLoaded(AudioTheme),
    /// The recording sounds were turned on or off.
    SoundsToggled(bool),
    /// Another theme was picked; it plays once.
    ThemePicked(AudioTheme),
    /// A theme could not be stored: go back to these.
    ThemeFailed {
        theme: AudioTheme,
        sounding: AudioTheme,
        message: String,
    },
    /// The daemon has this volume.
    VolumeLoaded(u8),
    /// The volume slider moved.
    VolumeChanged(u8),
    /// The slider was let go: store the volume.
    VolumeCommitted,
    /// The volume could not be stored: go back to this one.
    VolumeFailed { volume: u8, message: String },
}

/// Work [`update`] asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    /// Store the stop mode ([`run_save_stop_mode`]).
    SaveStopMode(RecordingStopMode),
    /// Store whether words are typed as they are heard
    /// ([`run_save_preview_typing`]).
    SavePreviewTyping(bool),
    /// Store the write method ([`run_save_write_method`]).
    SaveWriteMethod(WriteMethod),
    /// Have the daemon type the test sentence ([`run_test_typing`]). When
    /// `into_field`, the app first puts the cursor in its test field: the
    /// daemon types into whatever has focus, and pressing the button leaves
    /// it on the button.
    TestTyping { into_field: bool },
    /// Tell the page when a second has gone by ([`run_tick`]).
    Tick,
    /// Store the theme ([`run_save_theme`]), playing it once when `play`.
    /// `theme` and `sounding` are what to go back to if it is refused.
    SaveTheme {
        chosen: AudioTheme,
        play: bool,
        theme: AudioTheme,
        sounding: AudioTheme,
    },
    /// Store the volume ([`run_save_volume`]); `stored` is what to go back to
    /// if it is refused.
    SaveVolume { volume: u8, stored: u8 },
}

/// Something to tell the user. Each app says it in its own words, and shows
/// it with the section it is about: the recording settings and sounds, or
/// typing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    CouldNotSaveStopMode(String),
    CouldNotSavePreviewTyping(String),
    CouldNotSaveWriteMethod(String),
    TypingTestFailed(String),
    CouldNotSaveSounds(String),
}

/// Which part of the tab a notice is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Recording and its sounds.
    Recording,
    /// Typing, and the typing test.
    Typing,
}

impl Notice {
    /// Which part of the tab it is about.
    #[must_use]
    pub fn section(&self) -> Section {
        match self {
            Self::CouldNotSaveWriteMethod(_) | Self::TypingTestFailed(_) => Section::Typing,
            Self::CouldNotSaveStopMode(_)
            | Self::CouldNotSavePreviewTyping(_)
            | Self::CouldNotSaveSounds(_) => Section::Recording,
        }
    }
}

/// What an [`update`] asks of the app.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The work to start.
    pub effects: Vec<Effect>,
    /// A section whose notice is out of date and goes. Applied before
    /// [`notice`](Self::notice).
    pub clear_notice: Option<Section>,
    /// What to tell the user.
    pub notice: Option<Notice>,
}

/// What `input` does to the tab.
pub fn update(state: &mut BehaviorState, input: Input) -> Outcome {
    let mut out = Outcome::default();
    match input {
        Input::StopModeChanged(mode) => out.effects.push(Effect::SaveStopMode(mode)),
        Input::StopModeLoaded(mode) => {
            state.stop_mode = mode;
            out.clear_notice = Some(Section::Recording);
        }
        Input::StopModeFailed(e) => {
            log::warn!("recording stop mode: {e}");
            out.notice = Some(Notice::CouldNotSaveStopMode(e));
        }
        Input::PreviewTypingToggled(on) => out.effects.push(Effect::SavePreviewTyping(on)),
        Input::PreviewTypingLoaded(on) => {
            state.preview_typing = on;
            out.clear_notice = Some(Section::Recording);
        }
        Input::PreviewTypingFailed(e) => {
            log::warn!("preview typing: {e}");
            out.notice = Some(Notice::CouldNotSavePreviewTyping(e));
        }
        Input::WriteMethodChanged(method) => out.effects.push(Effect::SaveWriteMethod(method)),
        Input::WriteMethodLoaded(method) => {
            state.write_method = method;
            // What the last test came to was for the method before.
            state.typed_with = None;
            out.clear_notice = Some(Section::Typing);
        }
        Input::WriteMethodFailed(e) => {
            log::warn!("write method: {e}");
            state.typing_countdown = None;
            out.notice = Some(Notice::CouldNotSaveWriteMethod(e));
        }
        typing @ (Input::TypingTestTextChanged(_)
        | Input::TestTyping
        | Input::TestTypingLater
        | Input::TypingTick
        | Input::CancelTypingTest
        | Input::TypingTested(_)) => typing_test(state, typing, &mut out),
        sounds => sounds_work(state, sounds, &mut out),
    }
    out
}

/// The typing test.
fn typing_test(state: &mut BehaviorState, input: Input, out: &mut Outcome) {
    match input {
        Input::TypingTestTextChanged(text) => state.typing_test_text = text,
        Input::TestTyping => {
            state.typing_test_text.clear();
            out.clear_notice = Some(Section::Typing);
            out.effects.push(Effect::TestTyping { into_field: true });
        }
        // Not into the field: the user is moving to another window, and the
        // apps that drop typed keys are the ones this page can't hold.
        Input::TestTypingLater => {
            state.typing_test_text.clear();
            out.clear_notice = Some(Section::Typing);
            state.typing_countdown = Some(TYPING_COUNTDOWN_SECONDS);
            out.effects.push(Effect::Tick);
        }
        // A tick after a cancel does nothing: the cancel can't take back the
        // tick already on its way, and that tick would otherwise type into
        // the window the user moved to.
        Input::TypingTick => match state.typing_countdown {
            None => {}
            Some(seconds) => match seconds.saturating_sub(1) {
                0 => {
                    state.typing_countdown = None;
                    out.effects.push(Effect::TestTyping { into_field: false });
                }
                left => {
                    state.typing_countdown = Some(left);
                    out.effects.push(Effect::Tick);
                }
            },
        },
        Input::CancelTypingTest => state.typing_countdown = None,
        Input::TypingTested(Ok(method)) => {
            state.typed_with = method;
            state.typing_countdown = None;
        }
        Input::TypingTested(Err(e)) => {
            log::warn!("typing test: {e}");
            state.typing_countdown = None;
            out.notice = Some(Notice::TypingTestFailed(e));
        }
        _ => {}
    }
}

/// The recording sounds: on or off, which theme, how loud.
fn sounds_work(state: &mut BehaviorState, input: Input, out: &mut Outcome) {
    match input {
        Input::ThemesLoaded(themes) => state.audio_themes = themes,
        Input::ThemeLoaded(theme) => {
            state.audio_theme = theme;
            if theme != AudioTheme::Silent {
                state.sounding_theme = theme;
            }
        }
        // Shown at once; the daemon's refusal puts it back.
        Input::SoundsToggled(on) => {
            out.clear_notice = Some(Section::Recording);
            let (theme, sounding) = (state.audio_theme, state.sounding_theme);
            state.audio_theme = if on {
                state.sounding_theme
            } else {
                AudioTheme::Silent
            };
            out.effects.push(Effect::SaveTheme {
                chosen: state.audio_theme,
                play: false,
                theme,
                sounding,
            });
        }
        Input::ThemePicked(chosen) => {
            out.clear_notice = Some(Section::Recording);
            let (theme, sounding) = (state.audio_theme, state.sounding_theme);
            state.audio_theme = chosen;
            if chosen != AudioTheme::Silent {
                state.sounding_theme = chosen;
            }
            out.effects.push(Effect::SaveTheme {
                chosen,
                play: true,
                theme,
                sounding,
            });
        }
        Input::ThemeFailed {
            theme,
            sounding,
            message,
        } => {
            state.audio_theme = theme;
            state.sounding_theme = sounding;
            out.notice = Some(Notice::CouldNotSaveSounds(message));
        }
        Input::VolumeLoaded(volume) => {
            state.volume = volume;
            state.stored_volume = volume;
        }
        // Each step of a drag shows; the volume is stored once, when the
        // slider is let go.
        Input::VolumeChanged(volume) => state.volume = volume,
        Input::VolumeCommitted => {
            out.clear_notice = Some(Section::Recording);
            let stored = state.stored_volume;
            state.stored_volume = state.volume;
            out.effects.push(Effect::SaveVolume {
                volume: state.volume,
                stored,
            });
        }
        Input::VolumeFailed { volume, message } => {
            state.volume = volume;
            state.stored_volume = volume;
            out.notice = Some(Notice::CouldNotSaveSounds(message));
        }
        _ => {}
    }
}

// The choices the tab offers, and their names, for any app that draws it.

/// What can stop a recording, in the order the tab offers them.
pub const STOP_MODES: [RecordingStopMode; 3] = [
    RecordingStopMode::SilenceAndManual,
    RecordingStopMode::SilenceOnly,
    RecordingStopMode::ManualOnly,
];

/// A stop mode's name, as a message key.
#[must_use]
pub const fn stop_mode_key(mode: RecordingStopMode) -> &'static str {
    match mode {
        RecordingStopMode::SilenceOnly => "stop-silence",
        RecordingStopMode::SilenceAndManual => "stop-silence-manual",
        RecordingStopMode::ManualOnly => "stop-manual",
    }
}

/// The write methods this build can drive, in the order the tab offers
/// them: one the daemon has no backend for here could never work.
#[must_use]
pub fn write_methods() -> Vec<WriteMethod> {
    WriteMethod::supported_on_this_platform()
}

/// A write method's name. ydotool is a program's own name, the same in every
/// language.
#[must_use]
pub fn write_method_name(method: WriteMethod) -> String {
    match method {
        WriteMethod::Auto => crate::i18n::text("auto-recommended", &[]),
        WriteMethod::XdgDesktopPortal => crate::i18n::text("xdg-desktop-portal", &[]),
        WriteMethod::Ydotool => "ydotool".to_string(),
        WriteMethod::BuiltIn => crate::i18n::text("built-in", &[]),
    }
}

/// A sound theme's name, as a message key.
#[must_use]
pub const fn theme_key(theme: AudioTheme) -> &'static str {
    match theme {
        AudioTheme::Classic => "theme-classic",
        AudioTheme::Gentle => "theme-gentle",
        AudioTheme::Minimal => "theme-minimal",
        AudioTheme::SciFi => "theme-scifi",
        AudioTheme::Musical => "theme-musical",
        AudioTheme::Nature => "theme-nature",
        AudioTheme::Retro => "theme-retro",
        AudioTheme::Silent => "theme-silent",
    }
}

/// The themes the Theme picker offers: every one that makes a sound. Silent
/// is the sounds' switch turned off, not a theme to pick.
#[must_use]
pub fn sounding_themes(state: &BehaviorState) -> Vec<AudioTheme> {
    state
        .audio_themes
        .iter()
        .copied()
        .filter(|theme| *theme != AudioTheme::Silent)
        .collect()
}

/// The words of a notice, in the app's language.
#[must_use]
pub fn notice_text(notice: &Notice) -> String {
    let (key, error) = match notice {
        Notice::CouldNotSaveStopMode(e) => ("stop-mode-could-not-save", e),
        Notice::CouldNotSavePreviewTyping(e) => ("preview-typing-could-not-save", e),
        Notice::CouldNotSaveWriteMethod(e) => ("write-method-could-not-save", e),
        Notice::TypingTestFailed(e) => ("typing-test-failed", e),
        Notice::CouldNotSaveSounds(e) => ("sounds-could-not-save", e),
    };
    crate::i18n::text(key, &[("error", error)]).replace(['\u{2068}', '\u{2069}'], "")
}

/// Store the stop mode.
pub async fn run_save_stop_mode(mode: RecordingStopMode) -> Input {
    match set_recording_stop_mode(mode.to_string()).await {
        Ok(()) => Input::StopModeLoaded(mode),
        Err(e) => Input::StopModeFailed(e.to_string()),
    }
}

/// Store whether words are typed as they are heard.
pub async fn run_save_preview_typing(on: bool) -> Input {
    match set_preview_typing(on).await {
        Ok(()) => Input::PreviewTypingLoaded(on),
        Err(e) => Input::PreviewTypingFailed(e.to_string()),
    }
}

/// Store the write method.
pub async fn run_save_write_method(method: WriteMethod) -> Input {
    match set_write_method(method.to_string()).await {
        Ok(()) => Input::WriteMethodLoaded(method),
        Err(e) => Input::WriteMethodFailed(e.to_string()),
    }
}

/// Have the daemon type the test sentence. `None` in the answer is a daemon
/// that typed but named no method this build knows: the test still passed.
pub async fn run_test_typing() -> Input {
    Input::TypingTested(test_write_method().await.map_err(|e| e.to_string()))
}

/// A second of the typing test's countdown.
pub async fn run_tick() -> Input {
    tokio::time::sleep(Duration::from_secs(1)).await;
    Input::TypingTick
}

/// Read every setting the tab shows, each as the input that shows it. One the
/// daemon can't say comes back as its default, as the app does at
/// startup.
pub async fn run_load() -> Vec<Input> {
    let (themes, stop_mode, preview_typing, write_method, theme, volume) = tokio::join!(
        run_load_themes(),
        run_load_stop_mode(),
        run_load_preview_typing(),
        run_load_write_method(),
        run_load_theme(),
        run_load_volume(),
    );
    vec![
        themes,
        stop_mode,
        preview_typing,
        write_method,
        theme,
        volume,
    ]
}

// One reader per setting, for when the daemon says that one changed. Each
// falls back to the setting's default when the daemon can't say.

/// Read what stops a recording.
pub async fn run_load_stop_mode() -> Input {
    Input::StopModeLoaded(
        get_recording_stop_mode()
            .await
            .inspect_err(|e| log::warn!("Could not read the recording stop mode: {e}"))
            .ok()
            .and_then(|m| m.parse().ok())
            .unwrap_or_default(),
    )
}

/// Read whether previews are typed.
pub async fn run_load_preview_typing() -> Input {
    Input::PreviewTypingLoaded(
        get_preview_typing()
            .await
            .inspect_err(|e| log::warn!("Could not read whether previews are typed: {e}"))
            .unwrap_or(false),
    )
}

/// Read how transcripts are typed.
pub async fn run_load_write_method() -> Input {
    Input::WriteMethodLoaded(
        get_write_method()
            .await
            .inspect_err(|e| log::warn!("Could not read the write method: {e}"))
            .ok()
            .and_then(|m| m.parse().ok())
            .unwrap_or_default(),
    )
}

/// Read the sound theme.
pub async fn run_load_theme() -> Input {
    Input::ThemeLoaded(
        get_current_audio_theme()
            .await
            .inspect_err(|e| log::warn!("Could not read the sound theme: {e}"))
            .unwrap_or_default(),
    )
}

/// Read the sound volume.
pub async fn run_load_volume() -> Input {
    Input::VolumeLoaded(
        get_volume()
            .await
            .inspect_err(|e| log::warn!("Could not read the volume: {e}"))
            .unwrap_or(100),
    )
}

/// Read the sound themes, or the built-in list when the daemon can't say.
pub async fn run_load_themes() -> Input {
    Input::ThemesLoaded(load_audio_themes().await)
}

/// Store a theme, and play it when `play`. Nothing comes back when it was
/// stored: it shows already.
pub async fn run_save_theme(
    chosen: AudioTheme,
    play: bool,
    theme: AudioTheme,
    sounding: AudioTheme,
) -> Option<Input> {
    let result = if play {
        set_and_test_audio_theme(chosen).await
    } else {
        set_audio_theme(chosen).await
    };
    result.err().map(|e| Input::ThemeFailed {
        theme,
        sounding,
        message: e.to_string(),
    })
}

/// Store the volume. Nothing comes back when it was stored: it shows already.
pub async fn run_save_volume(volume: u8, stored: u8) -> Option<Input> {
    set_volume(volume).await.err().map(|e| Input::VolumeFailed {
        volume: stored,
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_shows_once_the_daemon_stored_it() {
        let mut state = BehaviorState::default();
        let out = update(&mut state, Input::PreviewTypingToggled(true));
        assert_eq!(out.effects, [Effect::SavePreviewTyping(true)]);
        assert!(!state.preview_typing);
        update(&mut state, Input::PreviewTypingLoaded(true));
        assert!(state.preview_typing);
        let out = update(&mut state, Input::PreviewTypingFailed("down".into()));
        assert_eq!(
            out.notice,
            Some(Notice::CouldNotSavePreviewTyping("down".into()))
        );
        assert!(state.preview_typing);
    }

    /// One tick a second, typing on the last; a tick after a cancel does
    /// nothing, or the cancelled test would type into the next window.
    #[test]
    fn the_typing_countdown_fires_on_its_last_second_and_not_after_a_cancel() {
        let mut state = BehaviorState::default();
        assert_eq!(
            update(&mut state, Input::TestTypingLater).effects,
            [Effect::Tick]
        );
        let mut ticks = 0;
        loop {
            ticks += 1;
            let out = update(&mut state, Input::TypingTick);
            if out.effects == [Effect::TestTyping { into_field: false }] {
                break;
            }
            assert_eq!(out.effects, [Effect::Tick]);
        }
        assert_eq!(ticks, TYPING_COUNTDOWN_SECONDS);
        assert!(state.typing_countdown.is_none());

        update(&mut state, Input::TestTypingLater);
        update(&mut state, Input::CancelTypingTest);
        assert!(update(&mut state, Input::TypingTick).effects.is_empty());
    }

    #[test]
    fn a_new_write_method_forgets_what_the_last_test_came_to() {
        let mut state = BehaviorState::default();
        update(
            &mut state,
            Input::TypingTested(Ok(Some(WriteMethod::default()))),
        );
        assert!(state.typed_with.is_some());
        update(&mut state, Input::WriteMethodLoaded(WriteMethod::default()));
        assert!(state.typed_with.is_none());
    }

    #[test]
    fn sounds_show_at_once_and_go_back_when_refused() {
        let mut state = BehaviorState::default();
        update(&mut state, Input::ThemeLoaded(AudioTheme::default()));
        let before = state.audio_theme;
        let out = update(&mut state, Input::SoundsToggled(false));
        assert_eq!(state.audio_theme, AudioTheme::Silent);
        let [
            Effect::SaveTheme {
                theme, sounding, ..
            },
        ] = out.effects.as_slice()
        else {
            panic!("a save, got {:?}", out.effects);
        };
        update(
            &mut state,
            Input::ThemeFailed {
                theme: *theme,
                sounding: *sounding,
                message: "down".into(),
            },
        );
        assert_eq!(state.audio_theme, before);
        // Turned back on, the theme it had comes back.
        update(&mut state, Input::SoundsToggled(false));
        update(&mut state, Input::SoundsToggled(true));
        assert_eq!(state.audio_theme, before);
    }

    #[test]
    fn every_choice_has_its_words_and_silent_is_not_a_theme_to_pick() {
        for mode in STOP_MODES {
            assert!(crate::i18n::LANGUAGE_LOADER.has(stop_mode_key(mode)));
        }
        for theme in [AudioTheme::Classic, AudioTheme::Retro, AudioTheme::Silent] {
            assert!(crate::i18n::LANGUAGE_LOADER.has(theme_key(theme)));
        }
        assert!(write_methods().contains(&WriteMethod::Auto));
        let state = BehaviorState {
            audio_themes: vec![AudioTheme::Classic, AudioTheme::Silent],
            ..BehaviorState::default()
        };
        assert_eq!(sounding_themes(&state), [AudioTheme::Classic]);
        let words = notice_text(&Notice::CouldNotSaveSounds("down".into()));
        assert!(
            words.contains("down") && !words.contains('\u{2068}'),
            "{words}"
        );
    }

    #[test]
    fn the_volume_is_stored_when_let_go_and_goes_back_when_refused() {
        let mut state = BehaviorState::default();
        update(&mut state, Input::VolumeLoaded(80));
        assert!(
            update(&mut state, Input::VolumeChanged(40))
                .effects
                .is_empty()
        );
        let out = update(&mut state, Input::VolumeCommitted);
        assert_eq!(
            out.effects,
            [Effect::SaveVolume {
                volume: 40,
                stored: 80
            }]
        );
        update(
            &mut state,
            Input::VolumeFailed {
                volume: 80,
                message: "down".into(),
            },
        );
        assert_eq!((state.volume, state.stored_volume), (80, 80));
    }
}
