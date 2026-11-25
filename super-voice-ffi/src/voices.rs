// SPDX-License-Identifier: GPL-3.0-only
//! The Voices page for an app written in another language: a store that holds
//! the page's state, runs the core's logic for it (`pages::voices`) on the
//! stores' runtime, and tells the app whenever the page changed.
//!
//! The app sends what the user did ([`VoicesAction`]) and draws the
//! [`VoicesPage`] it is handed. The library, the microphone and the upload all
//! happen here, as they do in the settings app; the file dialog is the app's.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super_voice_app_core::daemon::client::v1::settings::language::get_primary_language;
use super_voice_app_core::pages::voices::{self as page, Effect, Input, Notice};
use super_voice_app_core::state::scripts::{self, Script};
use super_voice_app_core::state::voices::{SampleOrigin, ScriptChoice, VoicesState};
use super_voice_shared::tts::models::voices::{VoiceInfo, VoiceModelSupport};

use crate::RUNTIME;

/// How often the meter and the time are read while recording.
const TICK: Duration = Duration::from_millis(100);

/// What the app is told. Called from the store's own threads: the app hops to
/// its main thread before it draws.
#[uniffi::export(with_foreign)]
pub trait VoicesObserver: Send + Sync {
    /// The page changed; draw this.
    fn changed(&self, page: VoicesPage);

    /// Ask the user for a WAV file, then call [`VoicesStore::file_picked`]
    /// with the answer.
    fn file_requested(&self);
}

/// What the user did on the page.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum VoicesAction {
    /// Read the library again: the page was opened.
    Refresh,
    /// Rename was pressed for a voice.
    BeginRename { id: String, label: String },
    /// The rename field now holds this.
    RenameChanged { label: String },
    /// The rename was called off.
    CancelRename,
    /// Save the rename.
    CommitRename,
    /// Delete a voice.
    Delete { id: String },
    /// Hear a voice, in the loaded model.
    Preview { voice_id: String },
    /// A script was picked by its id, or none: the user's own words.
    ScriptSelected { id: Option<String> },
    /// Open the microphone.
    StartRecording,
    /// Close it, and keep what it heard.
    StopRecording,
    /// Import a recording from a file.
    ImportFile,
    /// The name field now holds this.
    LabelChanged { label: String },
    /// The transcript field now holds this.
    TranscriptChanged { transcript: String },
    /// Throw the recording away.
    DiscardPending,
    /// Save the recording as a voice.
    Save,
}

/// One saved voice.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct VoiceRow {
    /// The library's id for it, which rename and delete take.
    pub id: String,
    /// The id an app speaks in it with: `voice:<id>`.
    pub voice_id: String,
    pub label: String,
    /// How long its recording is.
    pub seconds: f32,
    /// Whether it carries what its recording says.
    pub has_transcript: bool,
    /// Whether its delete is on its way.
    pub deleting: bool,
}

impl VoiceRow {
    fn new(voice: &VoiceInfo, state: &VoicesState) -> Self {
        Self {
            id: voice.id.clone(),
            voice_id: voice.voice_id.clone(),
            label: voice.label.clone(),
            seconds: voice.duration_seconds,
            has_transcript: voice.transcript.is_some(),
            deleting: state.deleting.contains(&voice.id),
        }
    }
}

/// What the loaded model can do with a cloned voice.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct VoiceModel {
    /// Its name.
    pub name: String,
    /// Whether it speaks in cloned voices at all.
    pub clones: bool,
    /// How much of a recording it uses, when it says.
    pub clone_ref_seconds: Option<f32>,
    /// Whether it also needs what the recording says.
    pub needs_transcript: bool,
}

impl From<&VoiceModelSupport> for VoiceModel {
    fn from(model: &VoiceModelSupport) -> Self {
        Self {
            name: model.name.clone(),
            clones: model.clones,
            clone_ref_seconds: model.clone_ref_seconds,
            needs_transcript: model.needs_transcript,
        }
    }
}

/// A script to read while recording.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ScriptInfo {
    /// Its id, which [`VoicesAction::ScriptSelected`] takes.
    pub id: String,
    pub title: String,
    /// The language it is in, as a tag: `en`, `fr`.
    pub language: String,
    /// What it is for.
    pub blurb: String,
    /// What to read.
    pub text: String,
    /// About how long it takes to read, unhurried.
    pub seconds: f32,
    /// Whether the loaded model hears all of it.
    pub fits: bool,
    /// Whether it is in the language the user speaks, or no language is set.
    pub in_language: bool,
}

impl ScriptInfo {
    fn new(script: &Script, budget: Option<f32>, language: Option<&str>) -> Self {
        Self {
            id: script.id.to_string(),
            title: script.title.to_string(),
            language: script.language.to_string(),
            blurb: script.blurb.to_string(),
            text: script.text.to_string(),
            seconds: script.estimated_seconds(),
            fits: script.fits(budget),
            in_language: language.is_none_or(|tag| scripts::same_language(script, tag)),
        }
    }
}

/// The microphone, while it is open.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Recording {
    /// How long it has been.
    pub seconds: f32,
    /// How loud it is now, 0 to 1.
    pub level: f32,
    /// Where it stops by itself.
    pub max_seconds: f32,
}

/// A recording waiting to be named and saved.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PendingVoice {
    /// How long it is. Zero when that could not be read.
    pub seconds: f32,
    /// The file it was imported from; `None` for one recorded here.
    pub imported_from: Option<String>,
    /// The script it was read from. Its words are the transcript, as written.
    pub script: Option<ScriptInfo>,
    /// Whether its upload is on its way.
    pub saving: bool,
    /// Whether Save can be pressed: it has a name, and a transcript when the
    /// loaded model needs one.
    pub can_save: bool,
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum VoicesNotice {
    CouldNotLoad {
        message: String,
    },
    CouldNotImport {
        message: String,
    },
    CouldNotRename {
        message: String,
    },
    CouldNotDelete {
        message: String,
    },
    CouldNotPreview {
        message: String,
    },
    CouldNotRecord {
        message: String,
    },
    CouldNotSave {
        message: String,
    },
    /// The loaded model clones from the transcript, and there is none.
    NeedsTranscript,
}

impl From<Notice> for VoicesNotice {
    fn from(notice: Notice) -> Self {
        match notice {
            Notice::CouldNotLoad(message) => Self::CouldNotLoad { message },
            Notice::CouldNotImport(message) => Self::CouldNotImport { message },
            Notice::CouldNotRename(message) => Self::CouldNotRename { message },
            Notice::CouldNotDelete(message) => Self::CouldNotDelete { message },
            Notice::CouldNotPreview(message) => Self::CouldNotPreview { message },
            Notice::CouldNotRecord(message) => Self::CouldNotRecord { message },
            Notice::CouldNotSave(message) => Self::CouldNotSave { message },
            Notice::NeedsTranscript => Self::NeedsTranscript,
        }
    }
}

/// Everything the page shows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct VoicesPage {
    /// Whether the library has been read.
    pub loaded: bool,
    /// The saved voices, newest first.
    pub voices: Vec<VoiceRow>,
    /// The loaded model; `None` while none is.
    pub model: Option<VoiceModel>,
    /// The language the user speaks, as the daemon has it, which picks the
    /// script suggested.
    pub language: Option<String>,
    /// The scripts the picker offers, the user's language first.
    pub scripts: Vec<ScriptInfo>,
    /// The script to read, or `None` for the user's own words.
    pub script: Option<ScriptInfo>,
    /// Whether the user picked their own words, rather than being suggested
    /// a script.
    pub own_words: bool,
    /// The microphone, while it is open.
    pub recording: Option<Recording>,
    /// The recording waiting to be saved.
    pub pending: Option<PendingVoice>,
    /// The name field, as the store holds it. When `label_replaced` goes up,
    /// the app's field takes it.
    pub label: String,
    pub label_replaced: u64,
    /// The transcript field, likewise.
    pub transcript: String,
    pub transcript_replaced: u64,
    /// The voice being renamed, and its field.
    pub renaming: Option<String>,
    pub rename_label: String,
    /// The voice a preview was asked for, until it is spoken.
    pub previewing: Option<String>,
    /// What to tell the user.
    pub notice: Option<VoicesNotice>,
}

/// The page's state, the notice it shows, and what the app needs to know of
/// its own fields.
#[derive(Default)]
struct Held {
    state: VoicesState,
    notice: Option<VoicesNotice>,
    language: Option<String>,
    label_replaced: u64,
    transcript_replaced: u64,
}

/// The Voices page's store.
#[derive(uniffi::Object)]
pub struct VoicesStore {
    held: Mutex<Held>,
    observer: Arc<dyn VoicesObserver>,
    /// Whether a ticker is reading the microphone.
    ticking: AtomicBool,
}

#[uniffi::export]
impl VoicesStore {
    /// A store that tells `observer` what changes. Nothing is read until the
    /// app sends [`VoicesAction::Refresh`].
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn VoicesObserver>) -> Arc<Self> {
        Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
            ticking: AtomicBool::new(false),
        })
    }

    /// What the page shows now.
    pub fn page(&self) -> VoicesPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: VoicesAction) {
        if action == VoicesAction::Refresh {
            Arc::clone(&self).read_language();
        }
        let input = self.input(action);
        self.dispatch(input);
    }

    /// The file dialog was answered: the WAV at `path`, or nothing when the
    /// user closed it (`None`).
    pub fn file_picked(self: Arc<Self>, path: Option<String>) {
        let Some(path) = path.map(PathBuf::from) else {
            self.dispatch(Input::FileImported(None));
            return;
        };
        RUNTIME.spawn(async move {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let input = match tokio::fs::read(&path).await {
                Ok(wav) => Input::FileImported(Some((name, wav))),
                Err(e) => Input::ImportFailed(format!("could not read {name}: {e}")),
            };
            self.dispatch(input);
        });
    }
}

impl VoicesStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `action` as the page's logic takes it, with what the store knows: the
    /// loaded model a preview speaks in, the language a script is read in.
    fn input(&self, action: VoicesAction) -> Input {
        match action {
            VoicesAction::Refresh => Input::Refresh,
            VoicesAction::BeginRename { id, label } => Input::BeginRename { id, label },
            VoicesAction::RenameChanged { label } => Input::RenameChanged(label),
            VoicesAction::CancelRename => Input::CancelRename,
            VoicesAction::CommitRename => Input::CommitRename,
            VoicesAction::Delete { id } => Input::Delete(id),
            VoicesAction::Preview { voice_id } => Input::Preview {
                voice_id,
                model: self.lock().state.model.as_ref().map(|m| m.name.clone()),
            },
            // An id that names no script lands on the user's own words, as
            // the settings app's picker does.
            VoicesAction::ScriptSelected { id } => {
                Input::ScriptSelected(id.as_deref().and_then(scripts::find).map(|s| s.id))
            }
            VoicesAction::StartRecording => Input::StartRecording,
            VoicesAction::StopRecording => Input::StopRecording {
                language: self.lock().language.clone(),
            },
            VoicesAction::ImportFile => Input::ImportFile,
            VoicesAction::LabelChanged { label } => Input::LabelChanged(label),
            VoicesAction::TranscriptChanged { transcript } => Input::TranscriptChanged(transcript),
            VoicesAction::DiscardPending => Input::DiscardPending,
            VoicesAction::Save => Input::Save,
        }
    }

    /// Hand `input` to the page's logic, tell the app what changed, and start
    /// the work it asked for.
    fn dispatch(self: Arc<Self>, input: Input) {
        let typed_label = matches!(input, Input::LabelChanged(_));
        let typed_transcript = matches!(input, Input::TranscriptChanged(_));
        let (page, effects, recording) = {
            let mut held = self.lock();
            let before = (
                held.state.label_input.clone(),
                held.state.transcript_input.clone(),
            );
            let outcome = page::update(&mut held.state, input);
            // A field the logic changed, rather than the user typing in it,
            // is one the app's field takes.
            if !typed_label && held.state.label_input != before.0 {
                held.label_replaced += 1;
            }
            if !typed_transcript && held.state.transcript_input != before.1 {
                held.transcript_replaced += 1;
            }
            if outcome.clear_notice {
                held.notice = None;
            }
            if let Some(notice) = outcome.notice {
                held.notice = Some(notice.into());
            }
            let recording = held.state.is_recording();
            (snapshot(&held), outcome.effects, recording)
        };
        self.observer.changed(page);
        for effect in effects {
            Arc::clone(&self).run(effect);
        }
        if recording {
            self.tick_while_recording();
        }
    }

    /// Start `effect`. Its answer comes back through [`Self::dispatch`].
    fn run(self: Arc<Self>, effect: Effect) {
        match effect {
            Effect::List => self.answer(page::run_list()),
            Effect::Rename { id, label } => self.answer(page::run_rename(id, label)),
            Effect::Delete(id) => self.answer(page::run_delete(id)),
            Effect::Preview { model, voice_id } => self.answer(page::run_preview(model, voice_id)),
            Effect::Save {
                label,
                transcript,
                wav,
            } => self.answer(page::run_save(label, transcript, wav)),
            Effect::PickFile => self.observer.file_requested(),
            // Nothing else in this app lists the library's voices yet.
            Effect::LibraryChanged => {}
        }
    }

    /// Run `work` and hand its answer back to the page.
    fn answer(self: Arc<Self>, work: impl Future<Output = Input> + Send + 'static) {
        RUNTIME.spawn(async move {
            let input = work.await;
            self.dispatch(input);
        });
    }

    /// Read the language the user speaks, which picks the script suggested.
    fn read_language(self: Arc<Self>) {
        RUNTIME.spawn(async move {
            // A daemon that can't say suggests the default script, as one with
            // no language set does.
            let language = get_primary_language().await.ok().flatten();
            let page = {
                let mut held = self.lock();
                if held.language == language {
                    return;
                }
                held.language = language;
                snapshot(&held)
            };
            self.observer.changed(page);
        });
    }

    /// While the microphone is open, tell the page every [`TICK`], for its
    /// meter and time, and so it stops once the recorder has. One ticker at a
    /// time.
    fn tick_while_recording(self: Arc<Self>) {
        if self.ticking.swap(true, Ordering::AcqRel) {
            return;
        }
        RUNTIME.spawn(async move {
            loop {
                tokio::time::sleep(TICK).await;
                let language = {
                    let held = self.lock();
                    if !held.state.is_recording() {
                        break;
                    }
                    held.language.clone()
                };
                Arc::clone(&self).dispatch(Input::RecordingTick { language });
            }
            self.ticking.store(false, Ordering::Release);
        });
    }
}

fn snapshot(held: &Held) -> VoicesPage {
    let state = &held.state;
    let language = held.language.as_deref();
    let budget = state.clone_ref_seconds();
    let (renaming, rename_label) = state
        .renaming
        .clone()
        .map_or((None, String::new()), |(id, label)| (Some(id), label));
    VoicesPage {
        loaded: state.loaded,
        voices: state
            .voices
            .iter()
            .map(|v| VoiceRow::new(v, state))
            .collect(),
        model: state.model.as_ref().map(VoiceModel::from),
        language: held.language.clone(),
        scripts: scripts::offered(language)
            .into_iter()
            .map(|s| ScriptInfo::new(s, budget, language))
            .collect(),
        script: state
            .script(language)
            .map(|s| ScriptInfo::new(s, budget, language)),
        own_words: state.script == ScriptChoice::Free,
        recording: state.recorder.as_ref().map(|recorder| Recording {
            seconds: state.recording_seconds,
            level: state.recording_level,
            max_seconds: recorder.max_seconds(),
        }),
        pending: state.pending.as_ref().map(|p| PendingVoice {
            seconds: p.seconds,
            imported_from: match &p.origin {
                SampleOrigin::Recorded => None,
                SampleOrigin::Imported(name) => Some(name.clone()),
            },
            script: p.script.map(|s| ScriptInfo::new(s, budget, language)),
            saving: state.saving,
            can_save: state.can_save(),
        }),
        label: state.label_input.clone(),
        label_replaced: held.label_replaced,
        transcript: state.transcript_input.clone(),
        transcript_replaced: held.transcript_replaced,
        renaming,
        rename_label,
        previewing: state.previewing.clone(),
        notice: held.notice.clone(),
    }
}
