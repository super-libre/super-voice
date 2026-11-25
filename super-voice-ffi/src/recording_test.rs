// SPDX-License-Identifier: GPL-3.0-only
//! Speech to text's recording test, on its Behavior tab, for an app written
//! in another language: a store that runs the core's logic for it
//! (`pages::recording_test`), records its take through the daemon, follows the
//! microphone from the daemon's events, and tells the app whenever the test
//! changed.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use futures_util::StreamExt;
use super_voice_app_core::events::Event;
use super_voice_app_core::pages::recording_test::{self as page, Effect, Input};
use super_voice_app_core::state::Microphone;
use super_voice_app_core::state::recording_test::{Heard, RecordingTest};
use tokio::sync::broadcast::error::RecvError;

use crate::RUNTIME;
use crate::notes::PreviewSpans;

/// What the app is told. Called from the store's own threads: the app hops to
/// its main thread before it draws.
#[uniffi::export(with_foreign)]
pub trait RecordingTestObserver: Send + Sync {
    /// The test changed; draw this.
    fn changed(&self, page: RecordingTestPage);
}

/// What the user did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RecordingTestAction {
    /// Test recording was pressed.
    Start,
    /// Stop recording was pressed.
    Stop,
}

/// What the last take came to.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum TestHeard {
    /// Its transcript.
    Words { text: String },
    /// It recorded, and no speech was in it.
    Nothing,
    /// It failed, in the daemon's words.
    Failed { message: String },
}

/// Everything the test shows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RecordingTestPage {
    /// Whether the microphone is open: this test's take, or another.
    pub recording: bool,
    /// Whether what it hears sounds like speech.
    pub speech: bool,
    /// How loud it is, 0 to 1.
    pub level: f32,
    /// What the take has heard so far.
    pub preview: String,
    /// What that spans, when the daemon said.
    pub preview_spans: Option<PreviewSpans>,
    /// What the last take came to, until the next starts.
    pub heard: Option<TestHeard>,
}

/// The test's state, and the microphone it shares with the daemon.
#[derive(Default)]
struct Held {
    test: RecordingTest,
    microphone: Microphone,
}

/// The recording test's store.
#[derive(uniffi::Object)]
pub struct RecordingTestStore {
    held: Mutex<Held>,
    observer: Arc<dyn RecordingTestObserver>,
}

#[uniffi::export]
impl RecordingTestStore {
    /// A store that tells `observer` what changes, following the microphone
    /// from now on.
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn RecordingTestObserver>) -> Arc<Self> {
        crate::identify();
        let store = Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
        });
        follow(Arc::downgrade(&store));
        store
    }

    /// What the test shows now.
    pub fn page(&self) -> RecordingTestPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: RecordingTestAction) {
        self.dispatch(match action {
            RecordingTestAction::Start => Input::Start,
            RecordingTestAction::Stop => Input::Stop,
        });
    }
}

impl RecordingTestStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hand `input` to the test's logic, tell the app what changed, and start
    /// the work it asked for.
    fn dispatch(self: Arc<Self>, input: Input) {
        let (page, effects) = {
            let mut held = self.lock();
            let Held { test, microphone } = &mut *held;
            let effects = page::update(test, microphone, input);
            (snapshot(&held), effects)
        };
        self.observer.changed(page);
        for effect in effects {
            Arc::clone(&self).run(effect);
        }
    }

    /// Start `effect`. Its answers come back through [`Self::dispatch`].
    fn run(self: Arc<Self>, effect: Effect) {
        match effect {
            Effect::Take => {
                RUNTIME.spawn(async move {
                    let mut take = std::pin::pin!(page::take());
                    while let Some(input) = take.next().await {
                        Arc::clone(&self).dispatch(input);
                    }
                });
            }
            Effect::Stop => {
                RUNTIME.spawn(page::run_stop());
            }
        }
    }
}

/// Hand the daemon's microphone events to `store` until it is dropped.
fn follow(store: Weak<RecordingTestStore>) {
    let mut events = crate::events::listen();
    RUNTIME.spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                // Skipped readings are only a meter's levels.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            };
            let Some(store) = store.upgrade() else {
                return;
            };
            let changed = {
                let mut held = store.lock();
                held.microphone.follow(&event).then(|| snapshot(&held))
            };
            if let Some(page) = changed {
                store.observer.changed(page);
            }
            // What the test showed belonged to a session that is gone.
            if matches!(event, Event::Connected) {
                store.dispatch(Input::Reconnected);
            }
        }
    });
}

fn snapshot(held: &Held) -> RecordingTestPage {
    let Held { test, microphone } = held;
    RecordingTestPage {
        recording: microphone.recording,
        speech: microphone.speech,
        level: microphone.level,
        preview: test.preview.clone(),
        preview_spans: test.preview_source.map(PreviewSpans::from),
        heard: test.heard.clone().map(|heard| match heard {
            Heard::Words(text) => TestHeard::Words { text },
            Heard::Nothing => TestHeard::Nothing,
            Heard::Failed(message) => TestHeard::Failed { message },
        }),
    }
}
