// SPDX-License-Identifier: GPL-3.0-only
//! The speech test, on text to speech's Behavior tab, for an app written in
//! another language: a store that holds the test's state, runs the core's
//! logic for it (`pages::speech`), follows the daemon's events, and tells the
//! app whenever the test changed.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super_voice_app_core::events::Event;
use super_voice_app_core::pages::speech::{self as page, Effect, Input, Notice};
use super_voice_app_core::state::{SpeakingStatus, SpeechState};
use tokio::sync::broadcast::error::RecvError;

use crate::RUNTIME;

/// What the app is told. Called from the store's own threads.
#[uniffi::export(with_foreign)]
pub trait SpeechObserver: Send + Sync {
    /// The test changed; draw this.
    fn changed(&self, page: SpeechPage);
}

/// What the user did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SpeechAction {
    /// Speak what the field holds.
    Speak,
    /// Stop what the daemon is saying.
    Stop,
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SpeechNotice {
    /// The daemon refused the utterance.
    CouldNotSpeak { message: String },
}

impl From<Notice> for SpeechNotice {
    fn from(notice: Notice) -> Self {
        match notice {
            Notice::CouldNotSpeak(message) => Self::CouldNotSpeak { message },
        }
    }
}

/// Everything the test shows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SpeechPage {
    /// Whether the daemon is speaking, for this app or any other.
    pub speaking: bool,
    /// Whether sound is reaching the speaker yet: an utterance is speaking
    /// before its first sample plays.
    pub playing: bool,
    /// How loud it is, 0 to 1.
    pub level: f32,
    /// Whether Speak can be pressed: nothing is speaking, and the field holds
    /// something to say.
    pub can_speak: bool,
    /// What to tell the user.
    pub notice: Option<SpeechNotice>,
}

/// The test's state, and the notice it shows.
#[derive(Default)]
struct Held {
    state: SpeechState,
    notice: Option<SpeechNotice>,
}

/// The speech test's store. Follows the daemon's events until it is dropped.
#[derive(uniffi::Object)]
pub struct SpeechStore {
    held: Mutex<Held>,
    observer: Arc<dyn SpeechObserver>,
}

#[uniffi::export]
impl SpeechStore {
    /// A store that tells `observer` what changes.
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn SpeechObserver>) -> Arc<Self> {
        let store = Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
        });
        follow(Arc::downgrade(&store));
        store
    }

    /// What the test shows now.
    pub fn page(&self) -> SpeechPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: SpeechAction) {
        self.dispatch(match action {
            SpeechAction::Speak => Input::Speak,
            SpeechAction::Stop => Input::Stop,
        });
    }

    /// The test field now holds `text`.
    pub fn set_text(self: Arc<Self>, text: String) {
        self.dispatch(Input::TextChanged(text));
    }
}

impl SpeechStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hand `input` to the test's logic, tell the app when what it shows
    /// changed, and start the work it asked for.
    fn dispatch(self: Arc<Self>, input: Input) {
        let (before, page, effects) = {
            let mut held = self.lock();
            let before = snapshot(&held);
            let outcome = page::update(&mut held.state, input);
            if outcome.clear_notice {
                held.notice = None;
            }
            if let Some(notice) = outcome.notice {
                held.notice = Some(notice.into());
            }
            (before, snapshot(&held), outcome.effects)
        };
        // A keystroke that doesn't change whether Speak can be pressed, or a
        // meter reading the same as the last, draws nothing new.
        if page != before {
            self.observer.changed(page);
        }
        for effect in effects {
            match effect {
                Effect::Speak(text) => {
                    let store = Arc::clone(&self);
                    RUNTIME.spawn(async move {
                        let input = page::run_speak(text).await;
                        store.dispatch(input);
                    });
                }
                Effect::Stop => {
                    RUNTIME.spawn(page::run_stop());
                }
            }
        }
    }
}

/// Hand the daemon's speaking events to `store`'s logic until it is dropped.
fn follow(store: Weak<SpeechStore>) {
    let mut events = crate::events::listen();
    RUNTIME.spawn(async move {
        loop {
            let input = match events.recv().await {
                Ok(Event::Speaking {
                    is_speaking,
                    utterance_id,
                }) => Input::Speaking {
                    is_speaking,
                    utterance_id,
                },
                Ok(Event::PlaybackLevel { level, is_speech }) => Input::Level { level, is_speech },
                // Skipped readings are only a meter's levels.
                Ok(_) | Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            };
            let Some(store) = store.upgrade() else {
                return;
            };
            store.dispatch(input);
        }
    });
}

fn snapshot(held: &Held) -> SpeechPage {
    let state = &held.state;
    SpeechPage {
        speaking: state.status == SpeakingStatus::Speaking,
        playing: state.is_playing,
        level: state.level,
        can_speak: page::can_speak(state),
        notice: held.notice.clone(),
    }
}
