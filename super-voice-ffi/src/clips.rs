// SPDX-License-Identifier: GPL-3.0-only
//! The Clips page for an app written in another language: a store that holds
//! the page's state, runs the core's logic for it (`pages::clips`) on the
//! stores' runtime, and tells the app whenever the page changed.
//!
//! The app sends what the user did ([`ClipsAction`], and the draft's text
//! through [`ClipsStore::set_draft`]) and draws the [`ClipsPage`] it is handed.
//! Making a clip, reading one to play, the player itself, exporting and
//! deleting all happen here, as they do in the settings app.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super_voice_app_core::clips::{self, Clip};
use super_voice_app_core::pages::clips::{self as page, Effect, Input, Notice};
use super_voice_app_core::state::clips::ClipsState;

use crate::{Buffer, RUNTIME};

/// How often a playing clip is checked for having played out.
const TICK: Duration = Duration::from_millis(250);

/// What the app is told. Called from the store's own threads: the app hops to
/// its main thread before it draws.
#[uniffi::export(with_foreign)]
pub trait ClipsObserver: Send + Sync {
    /// The page changed; draw this.
    fn changed(&self, page: ClipsPage);

    /// Ask the user where to export the clip `id`, suggesting `name`, then
    /// call [`ClipsStore::export_to`] with the answer.
    fn export_requested(&self, id: String, name: String);
}

/// What the user did on the page.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ClipsAction {
    /// Read the folder again: the page was opened.
    Reload,
    /// Make a clip of the draft.
    Make,
    /// Stop making it.
    Cancel,
    /// Play a clip.
    Play { id: String },
    /// Stop the clip playing.
    Stop,
    /// Export a clip.
    Export { id: String },
    /// Delete was pressed once for a clip.
    DeleteRequested { id: String },
    /// The delete was called off.
    DeleteCancelled,
    /// Delete the clip.
    DeleteConfirmed { id: String },
}

impl From<ClipsAction> for Input {
    fn from(action: ClipsAction) -> Self {
        match action {
            ClipsAction::Reload => Self::Reload,
            // The voice a clip is made in is named by the settings app; this
            // app doesn't know text to speech's voice yet.
            ClipsAction::Make => Self::Make { voice: None },
            ClipsAction::Cancel => Self::Cancel,
            ClipsAction::Play { id } => Self::Play(id),
            ClipsAction::Stop => Self::Stop,
            ClipsAction::Export { id } => Self::Export(id),
            ClipsAction::DeleteRequested { id } => Self::DeleteRequested(id),
            ClipsAction::DeleteCancelled => Self::DeleteCancelled,
            ClipsAction::DeleteConfirmed { id } => Self::DeleteConfirmed(id),
        }
    }
}

/// One clip in the list.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ClipSummary {
    /// The clip's id: its files' name without the extension.
    pub id: String,
    /// When it was made, as seconds since 1970.
    pub created: i64,
    /// What it says.
    pub text: String,
    /// The voice it was made in, as the settings app named it.
    pub voice: Option<String>,
    /// How long it plays.
    pub seconds: f32,
}

impl From<&Clip> for ClipSummary {
    fn from(clip: &Clip) -> Self {
        Self {
            id: clip.id.clone(),
            created: clip
                .created
                .and_local_timezone(chrono::Local)
                .earliest()
                .map_or(0, |at| at.timestamp()),
            text: clip.text.clone(),
            voice: clip.voice.clone(),
            seconds: clip.seconds,
        }
    }
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ClipsNotice {
    CouldNotRead { message: String },
    CouldNotMake { message: String },
    CouldNotPlay { message: String },
    CouldNotExport { message: String },
    CouldNotDelete { message: String },
}

impl From<Notice> for ClipsNotice {
    fn from(notice: Notice) -> Self {
        match notice {
            Notice::CouldNotRead(message) => Self::CouldNotRead { message },
            Notice::CouldNotMake(message) => Self::CouldNotMake { message },
            Notice::CouldNotPlay(message) => Self::CouldNotPlay { message },
            Notice::CouldNotExport(message) => Self::CouldNotExport { message },
            Notice::CouldNotDelete(message) => Self::CouldNotDelete { message },
        }
    }
}

/// Everything the page shows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ClipsPage {
    /// Whether the folder has been read.
    pub loaded: bool,
    /// The clips, newest first.
    pub clips: Vec<ClipSummary>,
    /// What the draft says, as the store holds it.
    pub draft: String,
    /// How many words that is, by the settings app's count.
    pub draft_words: u32,
    /// How many times the store replaced the draft (emptied it once its clip
    /// was made). When it goes up, the app's editor takes
    /// [`draft`](Self::draft).
    pub draft_replaced: u64,
    /// Whether a clip is being made.
    pub making: bool,
    /// The clip playing, or about to.
    pub playing: Option<String>,
    /// How far it has played, and how long it is, in seconds. `None` while
    /// its audio is still being read ("Starting…").
    pub playing_position: Option<f32>,
    pub playing_length: Option<f32>,
    /// The clip whose Delete was pressed once.
    pub confirming_delete: Option<String>,
    /// The clip last exported, and where it went.
    pub exported_id: Option<String>,
    pub exported_path: Option<String>,
    /// What to tell the user.
    pub notice: Option<ClipsNotice>,
}

/// The page's state, and the notice it shows.
#[derive(Default)]
struct Held {
    state: ClipsState<Buffer>,
    notice: Option<ClipsNotice>,
}

/// The Clips page's store.
#[derive(uniffi::Object)]
pub struct ClipsStore {
    held: Mutex<Held>,
    observer: Arc<dyn ClipsObserver>,
    /// Whether a ticker is checking the clip playing.
    ticking: AtomicBool,
}

#[uniffi::export]
impl ClipsStore {
    /// A store that tells `observer` what changes. Nothing is read until the
    /// app sends [`ClipsAction::Reload`].
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn ClipsObserver>) -> Arc<Self> {
        crate::identify();
        Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
            ticking: AtomicBool::new(false),
        })
    }

    /// What the page shows now.
    pub fn page(&self) -> ClipsPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: ClipsAction) {
        self.dispatch(action.into());
    }

    /// The draft's editor now holds `text`.
    pub fn set_draft(self: Arc<Self>, text: String) {
        let changed = {
            let mut held = self.lock();
            if held.state.draft.text == text {
                false
            } else {
                held.state.draft.text = text;
                true
            }
        };
        // The words and the estimate under the editor follow the draft.
        if changed {
            let page = self.page();
            self.observer.changed(page);
        }
    }

    /// The export dialog was answered: copy the clip `id` to `path`, or
    /// nothing when the user closed it (`None`).
    pub fn export_to(self: Arc<Self>, id: String, path: Option<String>) {
        let store = Arc::clone(&self);
        RUNTIME.spawn(async move {
            let result = match path {
                None => Ok(None),
                Some(chosen) => page::export_to(&id, &PathBuf::from(chosen)).await,
            };
            store.dispatch(Input::Exported { id, result });
        });
    }
}

impl ClipsStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hand `input` to the page's logic, tell the app what changed, and start
    /// the work it asked for.
    fn dispatch(self: Arc<Self>, input: Input) {
        let (page, effects, playing) = {
            let mut held = self.lock();
            let outcome = page::update(&mut held.state, input);
            if outcome.clear_notice {
                held.notice = None;
            }
            if let Some(notice) = outcome.notice {
                held.notice = Some(notice.into());
            }
            let playing = held.state.playing.is_some();
            (snapshot(&held), outcome.effects, playing)
        };
        self.observer.changed(page);
        for effect in effects {
            Arc::clone(&self).run(effect);
        }
        if playing {
            self.tick_while_playing();
        }
    }

    /// Start `effect`. Its answer comes back through [`Self::dispatch`].
    fn run(self: Arc<Self>, effect: Effect) {
        match effect {
            Effect::List => self.answer(page::run_list()),
            Effect::ReadSamples(id) => self.answer(page::run_samples(id)),
            Effect::Delete(id) => self.answer(page::run_delete(id)),
            Effect::Make { text, voice, abort } => {
                RUNTIME.spawn(async move {
                    // Nothing comes back from a clip Cancel aborted.
                    if let Some(input) = page::run_make(text, voice, abort).await {
                        self.dispatch(input);
                    }
                });
            }
            Effect::Export { id, name } => self.observer.export_requested(id, name),
        }
    }

    /// Run `work` and hand its answer back to the page.
    fn answer(self: Arc<Self>, work: impl Future<Output = Input> + Send + 'static) {
        RUNTIME.spawn(async move {
            let input = work.await;
            self.dispatch(input);
        });
    }

    /// While a clip plays, tell the page every [`TICK`], so it lets a clip go
    /// once it has played out. One ticker at a time.
    fn tick_while_playing(self: Arc<Self>) {
        if self.ticking.swap(true, Ordering::AcqRel) {
            return;
        }
        RUNTIME.spawn(async move {
            loop {
                tokio::time::sleep(TICK).await;
                if self.lock().state.playing.is_none() {
                    break;
                }
                Arc::clone(&self).dispatch(Input::Tick);
            }
            self.ticking.store(false, Ordering::Release);
        });
    }
}

fn snapshot(held: &Held) -> ClipsPage {
    let state = &held.state;
    let player = state.playing.as_ref().and_then(|p| p.player.as_ref());
    let (exported_id, exported_path) = state
        .exported
        .clone()
        .map_or((None, None), |(id, path)| (Some(id), Some(path)));
    ClipsPage {
        loaded: state.loaded,
        clips: state.items.iter().map(ClipSummary::from).collect(),
        draft: state.draft.text.clone(),
        draft_words: u32::try_from(super_voice_app_core::notes::words(&state.draft.text))
            .unwrap_or(u32::MAX),
        draft_replaced: state.draft.replaced,
        making: state.making.is_some(),
        playing: state.playing.as_ref().map(|p| p.id.clone()),
        playing_position: player.map(|p| p.position().as_secs_f32()),
        playing_length: player.map(|p| p.length().as_secs_f32()),
        confirming_delete: state.confirming_delete.clone(),
        exported_id,
        exported_path,
        notice: held.notice.clone(),
    }
}

/// The folder the clips are kept in.
#[must_use]
#[uniffi::export]
pub fn clips_folder() -> String {
    clips::dir().display().to_string()
}
