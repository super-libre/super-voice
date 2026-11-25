// SPDX-License-Identifier: GPL-3.0-only
//! The microphone for an app written in another language: a store that
//! follows the daemon's events and tells the app whenever the microphone
//! opened, closed, or its level moved. The pages that record read it for their
//! meter, and to know when something else holds the microphone.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super_voice_app_core::state::Microphone as Held;
use tokio::sync::broadcast::error::RecvError;

use crate::RUNTIME;

/// What the app is told. Called from the store's own threads: the app hops to
/// its main thread before it draws.
#[uniffi::export(with_foreign)]
pub trait MicrophoneObserver: Send + Sync {
    /// The microphone changed; draw this.
    fn changed(&self, microphone: Microphone);
}

/// What the daemon last said about its microphone.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct Microphone {
    /// Whether the microphone is open: a take of any app, or dictation.
    pub recording: bool,
    /// How loud it is, 0 to 1.
    pub level: f32,
    /// Whether that sounds like speech.
    pub speech: bool,
}

impl From<Held> for Microphone {
    fn from(held: Held) -> Self {
        Self {
            recording: held.recording,
            level: held.level,
            speech: held.speech,
        }
    }
}

/// The microphone's store.
#[derive(uniffi::Object)]
pub struct MicrophoneStore {
    held: Mutex<Held>,
    observer: Arc<dyn MicrophoneObserver>,
}

#[uniffi::export]
impl MicrophoneStore {
    /// A store that tells `observer` whenever the microphone changes, from
    /// now on.
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn MicrophoneObserver>) -> Arc<Self> {
        let store = Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
        });
        follow(Arc::downgrade(&store));
        store
    }

    /// The microphone now.
    pub fn microphone(&self) -> Microphone {
        (*self.lock()).into()
    }
}

impl MicrophoneStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Hand the daemon's events to `store` until it is dropped.
fn follow(store: Weak<MicrophoneStore>) {
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
                held.follow(&event).then_some(*held)
            };
            if let Some(microphone) = changed {
                store.observer.changed(microphone.into());
            }
        }
    });
}
