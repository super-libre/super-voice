// SPDX-License-Identifier: GPL-3.0-only
//! The Voices page: the library of cloned voices, and the recording or import
//! that fills it.
//!
//! Recording and importing converge on one pending sample as soon as the bytes
//! exist, so naming, validating and uploading are written once. The recorder
//! is held in the page's state for exactly as long as the user is recording:
//! it owns a device and a thread.

use super_voice_shared::tts::models::voices::{VoiceInfo, VoiceModelSupport};

use crate::audio::recorder::Recorder;
use crate::daemon::client::v1::speak::speak_in_voice;
use crate::daemon::client::v1::voices::{create_voice, delete_voice, list_voices, rename_voice};
use crate::events::Event;
use crate::state::scripts;
use crate::state::voices::{PendingSample, SampleOrigin, ScriptChoice, VoicesState};

/// The sentence a preview speaks. Fixed rather than the user's: the point is to
/// hear the voice, and a field to fill in first is friction between the user
/// and the only question they are asking.
pub const PREVIEW_TEXT: &str =
    "This is how the cloned voice sounds when it reads a sentence aloud.";

/// The longest recording the daemon's library stores, so the recorder stops
/// itself there rather than record audio the upload would refuse. Kept in step
/// with `super-voice-daemon`'s `voices::MAX_CLIP_SECONDS`.
pub const MAX_RECORDING_SECONDS: f32 = 120.0;

/// What happened on the page: something the user did, or the answer to work
/// an [`Effect`] started.
#[derive(Debug, Clone)]
pub enum Input {
    /// Read the library again.
    Refresh,
    /// The library was read: the voices, and what the loaded model can do
    /// with them.
    Loaded(Result<(Vec<VoiceInfo>, Option<VoiceModelSupport>), String>),

    /// Rename was pressed for a voice: edit its label, starting from `label`.
    BeginRename { id: String, label: String },
    /// The rename field now holds this.
    RenameChanged(String),
    /// The rename was called off.
    CancelRename,
    /// Save the rename.
    CommitRename,
    /// The rename was saved, or why not.
    Renamed(Result<VoiceInfo, String>),

    /// Delete was pressed for a voice: ask first.
    DeleteRequested(String),
    /// The delete was called off.
    DeleteCancelled,
    /// Delete a voice: the ask was answered yes.
    Delete(String),
    /// The voice was deleted, or why not.
    Deleted {
        id: String,
        result: Result<(), String>,
    },

    /// Hear a voice, spoken by `model`: the model text to speech speaks in,
    /// which the app knows. Nothing to preview with when it is `None`.
    Preview {
        voice_id: String,
        model: Option<String>,
    },
    /// The preview was spoken, or why not.
    Previewed(Result<(), String>),
    /// The daemon is getting a model ready to speak in this voice (its
    /// `preparing_voice` event), whoever asked.
    Preparing(String),
    /// It is done with this voice, ready or not (its `voice_prepared`
    /// event).
    Prepared(String),
    /// The daemon started speaking: whatever was being got ready is done,
    /// since a voice is ready before its utterance is taken.
    SpeechStarted,

    /// A script was picked to read, or none (the user's own words).
    ScriptSelected(Option<&'static str>),
    /// Open the microphone.
    StartRecording,
    /// A moment passed while recording. `language` is the user's primary
    /// language, for the script read, in case the recorder stopped itself.
    RecordingTick { language: Option<String> },
    /// Close the microphone and keep what it heard. `language` as for
    /// [`RecordingTick`](Self::RecordingTick).
    StopRecording { language: Option<String> },

    /// Import a recording from a file.
    ImportFile,
    /// The file picked: its name and bytes, or `None` when the user closed
    /// the picker.
    FileImported(Option<(String, Vec<u8>)>),
    /// The file could not be read.
    ImportFailed(String),

    /// The label field now holds this.
    LabelChanged(String),
    /// The transcript field now holds this.
    TranscriptChanged(String),
    /// Throw the pending sample away.
    DiscardPending,
    /// Save the pending sample as a voice.
    Save,
    /// It was saved, or why not.
    Saved(Result<VoiceInfo, String>),
}

/// Work [`update`] asks for. Each says which runner does it and what it
/// answers with.
#[derive(Debug, Clone)]
pub enum Effect {
    /// Read the library ([`run_list`]); answers [`Input::Loaded`].
    List,
    /// Rename a voice ([`run_rename`]); answers [`Input::Renamed`].
    Rename { id: String, label: String },
    /// Delete a voice ([`run_delete`]); answers [`Input::Deleted`].
    Delete(String),
    /// Speak [`PREVIEW_TEXT`] in a voice ([`run_preview`]); answers
    /// [`Input::Previewed`].
    Preview { model: String, voice_id: String },
    /// Ask the user for a WAV file; answers [`Input::FileImported`] or
    /// [`Input::ImportFailed`]. The dialog is the app's own.
    PickFile,
    /// Upload a recording as a voice ([`run_save`]); answers
    /// [`Input::Saved`].
    Save {
        label: String,
        transcript: Option<String>,
        wav: Vec<u8>,
    },
    /// The library changed. An app that lists a model's voices elsewhere (the
    /// app's Models card) reads them again.
    LibraryChanged,
}

/// Something to tell the user. Each app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The library could not be read.
    CouldNotLoad(String),
    /// The file could not be read.
    CouldNotImport(String),
    /// The voice could not be renamed.
    CouldNotRename(String),
    /// The voice could not be deleted.
    CouldNotDelete(String),
    /// The preview could not be spoken.
    CouldNotPreview(String),
    /// The microphone failed, or what it heard could not be kept.
    CouldNotRecord(String),
    /// The voice could not be saved.
    CouldNotSave(String),
    /// The loaded model clones from the recording's transcript, and there is
    /// none.
    NeedsTranscript,
}

/// What an [`update`] asks of the app.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The work to start, in order.
    pub effects: Vec<Effect>,
    /// Whether the page's notice is out of date and goes. Applied before
    /// [`notice`](Self::notice).
    pub clear_notice: bool,
    /// What to tell the user.
    pub notice: Option<Notice>,
}

/// What `input` does to the page.
pub fn update(state: &mut VoicesState, input: Input) -> Outcome {
    let mut out = Outcome::default();
    match input {
        Input::Refresh => out.effects.push(Effect::List),
        Input::Loaded(Ok((voices, model))) => {
            state.voices = voices;
            state.model = model;
            state.loaded = true;
            out.clear_notice = true;
        }
        Input::Loaded(Err(e)) => out.notice = Some(Notice::CouldNotLoad(e)),

        rename @ (Input::BeginRename { .. }
        | Input::RenameChanged(_)
        | Input::CancelRename
        | Input::CommitRename
        | Input::Renamed(_)) => renaming(state, rename, &mut out),

        library @ (Input::DeleteRequested(_)
        | Input::DeleteCancelled
        | Input::Delete(_)
        | Input::Deleted { .. }
        | Input::Preview { .. }
        | Input::Previewed(_)
        | Input::Preparing(_)
        | Input::Prepared(_)
        | Input::SpeechStarted) => library_work(state, library, &mut out),

        capture @ (Input::ScriptSelected(_)
        | Input::StartRecording
        | Input::RecordingTick { .. }
        | Input::StopRecording { .. }
        | Input::ImportFile
        | Input::FileImported(_)
        | Input::ImportFailed(_)) => capture_work(state, capture, &mut out),

        pending => pending_work(state, pending, &mut out),
    }
    out
}

/// Renaming a voice.
fn renaming(state: &mut VoicesState, input: Input, out: &mut Outcome) {
    match input {
        Input::BeginRename { id, label } => state.renaming = Some((id, label)),
        Input::RenameChanged(label) => {
            if let Some((_, current)) = state.renaming.as_mut() {
                *current = label;
            }
        }
        Input::CancelRename => state.renaming = None,
        // An empty field is a cancel rather than a request: the daemon refuses
        // an empty label, and a round trip that can only fail is worse than
        // doing nothing.
        Input::CommitRename => {
            if let Some((id, label)) = state.renaming.take() {
                let label = label.trim().to_string();
                if !label.is_empty() {
                    out.clear_notice = true;
                    out.effects.push(Effect::Rename { id, label });
                }
            }
        }
        Input::Renamed(Ok(voice)) => {
            if let Some(existing) = state.voices.iter_mut().find(|v| v.id == voice.id) {
                *existing = voice;
            }
            out.effects.push(Effect::LibraryChanged);
        }
        Input::Renamed(Err(e)) => out.notice = Some(Notice::CouldNotRename(e)),
        _ => {}
    }
}

/// Deleting a voice, and hearing one.
fn library_work(state: &mut VoicesState, input: Input, out: &mut Outcome) {
    match input {
        Input::DeleteRequested(id) => state.confirming_delete = Some(id),
        Input::DeleteCancelled => state.confirming_delete = None,
        // The row greys while the request is in flight.
        Input::Delete(id) => {
            state.confirming_delete = None;
            out.clear_notice = true;
            state.deleting.push(id.clone());
            out.effects.push(Effect::Delete(id));
        }
        Input::Deleted { id, result } => {
            state.deleting.retain(|d| d != &id);
            match result {
                Ok(()) => {
                    state.voices.retain(|v| v.id != id);
                    out.effects.push(Effect::LibraryChanged);
                }
                Err(e) => out.notice = Some(Notice::CouldNotDelete(e)),
            }
        }
        // One at a time: a second speak request while one is open cancels the
        // first rather than queueing, so a press during one is dropped.
        Input::Preview { voice_id, model } => {
            if state.previewing.is_some() {
                return;
            }
            let Some(model) = model else {
                log::debug!("voices: no model to preview {voice_id} with");
                return;
            };
            out.clear_notice = true;
            state.previewing = Some(voice_id.clone());
            out.effects.push(Effect::Preview { model, voice_id });
        }
        Input::Previewed(result) => {
            state.previewing = None;
            if let Err(e) = result {
                out.notice = Some(Notice::CouldNotPreview(e));
            }
        }
        // The wait is the same whoever started it, so the page shows it for
        // a preparation another client asked for too.
        Input::Preparing(voice) => state.preparing = Some(voice),
        // A failure is not said here: nothing the user did has failed yet,
        // and the next attempt to speak in the voice reports it.
        Input::Prepared(voice) => {
            if state.preparing.as_deref() == Some(voice.as_str()) {
                state.preparing = None;
            }
        }
        // The event is the speaker's, so another app's utterance clears the
        // wait too; that only ends a label early.
        Input::SpeechStarted => {
            state.previewing = None;
            state.preparing = None;
        }
        _ => {}
    }
}

/// What a daemon event means for the page: nothing, or the inputs to take in.
#[must_use]
pub fn heard(event: &Event) -> Vec<Input> {
    use super_voice_shared::tts::models::protocol::DaemonStatusEvent as SynthesisEvent;
    match event {
        Event::Speaking {
            is_speaking: true, ..
        } => vec![Input::SpeechStarted],
        // A voice was added, renamed or deleted, by this app or another.
        _ if crate::events::setting_changed(event) == Some("voices") => vec![Input::Refresh],
        Event::Status(status)
            if status.event_type == "daemon_status_changed"
                && status
                    .data
                    .get("feature")
                    .and_then(serde_json::Value::as_str)
                    == Some("tts") =>
        {
            match serde_json::from_value::<SynthesisEvent>(status.data.clone()) {
                Ok(SynthesisEvent::PreparingVoice { voice, .. }) => vec![Input::Preparing(voice)],
                Ok(SynthesisEvent::VoicePrepared {
                    voice,
                    error,
                    model,
                }) => {
                    if let Some(e) = error {
                        log::warn!("the daemon could not prepare voice {voice} for {model}: {e}");
                    }
                    vec![Input::Prepared(voice)]
                }
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

/// Adding a voice: the microphone, and the file picker.
fn capture_work(state: &mut VoicesState, input: Input, out: &mut Outcome) {
    match input {
        // An id that no longer names a script lands on the user's own words,
        // as the picker's own row does.
        Input::ScriptSelected(id) => {
            state.script = match id.and_then(scripts::find) {
                Some(script) => ScriptChoice::Chosen(script),
                None => ScriptChoice::Free,
            };
        }
        // Any pending sample goes first: one at a time keeps "what am I about
        // to save" unambiguous.
        Input::StartRecording => {
            if state.is_recording() {
                return;
            }
            out.clear_notice = true;
            state.clear_pending();
            state.recording_seconds = 0.0;
            state.recording_level = 0.0;
            state.recorder = Some(Recorder::start(MAX_RECORDING_SECONDS));
        }
        Input::RecordingTick { language } => {
            let Some(recorder) = state.recorder.as_ref() else {
                return;
            };
            let status = recorder.status();
            state.recording_seconds = status.seconds;
            state.recording_level = status.level;
            // The limit was reached, and the audio is worth keeping; or the
            // device failed, which stopping reports.
            if status.finished {
                stop_recording(state, language.as_deref(), out);
            }
        }
        Input::StopRecording { language } => stop_recording(state, language.as_deref(), out),
        Input::ImportFile => {
            out.clear_notice = true;
            out.effects.push(Effect::PickFile);
        }
        Input::FileImported(Some((name, wav))) => {
            let seconds = wav_seconds(&wav);
            // The file's name is nearly always what the user would have
            // typed, and one field fewer between them and a saved voice.
            if state.label_input.trim().is_empty() {
                state.label_input = label_from_filename(&name);
            }
            state.pending = Some(PendingSample {
                wav,
                seconds,
                origin: SampleOrigin::Imported(name),
                // Never a script's: nobody knows what words are in a file
                // chosen off disk.
                script: None,
            });
        }
        Input::ImportFailed(e) => out.notice = Some(Notice::CouldNotImport(e)),
        // `FileImported(None)`: the picker was closed, and there is nothing
        // to say.
        _ => {}
    }
}

/// Naming, transcribing, discarding and saving the pending sample.
fn pending_work(state: &mut VoicesState, input: Input, out: &mut Outcome) {
    match input {
        Input::LabelChanged(label) => state.label_input = label,
        // Kept out for a recording read from a built-in script: those words
        // are a fact about the audio, not a field to type over.
        Input::TranscriptChanged(transcript) => {
            if state
                .pending
                .as_ref()
                .is_none_or(|pending| pending.script.is_none())
            {
                state.transcript_input = transcript;
            }
        }
        Input::DiscardPending => {
            state.clear_pending();
            out.clear_notice = true;
        }
        Input::Save => save(state, out),
        Input::Saved(Ok(voice)) => {
            log::info!("stored cloned voice {} ({})", voice.label, voice.voice_id);
            state.saving = false;
            state.clear_pending();
            out.clear_notice = true;
            // The library is listed again rather than the voice inserted: the
            // daemon owns the order.
            out.effects.push(Effect::List);
            out.effects.push(Effect::LibraryChanged);
        }
        Input::Saved(Err(e)) => {
            state.saving = false;
            out.notice = Some(Notice::CouldNotSave(e));
        }
        _ => {}
    }
}

/// Close the microphone and turn what it heard into the pending sample.
fn stop_recording(state: &mut VoicesState, language: Option<&str>, out: &mut Outcome) {
    let Some(recorder) = state.recorder.take() else {
        return;
    };
    state.recording_level = 0.0;
    let recording = match recorder.finish() {
        Ok(recording) => recording,
        Err(e) => {
            state.recording_seconds = 0.0;
            out.notice = Some(Notice::CouldNotRecord(e));
            return;
        }
    };
    let seconds = recording.seconds();
    match recording.to_wav() {
        Ok(wav) => {
            // A script just read is a transcript already written: filled in,
            // rather than asking for words that were on screen a second ago.
            let script = state.script(language);
            if let Some(script) = script {
                state.transcript_input = script.text.to_string();
            }
            state.pending = Some(PendingSample {
                wav,
                seconds,
                origin: SampleOrigin::Recorded,
                script,
            });
        }
        Err(e) => out.notice = Some(Notice::CouldNotRecord(e)),
    }
    state.recording_seconds = 0.0;
}

/// Upload the pending sample, when it has a label, and a transcript for a
/// model that needs one.
fn save(state: &mut VoicesState, out: &mut Outcome) {
    let label = state.label_input.trim().to_string();
    if state.pending.is_none() || label.is_empty() {
        return;
    }
    let transcript = {
        let t = state.transcript_input.trim();
        (!t.is_empty()).then(|| t.to_string())
    };
    // The same rule Save's button keeps: a model that clones from the
    // transcript can't use a recording whose words it wasn't given, and
    // saying so beats a refusal after the upload.
    if state.needs_transcript() && transcript.is_none() {
        out.notice = Some(Notice::NeedsTranscript);
        return;
    }
    // Cloned, not taken: a failed upload leaves the sample, so Save can be
    // pressed again rather than recording again.
    let Some(pending) = state.pending.clone() else {
        return;
    };
    state.saving = true;
    out.clear_notice = true;
    out.effects.push(Effect::Save {
        label,
        transcript,
        wav: pending.wav,
    });
}

/// Read the library and what the loaded model can do with it.
pub async fn run_list() -> Input {
    Input::Loaded(list_voices().await.map_err(|e| e.to_string()))
}

/// Rename a voice.
pub async fn run_rename(id: String, label: String) -> Input {
    Input::Renamed(rename_voice(id, label).await.map_err(|e| e.to_string()))
}

/// Delete a voice.
pub async fn run_delete(id: String) -> Input {
    let result = delete_voice(id.clone())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string());
    Input::Deleted { id, result }
}

/// Speak [`PREVIEW_TEXT`] in a voice, through `model`. Hearing a voice selects
/// it for the model: a speak request carries no voice of its own.
pub async fn run_preview(model: String, voice_id: String) -> Input {
    Input::Previewed(
        speak_in_voice(model, voice_id, PREVIEW_TEXT.to_string())
            .await
            .map_err(|e| e.to_string()),
    )
}

/// Upload a recording as a voice.
pub async fn run_save(label: String, transcript: Option<String>, wav: Vec<u8>) -> Input {
    Input::Saved(
        create_voice(label, transcript, wav)
            .await
            .map_err(|e| e.to_string()),
    )
}

/// A file's name as a plausible voice label: the stem, with separators as
/// spaces.
#[must_use]
pub fn label_from_filename(name: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    stem.replace(['_', '-'], " ").trim().to_string()
}

/// A WAV file's length in seconds, from its header. Zero when the bytes are
/// not a readable WAV: the length then says nothing rather than guess, and the
/// daemon is the one that refuses the upload.
#[must_use]
pub fn wav_seconds(wav: &[u8]) -> f32 {
    let Ok(reader) = hound::WavReader::new(std::io::Cursor::new(wav)) else {
        return 0.0;
    };
    let rate = reader.spec().sample_rate;
    if rate == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let (frames, rate) = (reader.duration() as f32, rate as f32);
    frames / rate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(seconds: f32) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut w = hound::WavWriter::new(&mut cursor, spec).expect("a writer");
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            for _ in 0..(seconds * 16_000.0) as usize {
                w.write_sample(0_i16).expect("a sample");
            }
            w.finalize().expect("finished");
        }
        cursor.into_inner()
    }

    #[test]
    fn a_filename_becomes_a_plausible_label() {
        assert_eq!(label_from_filename("ada_lovelace.wav"), "ada lovelace");
        assert_eq!(label_from_filename("my-voice.WAV"), "my voice");
        assert_eq!(label_from_filename("Ada"), "Ada");
    }

    #[test]
    fn a_wav_header_gives_the_length() {
        assert!((wav_seconds(&wav(0.5)) - 0.5).abs() < 1e-3);
        assert!((wav_seconds(b"not audio") - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn an_imported_file_is_pending_and_names_itself() {
        let mut state = VoicesState::default();
        update(
            &mut state,
            Input::FileImported(Some(("grace_hopper.wav".into(), wav(2.0)))),
        );
        let pending = state.pending.as_ref().expect("pending");
        assert!((pending.seconds - 2.0).abs() < 1e-3);
        assert!(pending.script.is_none());
        assert_eq!(state.label_input, "grace hopper");
    }

    #[test]
    fn save_needs_a_label_and_keeps_the_sample_until_it_lands() {
        let mut state = VoicesState::default();
        update(
            &mut state,
            Input::FileImported(Some(("x.wav".into(), wav(1.0)))),
        );
        update(&mut state, Input::LabelChanged("  ".into()));
        assert!(update(&mut state, Input::Save).effects.is_empty());

        update(&mut state, Input::LabelChanged("Grace".into()));
        let out = update(&mut state, Input::Save);
        assert!(matches!(out.effects.as_slice(), [Effect::Save { label, .. }] if label == "Grace"));
        assert!(state.saving && state.pending.is_some());

        let out = update(&mut state, Input::Saved(Err("too long".into())));
        assert_eq!(out.notice, Some(Notice::CouldNotSave("too long".into())));
        assert!(!state.saving && state.pending.is_some());
    }

    #[test]
    fn an_empty_rename_is_a_cancel() {
        let mut state = VoicesState::default();
        update(
            &mut state,
            Input::BeginRename {
                id: "v".into(),
                label: "Ada".into(),
            },
        );
        update(&mut state, Input::RenameChanged("   ".into()));
        assert!(update(&mut state, Input::CommitRename).effects.is_empty());
        assert!(state.renaming.is_none());
    }

    #[test]
    fn one_preview_at_a_time_and_none_without_a_model() {
        let mut state = VoicesState::default();
        let none = update(
            &mut state,
            Input::Preview {
                voice_id: "a".into(),
                model: None,
            },
        );
        assert!(none.effects.is_empty() && state.previewing.is_none());

        update(
            &mut state,
            Input::Preview {
                voice_id: "a".into(),
                model: Some("m".into()),
            },
        );
        let second = update(
            &mut state,
            Input::Preview {
                voice_id: "b".into(),
                model: Some("m".into()),
            },
        );
        assert!(second.effects.is_empty());
        assert_eq!(state.previewing.as_deref(), Some("a"));
    }

    #[test]
    fn a_deleted_voice_leaves_and_the_library_changed() {
        let mut state = VoicesState::default();
        let out = update(&mut state, Input::Delete("v".into()));
        assert!(matches!(out.effects.as_slice(), [Effect::Delete(id)] if id == "v"));
        assert_eq!(state.deleting, ["v"]);
        let out = update(
            &mut state,
            Input::Deleted {
                id: "v".into(),
                result: Ok(()),
            },
        );
        assert!(state.deleting.is_empty());
        assert!(matches!(out.effects.as_slice(), [Effect::LibraryChanged]));
    }

    fn status(data: serde_json::Value) -> Event {
        Event::Status(super_voice_shared::models::protocol::NotificationEvent {
            event_type_field: "daemon_status_changed".into(),
            event_type: "daemon_status_changed".into(),
            client_id: "daemon".into(),
            timestamp: String::new(),
            data,
        })
    }

    #[test]
    fn a_delete_asks_first() {
        let mut state = VoicesState::default();
        let out = update(&mut state, Input::DeleteRequested("v".into()));
        assert!(out.effects.is_empty());
        assert_eq!(state.confirming_delete.as_deref(), Some("v"));
        update(&mut state, Input::DeleteCancelled);
        assert!(state.confirming_delete.is_none());

        update(&mut state, Input::DeleteRequested("v".into()));
        let out = update(&mut state, Input::Delete("v".into()));
        assert!(matches!(out.effects.as_slice(), [Effect::Delete(id)] if id == "v"));
        assert!(state.confirming_delete.is_none());
    }

    #[test]
    fn a_preparation_shows_until_the_daemon_is_done_or_speaks() {
        let preparing = status(serde_json::json!({
            "feature": "tts", "status": "preparing_voice",
            "voice": "voice:a", "model": "qwen3-tts-0.6b-base",
        }));
        let mut state = VoicesState::default();
        for input in heard(&preparing) {
            update(&mut state, input);
        }
        assert!(state.is_busy("voice:a"));

        let prepared = status(serde_json::json!({
            "feature": "tts", "status": "voice_prepared",
            "voice": "voice:a", "model": "qwen3-tts-0.6b-base",
        }));
        for input in heard(&prepared) {
            update(&mut state, input);
        }
        assert!(!state.any_busy());

        state.preparing = Some("voice:b".into());
        state.previewing = Some("voice:b".into());
        let speaking = Event::Speaking {
            is_speaking: true,
            utterance_id: None,
        };
        for input in heard(&speaking) {
            update(&mut state, input);
        }
        assert!(!state.any_busy());
    }

    #[test]
    fn a_library_changed_elsewhere_is_read_again() {
        let changed = status(serde_json::json!({
            "feature": "tts", "status": "settings_changed", "setting": "voices",
        }));
        assert!(matches!(heard(&changed).as_slice(), [Input::Refresh]));
        let other = status(serde_json::json!({
            "feature": "tts", "status": "settings_changed", "setting": "volume",
        }));
        assert!(heard(&other).is_empty());
    }
}
