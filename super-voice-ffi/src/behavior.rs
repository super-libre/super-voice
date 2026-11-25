// SPDX-License-Identifier: GPL-3.0-only
//! Speech to text's Behavior tab for an app written in another language: a
//! store that holds the tab's state, runs the core's logic for it
//! (`pages::behavior`), and tells the app whenever the tab changed.
//!
//! Each choice comes with its name in the user's language, from the shared
//! catalogs, under the same keys the settings app uses.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super_voice_app_core::pages::behavior::{self as page, Effect, Input, Notice, Section};
use super_voice_app_core::state::AudioTheme;
use super_voice_app_core::state::behavior::BehaviorState;
use super_voice_shared::models::recording_stop_mode::RecordingStopMode;
use super_voice_shared::models::write_method::WriteMethod;

use crate::RUNTIME;

/// The stop modes, in the settings app's order.
const STOP_MODES: [RecordingStopMode; 3] = [
    RecordingStopMode::SilenceOnly,
    RecordingStopMode::SilenceAndManual,
    RecordingStopMode::ManualOnly,
];

/// How long the cursor gets to land in the test field before the daemon types.
const FOCUS_SETTLES: Duration = Duration::from_millis(250);

/// What the app is told. Called from the store's own threads.
#[uniffi::export(with_foreign)]
pub trait BehaviorObserver: Send + Sync {
    /// The tab changed; draw this.
    fn changed(&self, page: BehaviorPage);

    /// Put the cursor in the typing test's field: the daemon is about to
    /// type into whatever has focus.
    fn focus_typing_field(&self);
}

/// What the user did on the tab. Choices are named by the ids the page
/// gave them.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum BehaviorAction {
    /// Read the settings: the tab was opened.
    Reload,
    StopModeChanged {
        id: String,
    },
    PreviewTypingToggled {
        on: bool,
    },
    WriteMethodChanged {
        id: String,
    },
    /// The typing test's field now holds this.
    TypingTestTextChanged {
        text: String,
    },
    /// Type the test sentence into the test field now.
    TestTyping,
    /// Type it after a countdown, into whatever window has focus then.
    TestTypingLater,
    CancelTypingTest,
    SoundsToggled {
        on: bool,
    },
    ThemePicked {
        id: String,
    },
    /// The volume slider moved.
    VolumeChanged {
        volume: u8,
    },
    /// It was let go.
    VolumeCommitted,
}

/// One choice in a picker.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Choice {
    pub id: String,
    /// Its name, in the user's language.
    pub name: String,
}

/// Which part of the tab a notice is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BehaviorSection {
    Recording,
    Typing,
}

/// Something to tell the user, with the section it is about, in the user's
/// language around the daemon's own words.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BehaviorNotice {
    pub section: BehaviorSection,
    pub message: String,
}

impl From<Notice> for BehaviorNotice {
    fn from(notice: Notice) -> Self {
        let section = match notice.section() {
            Section::Recording => BehaviorSection::Recording,
            Section::Typing => BehaviorSection::Typing,
        };
        let (key, error) = match notice {
            Notice::CouldNotSaveStopMode(e) => ("stop-mode-could-not-save", e),
            Notice::CouldNotSavePreviewTyping(e) => ("preview-typing-could-not-save", e),
            Notice::CouldNotSaveWriteMethod(e) => ("write-method-could-not-save", e),
            Notice::TypingTestFailed(e) => ("typing-test-failed", e),
            // The daemon's own words, as the settings app shows them.
            Notice::CouldNotSaveSounds(message) => return Self { section, message },
        };
        let message = crate::text_with(key, HashMap::from([("error".to_string(), error)]));
        Self { section, message }
    }
}

/// Everything the tab shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BehaviorPage {
    /// Whether the settings have been read.
    pub loaded: bool,
    pub stop_modes: Vec<Choice>,
    pub stop_mode: String,
    pub preview_typing: bool,
    /// The write methods this machine can type with.
    pub write_methods: Vec<Choice>,
    pub write_method: String,
    /// The name of what the last typing test typed with, when it said.
    pub typed_with: Option<String>,
    /// The typing test's field, as the store holds it. When
    /// `typing_test_replaced` goes up, the app's field takes it.
    pub typing_test_text: String,
    pub typing_test_replaced: u64,
    /// Seconds left before the delayed typing test types.
    pub typing_countdown: Option<u8>,
    pub sounds_on: bool,
    /// The themes that make a sound.
    pub themes: Vec<Choice>,
    pub theme: String,
    /// 0 to 100.
    pub volume: u8,
    pub notice: Option<BehaviorNotice>,
}

/// The tab's state, and what the app needs to know of it.
#[derive(Default)]
struct Held {
    state: BehaviorState,
    loaded: bool,
    notice: Option<BehaviorNotice>,
    typing_test_replaced: u64,
}

/// The Behavior tab's store.
#[derive(uniffi::Object)]
pub struct BehaviorStore {
    held: Mutex<Held>,
    observer: Arc<dyn BehaviorObserver>,
}

#[uniffi::export]
impl BehaviorStore {
    /// A store that tells `observer` what changes. Nothing is read until the
    /// app sends [`BehaviorAction::Reload`].
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn BehaviorObserver>) -> Arc<Self> {
        Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
        })
    }

    /// What the tab shows now.
    pub fn page(&self) -> BehaviorPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: BehaviorAction) {
        if action == BehaviorAction::Reload {
            RUNTIME.spawn(async move {
                for input in page::run_load().await {
                    Arc::clone(&self).dispatch(input);
                }
                let page = {
                    let mut held = self.lock();
                    held.loaded = true;
                    snapshot(&held)
                };
                self.observer.changed(page);
            });
            return;
        }
        let Some(input) = self.input(action) else {
            return;
        };
        self.dispatch(input);
    }
}

impl BehaviorStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `action` as the tab's logic takes it. `None` for a choice whose id
    /// names nothing.
    fn input(&self, action: BehaviorAction) -> Option<Input> {
        Some(match action {
            BehaviorAction::Reload => return None,
            BehaviorAction::StopModeChanged { id } => Input::StopModeChanged(id.parse().ok()?),
            BehaviorAction::PreviewTypingToggled { on } => Input::PreviewTypingToggled(on),
            BehaviorAction::WriteMethodChanged { id } => {
                Input::WriteMethodChanged(id.parse().ok()?)
            }
            BehaviorAction::TypingTestTextChanged { text } => Input::TypingTestTextChanged(text),
            BehaviorAction::TestTyping => Input::TestTyping,
            BehaviorAction::TestTypingLater => Input::TestTypingLater,
            BehaviorAction::CancelTypingTest => Input::CancelTypingTest,
            BehaviorAction::SoundsToggled { on } => Input::SoundsToggled(on),
            BehaviorAction::ThemePicked { id } => {
                let themes = self.lock().state.audio_themes.clone();
                Input::ThemePicked(themes.into_iter().find(|t| t.to_string() == id)?)
            }
            BehaviorAction::VolumeChanged { volume } => Input::VolumeChanged(volume.min(100)),
            BehaviorAction::VolumeCommitted => Input::VolumeCommitted,
        })
    }

    /// Hand `input` to the tab's logic, tell the app what changed, and start
    /// the work it asked for.
    fn dispatch(self: Arc<Self>, input: Input) {
        let typed = matches!(input, Input::TypingTestTextChanged(_));
        let (page, effects) = {
            let mut held = self.lock();
            let before = held.state.typing_test_text.clone();
            let outcome = page::update(&mut held.state, input);
            if !typed && held.state.typing_test_text != before {
                held.typing_test_replaced += 1;
            }
            if let Some(section) = outcome.clear_notice {
                let cleared = match section {
                    Section::Recording => BehaviorSection::Recording,
                    Section::Typing => BehaviorSection::Typing,
                };
                if held.notice.as_ref().is_some_and(|n| n.section == cleared) {
                    held.notice = None;
                }
            }
            if let Some(notice) = outcome.notice {
                held.notice = Some(notice.into());
            }
            (snapshot(&held), outcome.effects)
        };
        self.observer.changed(page);
        for effect in effects {
            Arc::clone(&self).run(effect);
        }
    }

    /// Start `effect`. Its answer, when it has one, comes back through
    /// [`Self::dispatch`].
    fn run(self: Arc<Self>, effect: Effect) {
        RUNTIME.spawn(async move {
            let input = match effect {
                Effect::SaveStopMode(mode) => Some(page::run_save_stop_mode(mode).await),
                Effect::SavePreviewTyping(on) => Some(page::run_save_preview_typing(on).await),
                Effect::SaveWriteMethod(method) => Some(page::run_save_write_method(method).await),
                Effect::TestTyping { into_field } => {
                    if into_field {
                        self.observer.focus_typing_field();
                        tokio::time::sleep(FOCUS_SETTLES).await;
                    }
                    Some(page::run_test_typing().await)
                }
                Effect::Tick => Some(page::run_tick().await),
                Effect::SaveTheme {
                    chosen,
                    play,
                    theme,
                    sounding,
                } => page::run_save_theme(chosen, play, theme, sounding).await,
                Effect::SaveVolume { volume, stored } => {
                    page::run_save_volume(volume, stored).await
                }
            };
            if let Some(input) = input {
                self.dispatch(input);
            }
        });
    }
}

/// A stop mode's name, under the settings app's key.
fn stop_mode_name(mode: RecordingStopMode) -> String {
    super_voice_app_core::i18n::text(
        match mode {
            RecordingStopMode::SilenceOnly => "stop-silence",
            RecordingStopMode::SilenceAndManual => "stop-silence-manual",
            RecordingStopMode::ManualOnly => "stop-manual",
        },
        &[],
    )
}

/// A write method's name, under the settings app's key.
fn write_method_name(method: WriteMethod) -> String {
    let key = match method {
        WriteMethod::Auto => "auto-recommended",
        WriteMethod::XdgDesktopPortal => "xdg-desktop-portal",
        WriteMethod::Ydotool => return "ydotool".to_string(),
        WriteMethod::BuiltIn => "built-in",
    };
    super_voice_app_core::i18n::text(key, &[])
}

/// A theme's name, under the settings app's key.
fn theme_name(theme: AudioTheme) -> String {
    let key = match theme {
        AudioTheme::Classic => "theme-classic",
        AudioTheme::Gentle => "theme-gentle",
        AudioTheme::Minimal => "theme-minimal",
        AudioTheme::SciFi => "theme-scifi",
        AudioTheme::Musical => "theme-musical",
        AudioTheme::Nature => "theme-nature",
        AudioTheme::Retro => "theme-retro",
        AudioTheme::Silent => "theme-silent",
    };
    super_voice_app_core::i18n::text(key, &[])
}

fn snapshot(held: &Held) -> BehaviorPage {
    let state = &held.state;
    BehaviorPage {
        loaded: held.loaded,
        stop_modes: STOP_MODES
            .iter()
            .map(|m| Choice {
                id: m.to_string(),
                name: stop_mode_name(*m),
            })
            .collect(),
        stop_mode: state.stop_mode.to_string(),
        preview_typing: state.preview_typing,
        write_methods: WriteMethod::supported_on_this_platform()
            .into_iter()
            .map(|m| Choice {
                id: m.to_string(),
                name: write_method_name(m),
            })
            .collect(),
        write_method: state.write_method.to_string(),
        typed_with: state.typed_with.map(write_method_name),
        typing_test_text: state.typing_test_text.clone(),
        typing_test_replaced: held.typing_test_replaced,
        typing_countdown: state.typing_countdown,
        sounds_on: state.sounds_on(),
        themes: state
            .audio_themes
            .iter()
            .filter(|t| **t != AudioTheme::Silent)
            .map(|t| Choice {
                id: t.to_string(),
                name: theme_name(*t),
            })
            .collect(),
        theme: state.sounding_theme.to_string(),
        volume: state.volume,
        notice: held.notice.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{BehaviorNotice, BehaviorSection, Notice};

    /// A failed save says what didn't save, around the daemon's words.
    #[test]
    fn a_notice_says_what_failed() {
        let notice = BehaviorNotice::from(Notice::CouldNotSaveStopMode("down".into()));
        assert_eq!(notice.section, BehaviorSection::Recording);
        assert_eq!(notice.message, "Couldn't save the stop mode: down");

        let notice = BehaviorNotice::from(Notice::TypingTestFailed("no portal".into()));
        assert_eq!(notice.section, BehaviorSection::Typing);
        assert_eq!(notice.message, "The typing test failed: no portal");
    }

    /// Saving a sound has no lead-in: the daemon's words, as in the settings
    /// app.
    #[test]
    fn a_sound_notice_is_the_daemons_words() {
        let notice = BehaviorNotice::from(Notice::CouldNotSaveSounds("busy".into()));
        assert_eq!(notice.message, "busy");
    }
}
