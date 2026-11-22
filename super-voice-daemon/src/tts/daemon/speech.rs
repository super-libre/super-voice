// SPDX-License-Identifier: GPL-3.0-only
//! The speak path: text in, audio out of the speakers.
//!
//! Sits between the `/speak` endpoint and the two halves already built — the
//! backend's `POST /v1/synthesize` ([`crate::tts::tts_models::v1`]) and the playback
//! pipeline ([`crate::tts::audio::playback`]) — and owns the small amount of state
//! that belongs to neither: which utterance is current, and how to stop it.
//!
//! **A channel bridges the sink and the ring.** [`SynthesisSink::on_frame`] is
//! synchronous (it is called from inside the frame decoder), while
//! [`Playback::push`] is async because it waits for ring space. So the sink
//! decodes samples and hands them to a pump task over a channel, and the pump
//! does the awaiting. This is what makes playback start while the backend is
//! still producing, rather than after it finishes.
//!
//! **The audio device is opened lazily, on a dedicated thread.** cpal's stream
//! is `!Send` on several backends, so it cannot live in the daemon's shared
//! state alongside everything the async tasks touch. The thread opens the
//! device, sends the [`Playback`] producer half back, and parks holding the
//! stream. Opening lazily rather than at startup keeps a daemon that never
//! speaks from claiming an output device it will not use.
//!
//! **One utterance at a time, newest wins.** A second `/speak` cancels the
//! first rather than queueing behind it. That is the v1 policy, not the end
//! state: every utterance already carries an id, so a real queue with priority
//! and ducking can be added without changing the endpoint's shape.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use super_voice_shared::tts::models::protocol::DaemonStatusEvent;

use anyhow::{Result, anyhow, bail};
use log::{debug, info, warn};
use parking_lot::Mutex;
use super_voice_shared::tts::audio::frames::{AudioParams, Frame, FrameKind};
use tokio::sync::{mpsc, oneshot};

use crate::tts::audio::playback::{DeviceFormat, Playback, mixer};
use crate::tts::daemon::types::SharedLoadedModel;
use crate::tts::text::chunk::{ChunkPolicy, Chunker};
use crate::tts::tts_models::v1::{SynthesisSink, SynthesizeRequest};
use super_engine_daemon::stage::StageExt;

/// Longest `text` accepted when the model declares no `max_input_chars`.
///
/// A bound has to exist regardless of the manifest: `text` arrives from an
/// authorized client, and an unbounded one would hold a backend and the output
/// device for as long as it liked.
pub const MAX_TEXT_CHARS: usize = 50_000;

/// FFT window for the playback visualizer. Matches what the capture path used,
/// so the applet renders speech the same way it rendered a microphone.
const ANALYSIS_WINDOW: usize = 1024;

/// How long the playout watcher tolerates a device that has stopped consuming
/// before it declares the utterance over anyway.
///
/// Generous, because the honest reason for a pause is a slow sink rather than a
/// dead one, and cutting `speaking_state` short is the bug this whole path
/// exists to fix. Bounded, because the utterance slot is what `/status` reports
/// as busy and what uninstalling a backend waits on: a sink that went away must
/// not hold the daemon busy forever.
const PLAYOUT_STALL_TIMEOUT: Duration = Duration::from_secs(5);

/// How often the visualizer publisher looks at the playback position.
///
/// Sets the jitter on a band frame's arrival, so it wants to be well under a
/// video frame; it costs one uncontended lock per tick.
const BAND_TICK: Duration = Duration::from_millis(10);

/// How often `speech_progress` is republished, from the start of an utterance
/// to the end of its playout.
///
/// `docs/protocol/endpoints/v1/events.md` promises progress "repeatedly, as
/// audio is rendered", and publishing as synthesis goes cannot deliver that.
/// One-shot text is synthesized in a single `push`, so the first chunk plays
/// for as long as all the others take to synthesize; and a backend faster than
/// realtime is done producing before the first word is heard. Four a second is
/// smooth enough for a progress bar and far below what the SSE stream or the
/// ring's lock would notice.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// A running or finished utterance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utterance {
    /// Opaque id the client cancels or correlates events by.
    pub id: String,
    /// Alignment marks the backend emitted.
    pub marks: usize,
    /// How many synthesis chunks the text was split into.
    pub chunks: usize,
}

/// How an utterance should be spoken, apart from the text itself.
///
/// Grouped so the per-chunk loop can carry them without a closure: a closure
/// returning `SynthesizeRequest<'_>` ties the request's lifetime to the chunk,
/// which does not outlive the borrow of these.
#[derive(Debug, Default, Clone, Copy)]
pub struct SpeakOptions<'a> {
    /// A `voice` id the model declares, or `None` for its `default_voice`.
    pub voice: Option<&'a str>,
    /// Resolved BCP-47 tag.
    pub language: Option<&'a str>,
}

impl SpeakOptions<'_> {
    /// Copy into owned strings so a session can outlive the request that
    /// started it.
    fn into_owned(self) -> OwnedSpeakOptions {
        OwnedSpeakOptions {
            voice: self.voice.map(str::to_owned),
            language: self.language.map(str::to_owned),
        }
    }
}

/// [`SpeakOptions`] with owned strings, held by a [`SpeakSession`].
#[derive(Debug, Default, Clone)]
struct OwnedSpeakOptions {
    voice: Option<String>,
    language: Option<String>,
}

impl OwnedSpeakOptions {
    fn as_ref(&self) -> SpeakOptions<'_> {
        SpeakOptions {
            voice: self.voice.as_deref(),
            language: self.language.as_deref(),
        }
    }
}

/// Something worth telling a streaming client about, as it happens.
#[derive(Debug, Clone, PartialEq)]
pub enum SpeechEvent {
    /// The backend aligned a span of audio to a span of the text.
    Mark(super_voice_shared::tts::audio::frames::Mark),
}

/// Refuse text that cannot be spoken: empty, longer than [`MAX_TEXT_CHARS`],
/// or entirely markup. Needs no model, so a request can be refused before one
/// is loaded for it.
///
/// # Errors
/// [`SpeakError::EmptyText`] or [`SpeakError::TextTooLong`].
pub(crate) fn check_text(text: &str) -> Result<(), SpeakError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(SpeakError::EmptyText);
    }
    if trimmed.chars().count() > MAX_TEXT_CHARS {
        return Err(SpeakError::TextTooLong);
    }
    if crate::tts::text::normalize::normalize(trimmed).is_empty() {
        // The input was entirely markup. Nothing to say is not a failure, but
        // it is not an utterance either.
        return Err(SpeakError::EmptyText);
    }
    Ok(())
}

/// Why a speak request could not be served.
#[derive(Debug, thiserror::Error)]
pub enum SpeakError {
    /// `text` was empty or whitespace only.
    #[error("empty_text")]
    EmptyText,
    /// `text` exceeded [`MAX_TEXT_CHARS`].
    #[error("text_too_long")]
    TextTooLong,
    /// No model is loaded.
    #[error("model_not_loaded")]
    NotLoaded,
    /// The `voice` id is not one the model can resolve: either a shape it did
    /// not opt into, or a preset it does not declare.
    #[error("unknown_voice: {0}")]
    UnknownVoice(String),
    /// The audio device could not be opened.
    #[error("audio_device_unavailable: {0}")]
    Device(String),
    /// The backend failed.
    #[error("synthesis_failed: {0}")]
    Synthesis(String),
    /// The selected model, named here, speaks only in a voice the user
    /// chooses, and none is chosen. See
    /// [`voice_still_to_choose`](crate::tts::daemon::types::SuperTTSDaemon::voice_still_to_choose).
    #[error("voice_required: {0} speaks only in a voice the user chooses, and none is chosen")]
    VoiceRequired(String),
    /// A request that arrived after this one started speaking first, or a stop
    /// came after it, while this one still waited — for the model to load,
    /// usually. Newest wins by arrival, not by who got through the wait first.
    #[error("superseded: a newer request to speak, or a stop, came while this one waited")]
    Superseded,
    /// A recording is capturing, and the microphone would hear the speech.
    #[error(
        "recording_in_progress: a recording is capturing, and the microphone would hear the speech"
    )]
    RecordingInProgress,
}

/// What the sink hands the pump.
///
/// A chunk boundary travels through the same channel as the audio rather than
/// being applied directly, so it stays *ordered* with the samples around it.
/// Calling `end_chunk` from the sink would fade a seam into whatever the pump
/// happened to be pushing at that moment.
enum PcmMsg {
    /// Decoded samples, with the format they were decoded in.
    Samples(Vec<f32>, AudioParams),
    /// The synthesis response ended; the next one is a new prosodic unit.
    Boundary,
}

/// Decodes frames and forwards samples to the pump task.
///
/// Holds back any trailing partial sample: a backend may end a frame mid-sample
/// (frame boundaries are its choice, and nothing requires them to be
/// sample-aligned), so the straddling bytes belong to whatever comes next.
struct PlaybackSink {
    tx: mpsc::UnboundedSender<PcmMsg>,
    params: Option<AudioParams>,
    /// Undecoded bytes: a partial sample carried across frames.
    leftover: Vec<u8>,
    marks: usize,
    cancelled: Arc<AtomicBool>,
    /// Marks are forwarded as they decode so a streaming client can highlight
    /// along with the audio; the one-shot path simply drops them.
    events: mpsc::UnboundedSender<SpeechEvent>,
}

impl PlaybackSink {
    /// Bytes per interleaved frame in the declared format.
    fn frame_width(params: AudioParams) -> usize {
        (params.format.bytes_per_sample() * params.channels.max(1) as usize).max(1)
    }

    /// Close the current synthesis chunk.
    ///
    /// Any straddling bytes are dropped: a well-formed response ends on a whole
    /// sample, and carrying a fragment into the next response — which may not
    /// even share a format — would shift every sample after it by a byte.
    fn boundary(&mut self) {
        self.leftover.clear();
        self.params = None;
        let _ = self.tx.send(PcmMsg::Boundary);
    }
}

impl SynthesisSink for PlaybackSink {
    fn on_params(&mut self, params: AudioParams) -> Result<()> {
        self.params = Some(params);
        Ok(())
    }

    fn on_frame(&mut self, frame: Frame) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            // An error is how the sink tells the pump to stop reading the
            // backend. The caller maps it back to "cancelled", not a failure.
            bail!("cancelled");
        }
        match frame.kind {
            FrameKind::Audio => {
                let Some(params) = self.params else {
                    bail!("audio frame before response headers were read");
                };
                self.leftover.extend_from_slice(&frame.payload);
                let width = Self::frame_width(params);
                let usable = self.leftover.len() - (self.leftover.len() % width);
                if usable > 0 {
                    let bytes: Vec<u8> = self.leftover.drain(..usable).collect();
                    let samples = params.format.decode(&bytes);
                    // Unbounded, but bounded in practice by the frame decoder's
                    // MAX_TOTAL_AUDIO_BYTES cap on one response. Real
                    // backpressure lives at the ring, which the pump awaits.
                    let _ = self.tx.send(PcmMsg::Samples(samples, params));
                }
            }
            FrameKind::Mark => {
                self.marks += 1;
                if let Ok(mark) = serde_json::from_slice(&frame.payload) {
                    let _ = self.events.send(SpeechEvent::Mark(mark));
                }
            }
            FrameKind::Done | FrameKind::Error => {}
        }
        Ok(())
    }
}

/// The utterance currently in flight: its id and its cancel flag.
///
/// Shared with the live [`SpeakSession`] so the session can release the slot
/// when it ends, and so a cancel arriving from anywhere reaches the flag the
/// session is checking.
type CurrentSlot = Arc<Mutex<Option<(String, Arc<AtomicBool>)>>>;

/// Owns the output device and the current utterance.
pub struct SpeechEngine {
    /// `None` until the first utterance opens the device.
    playback: tokio::sync::Mutex<Option<Arc<Playback>>>,
    /// The in-flight utterance, if any.
    current: CurrentSlot,
    /// Overrides the real device; set by tests to run with no audio hardware.
    test_format: Option<DeviceFormat>,
    /// Where `speaking_state`, `speech_progress`, and `frequency_bands` go.
    /// `None` in tests that only inspect the ring.
    events: Option<Arc<crate::tts::daemon::events::EventBus>>,
    /// Reference clips for cloned voices, resolved when a `voice:<uuid>` id
    /// reaches the speak path. `None` in tests that never clone.
    voices: Option<Arc<crate::tts::voices::VoiceLibrary>>,
    /// The last ticket [`arrival`](Self::arrival) handed out.
    arrivals: AtomicU64,
    /// The newest ticket that started speaking, or that a stop overruled.
    /// Written only under the [`current`](Self::current) lock, so a check
    /// against it and the slot claim it guards are one step.
    newest_started: AtomicU64,
    /// Requests announced before they claim the slot, by ticket: the id each
    /// will speak under. See [`announce`](Self::announce). Locked after
    /// [`current`](Self::current) wherever both are held.
    announced: Mutex<std::collections::BTreeMap<u64, String>>,
    /// How many [`synthesize_wav`](Self::synthesize_wav) calls are running.
    /// They use the model without claiming the utterance slot, and the model
    /// must not be put to sleep under them.
    synthesizing: Arc<AtomicUsize>,
}

impl std::fmt::Debug for SpeechEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeechEngine")
            .field("current", &self.current())
            .finish_non_exhaustive()
    }
}

impl Default for SpeechEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Make sure a `voice:<uuid>` id has been registered with the loaded backend,
/// pushing its reference clip the first time it is used: the engine's
/// [`SpeechEngine::ensure_cloned_voice`], and a session's again when a backend
/// has lost a voice.
async fn ensure_registered(
    voices: Option<&Arc<crate::tts::voices::VoiceLibrary>>,
    events: Option<&Arc<crate::tts::daemon::events::EventBus>>,
    loaded: &crate::tts::daemon::types::LoadedModel,
    voice: &str,
) -> Result<(), SpeakError> {
    let Some(id) = crate::tts::voices::strip_prefix(voice) else {
        return Ok(());
    };
    if loaded.cloned_voices.lock().contains(voice) {
        return Ok(());
    }
    let Some(library) = voices.cloned() else {
        return Err(SpeakError::UnknownVoice(
            "this daemon has no voice library".into(),
        ));
    };

    let record = library
        .get(id)
        .map_err(|e| SpeakError::UnknownVoice(format!("voice '{voice}': {e}")))?;
    // Checked here rather than at synthesis so the user is told which
    // half is missing, once, instead of watching every request fail.
    if loaded.definition.product.clone_needs_transcript && record.transcript.is_none() {
        return Err(SpeakError::UnknownVoice(format!(
            "model '{}' clones from a reference transcript, and voice '{}' was stored without one",
            loaded.definition.name, record.label
        )));
    }

    // Decoding and trimming the clip is CPU work on a file; it does not
    // belong on the runtime thread that is about to pump audio.
    let clip = {
        let library = Arc::clone(&library);
        let id = id.to_string();
        let budget = loaded.definition.product.clone_ref_seconds;
        tokio::task::spawn_blocking(move || library.clip_pcm(&id, budget))
            .await
            .map_err(|e| SpeakError::Synthesis(format!("reading the reference clip: {e}")))?
            .map_err(|e| SpeakError::UnknownVoice(format!("voice '{voice}': {e}")))?
    };

    info!(
        "registering cloned voice {voice} ({:.1}s) with {}",
        clip.seconds, loaded.definition.name
    );
    // Announced on `/events` around the call, not just logged: the backend
    // has to derive an embedding — and, for a clip with a transcript, the
    // codec codes of an in-context example — from a clip length it may
    // never have encoded, which is GPU work measured in seconds. Every
    // client that can speak in a cloned voice can be left waiting on it,
    // and a pause nobody announced is indistinguishable from a hang.
    publish_voice_status(
        events,
        DaemonStatusEvent::PreparingVoice {
            voice: voice.to_string(),
            model: loaded.definition.name.clone(),
        },
    );
    let registered = loaded
        .instance
        .register_voice(&crate::tts::tts_models::v1::RegisterVoiceRequest {
            voice,
            transcript: record.transcript.as_deref(),
            sample_rate: crate::tts::voices::CLIP_SAMPLE_RATE,
            channels: crate::tts::voices::CLIP_CHANNELS,
            format: crate::tts::voices::CLIP_FORMAT,
            pcm: &clip.pcm,
        })
        .await
        .map_err(|e| SpeakError::Synthesis(format!("registering voice '{voice}': {e}")));
    publish_voice_status(
        events,
        DaemonStatusEvent::VoicePrepared {
            voice: voice.to_string(),
            model: loaded.definition.name.clone(),
            error: registered.as_ref().err().map(ToString::to_string),
        },
    );
    registered?;
    loaded.cloned_voices.lock().insert(voice.to_string());
    Ok(())
}

/// Put one voice-preparation event on the bus, when there is one.
///
/// A detached engine — the one the tests build — has no bus, and preparing a
/// voice must work the same without it.
fn publish_voice_status(
    events: Option<&Arc<crate::tts::daemon::events::EventBus>>,
    event: DaemonStatusEvent,
) {
    if let Some(events) = events {
        events.publish_synthesis_status(event);
    }
}

/// What [`SpeechEngine::claim`] took the slot with.
struct Claimed {
    id: String,
    flag: Arc<AtomicBool>,
    /// Whether [`SpeechEngine::announce`] already said it started.
    announced: bool,
}

/// A request announced with [`SpeechEngine::announce`]. Dropped before the
/// request claims the slot, it withdraws the announcement.
#[must_use = "dropping the announcement withdraws it at once"]
pub struct Announcement<'a> {
    engine: &'a SpeechEngine,
    ticket: u64,
}

impl Drop for Announcement<'_> {
    fn drop(&mut self) {
        self.engine.withdraw(self.ticket);
    }
}

/// Releases a [`SpeechEngine::claim_for_test`] claim on drop, so a test cannot
/// leave the engine looking busy for whatever runs next in the same process.
#[cfg(test)]
pub(crate) struct ClaimGuard<'a> {
    engine: &'a SpeechEngine,
}

#[cfg(test)]
impl Drop for ClaimGuard<'_> {
    fn drop(&mut self) {
        *self.engine.current.lock() = None;
    }
}

impl SpeechEngine {
    /// An engine that opens the default output device on first use.
    #[must_use]
    pub fn new() -> Self {
        Self {
            playback: tokio::sync::Mutex::new(None),
            current: CurrentSlot::default(),
            test_format: None,
            events: None,
            voices: None,
            arrivals: AtomicU64::new(0),
            newest_started: AtomicU64::new(0),
            announced: Mutex::default(),
            synthesizing: Arc::default(),
        }
    }

    /// An engine backed by a detached (null-sink) pipeline at `format`.
    ///
    /// Exercises every step of the speak path except the cpal binding, so the
    /// endpoint can be tested on a machine with no audio device.
    #[must_use]
    pub fn detached(format: DeviceFormat) -> Self {
        Self {
            playback: tokio::sync::Mutex::new(None),
            current: CurrentSlot::default(),
            test_format: Some(format),
            events: None,
            voices: None,
            arrivals: AtomicU64::new(0),
            newest_started: AtomicU64::new(0),
            announced: Mutex::default(),
            synthesizing: Arc::default(),
        }
    }

    /// Publish playback events to `bus`.
    ///
    /// Separate from construction because the daemon builds the engine and the
    /// bus independently; without a bus the engine still speaks, it is just
    /// silent on `/events`.
    #[must_use]
    pub fn with_events(mut self, bus: Arc<crate::tts::daemon::events::EventBus>) -> Self {
        self.events = Some(bus);
        self
    }

    /// Resolve cloned voices against `library`.
    ///
    /// Separate from construction for the same reason [`Self::with_events`]
    /// is: the daemon builds the engine and the library independently, and an
    /// engine without one still speaks — it just cannot resolve a
    /// `voice:<uuid>` id.
    #[must_use]
    pub fn with_voices(mut self, library: Arc<crate::tts::voices::VoiceLibrary>) -> Self {
        self.voices = Some(library);
        self
    }

    /// Whether a [`synthesize_wav`](Self::synthesize_wav) is running.
    #[must_use]
    pub fn is_synthesizing(&self) -> bool {
        self.synthesizing.load(Ordering::SeqCst) > 0
    }

    /// The id of the utterance currently in flight, if any.
    #[must_use]
    pub fn current(&self) -> Option<String> {
        self.current.lock().as_ref().map(|(id, _)| id.clone())
    }

    /// Claim the in-flight slot without synthesizing anything, so a test can
    /// put the daemon into the "model is occupied" state that `/status`,
    /// uninstall and the model-change handlers read. Returns a guard that
    /// releases the slot when dropped.
    ///
    /// Exists because [`SuperTTSDaemon::is_busy`] reads this slot rather than a
    /// mirrored flag: with no way to claim it, every busy-gated test would have
    /// to drive a real synthesis to assert a guard that has nothing to do with
    /// audio.
    ///
    /// [`SuperTTSDaemon::is_busy`]: crate::tts::daemon::types::SuperTTSDaemon::is_busy
    #[cfg(test)]
    pub(crate) fn claim_for_test(&self, id: &str) -> ClaimGuard<'_> {
        *self.current.lock() = Some((id.to_string(), Arc::new(AtomicBool::new(false))));
        ClaimGuard { engine: self }
    }

    /// Whether a request holding `ticket` would now be refused the slot:
    /// something newer has started, or a stop came after it.
    #[cfg(test)]
    pub(crate) fn is_overruled(&self, ticket: u64) -> bool {
        self.newest_started.load(Ordering::SeqCst) > ticket
    }

    /// The playback handle, for tests that inspect what was rendered.
    pub async fn playback_handle(&self) -> Option<Arc<Playback>> {
        self.playback.lock().await.as_ref().map(Arc::clone)
    }

    /// The playback handle, opening the device on first use.
    async fn playback(&self) -> Result<Arc<Playback>, SpeakError> {
        let mut guard = self.playback.lock().await;
        if let Some(p) = guard.as_ref() {
            return Ok(Arc::clone(p));
        }
        let playback = match self.test_format {
            Some(format) => Playback::detached(format),
            None => open_device_thread().await?,
        };
        let handle = Arc::new(playback);
        *guard = Some(Arc::clone(&handle));
        Ok(handle)
    }

    /// Stop speaking: flush queued audio and abort any synthesis still running.
    ///
    /// Returns the id of the utterance that was still going, if any — whether
    /// it was synthesizing or only playing out. Those are one case here on
    /// purpose: `speak` returns once the last sample is queued, so by the time
    /// a user hits stop the utterance is usually finished producing but still
    /// coming out of the speakers, and that is exactly the case people press
    /// stop in. The slot is held for the whole of it, so cancelling then both
    /// flushes the ring and reports what it silenced.
    pub async fn cancel(&self) -> Option<String> {
        let taken = self.current.lock().take();
        if let Some((_, flag)) = taken.as_ref() {
            flag.store(true, Ordering::Relaxed);
        }
        if let Some(p) = self.playback.lock().await.as_ref() {
            p.cancel();
        }
        let id = taken.map(|(id, _)| id);
        if let Some(id) = &id {
            debug!("cancelled in-flight utterance {id}");
            if let Some(bus) = &self.events {
                bus.publish_speaking_state(false, Some(id.clone()));
            }
        }
        id
    }

    /// Register a voice with the loaded model ahead of anything asking to speak
    /// in it.
    ///
    /// What the first use of a cloned voice pays for is not the speaking: it is
    /// deriving the speaker embedding and, for a clip stored with a transcript,
    /// encoding it into the codes an in-context example is made of. On a fresh
    /// clip length that is seconds to tens of seconds of GPU work, and landing
    /// it on the first press of a Preview button makes a working feature look
    /// broken. Called when a clip is saved, it lands instead where the user has
    /// just finished recording and expects a pause — and the backend writes
    /// what it derived to disk, so the cost is paid once rather than once per
    /// load.
    ///
    /// Quiet by design: nothing is waiting on the result, a model that does not
    /// clone has nothing to prepare, and a failure here must not make the save
    /// that triggered it look failed. The next request to speak in the voice
    /// registers it the usual way and reports properly if it cannot.
    pub async fn prepare_cloned_voice(&self, model: &SharedLoadedModel, voice: &str) {
        let guard = model.read().await;
        let Some(loaded) = guard.as_ref() else {
            return;
        };
        if !loaded
            .definition
            .product
            .voice_kinds
            .contains(&super_voice_registry_types::manifest::VoiceKind::Cloned)
        {
            return;
        }
        if let Err(e) = self.ensure_cloned_voice(loaded, voice).await {
            log::info!("voice {voice} will be registered on first use instead: {e}");
        }
    }

    /// Make sure a `voice:<uuid>` id has been registered with the loaded
    /// backend, pushing its reference clip the first time it is used.
    ///
    /// A no-op for preset and described ids, and for a voice this instance has
    /// already been given: registration is per loaded instance, so switching
    /// models re-registers and unloading forgets.
    async fn ensure_cloned_voice(
        &self,
        loaded: &crate::tts::daemon::types::LoadedModel,
        voice: &str,
    ) -> Result<(), SpeakError> {
        ensure_registered(self.voices.as_ref(), self.events.as_ref(), loaded, voice).await
    }

    /// Begin a streaming utterance.
    ///
    /// Text is fed in with [`SpeakSession::push`] and completed with
    /// [`SpeakSession::end`]. This is the path an LLM drives: chunks are
    /// synthesized as sentences complete, so speech starts long before the
    /// model has finished generating.
    ///
    /// # Errors
    /// See [`SpeakError`].
    pub async fn begin(
        &self,
        model: &SharedLoadedModel,
        options: SpeakOptions<'_>,
    ) -> Result<SpeakSession, SpeakError> {
        self.begin_with(self.arrival(), model, options).await
    }

    /// Take a place in line for the utterance slot.
    ///
    /// Taken when a request arrives, before anything it waits on. Two requests
    /// that find the model asleep both wait for one load, and which of them the
    /// scheduler wakes first is chance; ordering them by this ticket instead
    /// is what makes "newest wins" mean the newest the user asked for.
    pub fn arrival(&self) -> u64 {
        self.arrivals.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Overrule every request that has arrived so far and not yet started.
    ///
    /// A stop is the newest request: something asked for before it and still
    /// waiting for the model must not start speaking once the wait ends.
    pub fn overrule_waiting(&self) {
        // Its own ticket, newer than every one handed out before it, so the
        // request holding the last of those is overruled too; the next one to
        // arrive is newer still and speaks.
        let stop = self.arrival();
        let withdrawn = {
            let slot = self.current.lock();
            self.newest_started.fetch_max(stop, Ordering::SeqCst);
            // Every request announced so far is one of those overruled, so the
            // stop ends them now rather than once their wait does. Said here
            // only with nothing speaking: `cancel` says it for that.
            let mut announced = self.announced.lock();
            let last = announced.values().last().cloned();
            announced.clear();
            last.filter(|_| slot.is_none())
        };
        if let (Some(id), Some(bus)) = (withdrawn, &self.events) {
            bus.publish_speaking_state(false, Some(id));
        }
    }

    /// Say that the request holding `ticket` has started: `speaking_state`
    /// goes out now, under the id it will speak with, rather than once it
    /// claims the slot.
    ///
    /// For a request about to wait: on its model's load, which after an idle
    /// sleep is seconds, or on a cloned voice being given to the backend.
    /// Announced, it has the applet's working animation up for the wait
    /// instead of nothing at all. The utterance still starts once: the claim
    /// takes the announced id and does not say it again.
    ///
    /// The returned guard withdraws the announcement if the request ends
    /// without claiming the slot (refused, superseded, its client gone), which
    /// says the stop, unless something else is speaking or announced.
    pub fn announce(&self, ticket: u64) -> Announcement<'_> {
        let id = format!("utt_{}", uuid::Uuid::new_v4());
        self.announced.lock().insert(ticket, id.clone());
        if let Some(bus) = &self.events {
            bus.publish_speaking_state(true, Some(id));
        }
        Announcement {
            engine: self,
            ticket,
        }
    }

    /// End `ticket`'s announcement, if it is still one: the request will not
    /// speak. The stop goes out only when nothing else is speaking or
    /// announced, since `speaking_state` is the device's, not the request's,
    /// and an applet would drop the other one's animation on it.
    fn withdraw(&self, ticket: u64) {
        let stop = {
            let slot = self.current.lock();
            let mut announced = self.announced.lock();
            announced
                .remove(&ticket)
                .filter(|_| slot.is_none() && announced.is_empty())
        };
        if let (Some(id), Some(bus)) = (stop, &self.events) {
            bus.publish_speaking_state(false, Some(id));
        }
    }

    /// Claim the utterance slot for `ticket`: a new utterance supersedes the
    /// old, unless a newer one got there first.
    ///
    /// # Errors
    /// [`SpeakError::Superseded`] when a request that arrived after `ticket`
    /// has already started, or a stop came after it.
    async fn claim(&self, ticket: u64) -> Result<Claimed, SpeakError> {
        // A new utterance supersedes the old, unless a newer one got there
        // first. The check and the claim are one step under the slot lock: two
        // requests released by the same model load reach this point in any
        // order, and the one that arrived later must win either way.
        let flag = Arc::new(AtomicBool::new(false));
        let (id, announced, displaced) = {
            let mut slot = self.current.lock();
            let announced = self.announced.lock().remove(&ticket);
            if self.newest_started.load(Ordering::SeqCst) > ticket {
                debug!("dropped a request to speak that a newer one overtook");
                // What overtook it is speaking, so an announced one says no
                // stop here, unless nothing else is speaking or announced.
                let stop = announced.filter(|_| slot.is_none() && self.announced.lock().is_empty());
                drop(slot);
                if let (Some(id), Some(bus)) = (stop, &self.events) {
                    bus.publish_speaking_state(false, Some(id));
                }
                return Err(SpeakError::Superseded);
            }
            self.newest_started.store(ticket, Ordering::SeqCst);
            let was_announced = announced.is_some();
            let id = announced.unwrap_or_else(|| format!("utt_{}", uuid::Uuid::new_v4()));
            let displaced = slot.replace((id.clone(), Arc::clone(&flag)));
            (id, was_announced, displaced)
        };
        // Flushing after the claim rather than before it: the ring is still
        // empty of this utterance, and whatever claimed the slot before us is
        // flagged and flushed whichever order the two ran in.
        //
        // No stop event for the one displaced. The device does not fall silent
        // when one utterance replaces another, so `speaking_state` must not say
        // it did: a `false` right before the new utterance's `true` reads as a
        // gap, and the applet would drop a live visualizer back to idle for a
        // frame on every back-to-back utterance. See
        // `docs/protocol/endpoints/v1/events.md`.
        if let Some((old, old_flag)) = displaced {
            old_flag.store(true, Ordering::Relaxed);
            debug!("cancelled in-flight utterance {old}");
        }
        if let Some(p) = self.playback.lock().await.as_ref() {
            p.cancel();
        }
        Ok(Claimed {
            id,
            flag,
            announced,
        })
    }

    /// The playback for the utterance `id`, which has just claimed the slot.
    ///
    /// When the device cannot be opened, the slot is freed and the stop said:
    /// the utterance will never speak, and an announced one would otherwise
    /// leave a widget showing its wait for good.
    async fn playback_for(&self, id: &str) -> Result<Arc<Playback>, SpeakError> {
        let opened = self.playback().await;
        if opened.is_err() {
            finish_utterance(&self.current, self.events.as_ref(), id);
        }
        opened
    }

    /// Start reporting `speech_progress` for the utterance `id`.
    ///
    /// From the start, not from the end of synthesis: the first chunk is heard
    /// while the rest are still being synthesized, and a listener waiting for
    /// `spoken_ms > 0` must not wait for the last of them.
    fn spawn_progress(
        &self,
        playback: &Arc<Playback>,
        id: &str,
    ) -> Option<tokio::task::JoinHandle<()>> {
        self.events.as_ref().map(|bus| {
            tokio::spawn({
                let playback = Arc::clone(playback);
                let slot = Arc::clone(&self.current);
                let bus = Arc::clone(bus);
                let id = id.to_string();
                async move { report_progress(&playback, &slot, &bus, &id).await }
            })
        })
    }

    /// [`begin`](Self::begin) for a request that took its
    /// [`arrival`](Self::arrival) ticket earlier.
    ///
    /// # Errors
    /// See [`SpeakError`]. [`SpeakError::Superseded`] when a request that
    /// arrived after `ticket` has already started, or a stop came after it.
    pub async fn begin_with(
        &self,
        ticket: u64,
        model: &SharedLoadedModel,
        options: SpeakOptions<'_>,
    ) -> Result<SpeakSession, SpeakError> {
        let policy = {
            let guard = model.read().await;
            let loaded = guard.as_ref().ok_or(SpeakError::NotLoaded)?;
            // Before `cancel` below, for the same reason the not-loaded check
            // is: a request that cannot be served must not interrupt what is
            // already playing.
            if let Some(voice) = options.voice {
                check_voice(&loaded.definition, voice)?;
                // Under the same read guard as the check, and before the slot
                // is claimed: a cloned voice the backend has not been given
                // yet must be pushed before the first chunk, and a model
                // switch must not land in between.
                self.ensure_cloned_voice(loaded, voice).await?;
            }
            ChunkPolicy::for_model(loaded.definition.product.max_input_chars)
        };

        let Claimed {
            id,
            flag,
            announced,
        } = self.claim(ticket).await?;

        let playback = self.playback_for(&id).await?;
        let (tx, mut rx) = mpsc::unbounded_channel::<PcmMsg>();

        // Analysis happens on the producer side and delivery on the listener's,
        // so the two are separated by a queue. See [`publish_bands`].
        let (bands_tx, bands_rx) = mpsc::unbounded_channel::<BandFrame>();
        if let Some(bus) = &self.events {
            tokio::spawn(publish_bands(
                Arc::clone(&playback),
                bus.clone(),
                bands_rx,
                Arc::clone(&flag),
            ));
        }

        // The pump owns the awaiting: it is where ring backpressure is felt,
        // and it runs concurrently with the backend read so audio starts before
        // synthesis finishes.
        let pump = tokio::spawn({
            let playback = Arc::clone(&playback);
            let analyze = self.events.is_some();
            let cancelled = Arc::clone(&flag);
            async move {
                // The visualizer is analyzed here rather than on the audio
                // callback: the pump sees every sample on its way to the ring,
                // and an FFT on the callback thread would risk the device
                // deadline for a cosmetic feature. What the pump cannot do is
                // *publish*, because it runs up to a full ring ahead of the
                // speakers — so each frame is stamped with the position it
                // belongs to and handed to `publish_bands`.
                let mut windows = None;
                while let Some(msg) = rx.recv().await {
                    // The sink runs ahead of the speakers by as much as a
                    // faster-than-realtime backend can produce, so this channel
                    // can hold far more than the ring. A stop empties the ring
                    // but cannot reach in here: the pump has to notice it, or
                    // it refills the ring from the backlog and the stopped
                    // utterance carries on.
                    if cancelled.load(Ordering::Relaxed) {
                        break;
                    }
                    match msg {
                        PcmMsg::Samples(samples, params) => {
                            if analyze {
                                let windows = windows.get_or_insert_with(|| {
                                    BandWindows::new(params.sample_rate, playback.format())
                                });
                                // Before the push: this is where the chunk
                                // about to be queued starts.
                                let at = playback.queue_position();
                                for frame in windows.feed(&samples, params, at) {
                                    let _ = bands_tx.send(frame);
                                }
                            }
                            if let Err(e) = playback.push(&samples, params, &cancelled).await {
                                warn!("playback push failed: {e}");
                                break;
                            }
                        }
                        PcmMsg::Boundary => playback.end_chunk(&cancelled).await,
                    }
                }
                // A stopped utterance's tail will never be heard, and
                // `publish_bands` has already given up on it.
                if !cancelled.load(Ordering::Relaxed)
                    && let Some(frame) = windows.as_mut().and_then(BandWindows::flush)
                {
                    let _ = bands_tx.send(frame);
                }
            }
        });

        let (events_tx, events_rx) = mpsc::unbounded_channel::<SpeechEvent>();
        // Said once per utterance: an announced one said it on arrival.
        if let Some(bus) = &self.events
            && !announced
        {
            bus.publish_speaking_state(true, Some(id.clone()));
        }
        let progress = self.spawn_progress(&playback, &id);
        Ok(SpeakSession {
            slot: Arc::clone(&self.current),
            bus: self.events.clone(),
            voices: self.voices.clone(),
            id,
            model: Arc::clone(model),
            options: options.into_owned(),
            normalizer: crate::tts::text::normalize::Normalizer::new(),
            chunker: Chunker::new(policy),
            sink: Some(PlaybackSink {
                tx,
                params: None,
                leftover: Vec::new(),
                marks: 0,
                cancelled: Arc::clone(&flag),
                events: events_tx,
            }),
            events: events_rx,
            pump: Some(pump),
            progress,
            playback,
            flag,
            chars: 0,
            marks: 0,
            chunks: 0,
            finished: false,
        })
    }

    /// Synthesize `text` with the loaded model and play it.
    ///
    /// Returns once the backend has finished producing and every sample has
    /// been queued; the device plays the tail out afterwards. A second call
    /// cancels the first.
    ///
    /// A thin wrapper over [`begin`](Self::begin): routing the one-shot path
    /// through the same session means the two cannot drift on normalization,
    /// chunking, or seam handling.
    ///
    /// # Errors
    /// See [`SpeakError`].
    pub async fn speak(
        &self,
        model: &SharedLoadedModel,
        text: &str,
        voice: Option<&str>,
        language: Option<&str>,
    ) -> Result<Utterance, SpeakError> {
        self.speak_with(self.arrival(), model, text, voice, language)
            .await
    }

    /// [`speak`](Self::speak) for a request that took its
    /// [`arrival`](Self::arrival) ticket earlier.
    ///
    /// # Errors
    /// See [`SpeakError`].
    pub async fn speak_with(
        &self,
        ticket: u64,
        model: &SharedLoadedModel,
        text: &str,
        voice: Option<&str>,
        language: Option<&str>,
    ) -> Result<Utterance, SpeakError> {
        self.speak_noting(ticket, model, text, voice, language, |_| {})
            .await
    }

    /// [`speak_with`](Self::speak_with), telling `begun` the utterance's id
    /// the moment it claims the slot, before its text is produced. Speaking
    /// returns only once every chunk is made, which for a long text is well
    /// into its playing: a caller that must find its speech while it plays,
    /// as an assistant's `stop` must, learns it here instead.
    ///
    /// # Errors
    /// See [`SpeakError`].
    pub async fn speak_noting(
        &self,
        ticket: u64,
        model: &SharedLoadedModel,
        text: &str,
        voice: Option<&str>,
        language: Option<&str>,
        begun: impl FnOnce(&str),
    ) -> Result<Utterance, SpeakError> {
        check_text(text)?;
        // Checked before claiming the utterance slot, so a request that cannot
        // be served never interrupts what is already playing.
        if model.read().await.is_none() {
            return Err(SpeakError::NotLoaded);
        }

        let options = SpeakOptions { voice, language };
        let mut session = self.begin_with(ticket, model, options).await?;
        begun(session.id());
        session.push(text).await?;
        session.end().await
    }

    /// Synthesize `text` with the loaded model into one mono 16-bit WAV,
    /// played nowhere.
    ///
    /// The speak path without the speakers: the same normalizing, chunking
    /// and cloned-voice handling, so a clip sounds as the same text spoken
    /// would. It is not an utterance: it neither interrupts what is playing
    /// nor is interrupted by it, and it publishes no speaking events. While it
    /// runs the model counts as in use, so it is not put to sleep partway.
    ///
    /// Dropping the future stops it between backend reads: a client that
    /// hangs up takes the synthesis with it.
    ///
    /// # Errors
    /// See [`SpeakError`].
    pub async fn synthesize_wav(
        &self,
        model: &SharedLoadedModel,
        text: &str,
        voice: Option<&str>,
        language: Option<&str>,
    ) -> Result<Vec<u8>, SpeakError> {
        check_text(text)?;
        let _running = Synthesizing::start(&self.synthesizing);
        let (policy, name) = {
            let guard = model.read().await;
            let loaded = guard.as_ref().ok_or(SpeakError::NotLoaded)?;
            if let Some(voice) = voice {
                check_voice(&loaded.definition, voice)?;
                self.ensure_cloned_voice(loaded, voice).await?;
            }
            (
                ChunkPolicy::for_model(loaded.definition.product.max_input_chars),
                loaded.definition.name.clone(),
            )
        };

        let mut normalizer = crate::tts::text::normalize::Normalizer::new();
        let mut chunker = Chunker::new(policy);
        let mut chunks = chunker.push(&normalizer.push(text));
        chunks.extend(chunker.push(&normalizer.finish()));
        chunks.extend(chunker.finish());

        let mut out = crate::tts::daemon::synthesis::Assembler::default();
        for chunk in chunks.iter().filter(|c| !c.trim().is_empty()) {
            let request = SynthesizeRequest {
                text: chunk,
                voice,
                language,
                speed: None,
                instructions: None,
            };
            let mut sink = crate::tts::daemon::synthesis::CollectSink::default();
            synthesize_chunk(
                model,
                Some(&name),
                self.voices.as_ref(),
                self.events.as_ref(),
                &request,
                &mut sink,
            )
            .await?
            .map_err(|e| SpeakError::Synthesis(e.to_string()))?;
            if let Some((samples, params)) = sink.into_audio() {
                out.push(&samples, params)
                    .map_err(|e| SpeakError::Synthesis(e.to_string()))?;
            }
        }
        // Idle time counts from the end of the work, as it does for speech.
        model.touch();
        out.finish()
            .map_err(|e| SpeakError::Synthesis(e.to_string()))?
            .ok_or_else(|| SpeakError::Synthesis("the model produced no audio".to_string()))
    }
}

/// One streaming utterance.
///
/// Deltas go in with [`push`](Self::push); [`end`](Self::end) completes it.
/// Sentences are synthesized as they complete, so speech starts while the
/// producer is still writing — which is the entire point of the streaming path.
///
/// Dropping a session without calling `end` cancels it: the pump is stopped and
/// the ring flushed, so an abandoned connection cannot leave audio playing.
pub struct SpeakSession {
    slot: CurrentSlot,
    bus: Option<Arc<crate::tts::daemon::events::EventBus>>,
    /// The reference clips, for giving the backend a cloned voice again if it
    /// loses one mid-utterance.
    voices: Option<Arc<crate::tts::voices::VoiceLibrary>>,
    id: String,
    model: SharedLoadedModel,
    options: OwnedSpeakOptions,
    normalizer: crate::tts::text::normalize::Normalizer,
    chunker: Chunker,
    /// Taken in `end`/`cancel` to close the channel and stop the pump.
    sink: Option<PlaybackSink>,
    events: mpsc::UnboundedReceiver<SpeechEvent>,
    pump: Option<tokio::task::JoinHandle<()>>,
    /// The `speech_progress` ticker, running from the start of the utterance.
    /// `end` hands it to the playout watcher; a cancel or a drop stops it.
    progress: Option<tokio::task::JoinHandle<()>>,
    playback: Arc<Playback>,
    flag: Arc<AtomicBool>,
    chars: usize,
    marks: usize,
    chunks: usize,
    finished: bool,
}

impl std::fmt::Debug for SpeakSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeakSession")
            .field("id", &self.id)
            .field("chunks", &self.chunks)
            .finish_non_exhaustive()
    }
}

impl SpeakSession {
    /// The utterance id, for correlating events and cancellation.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// How many synthesis chunks have been queued so far.
    ///
    /// Grows as sentences complete, which is what makes the streaming path
    /// observable: it is non-zero before `end` is ever called.
    #[must_use]
    pub fn chunks(&self) -> usize {
        self.chunks
    }

    /// Whether the session has been cancelled out from under it — by a later
    /// utterance, or by an explicit stop.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    /// Take the next pending event, if any. Non-blocking.
    pub fn try_next_event(&mut self) -> Option<SpeechEvent> {
        self.events.try_recv().ok()
    }

    /// Milliseconds spoken and still queued, for a progress report.
    ///
    /// Both are derived from the device format rather than wall-clock time, so
    /// they describe a position in the utterance and stay correct while the
    /// stream is idle.
    #[must_use]
    pub fn progress(&self) -> (u64, u64) {
        let format = self.playback.format();
        let per_ms = u64::from(format.sample_rate) * u64::from(format.channels.max(1)) / 1000;
        if per_ms == 0 {
            return (0, 0);
        }
        let stats = self.playback.stats();
        let queued = self.playback.buffered() as u64;
        (stats.rendered_samples / per_ms, queued / per_ms)
    }

    /// Feed a delta.
    ///
    /// Synthesizes and queues every sentence the delta completed. Text that
    /// does not yet form a whole sentence stays buffered.
    ///
    /// # Errors
    /// See [`SpeakError`].
    pub async fn push(&mut self, delta: &str) -> Result<(), SpeakError> {
        if self.finished || self.is_cancelled() {
            return Ok(());
        }
        self.chars = self.chars.saturating_add(delta.chars().count());
        if self.chars > MAX_TEXT_CHARS {
            return Err(SpeakError::TextTooLong);
        }
        let normalized = self.normalizer.push(delta);
        let chunks = self.chunker.push(&normalized);
        for chunk in chunks {
            self.synthesize(&chunk).await?;
        }
        self.publish_progress();
        Ok(())
    }

    /// Whether the engine's slot still holds *this* utterance.
    ///
    /// Strictly equal, never "the slot is empty": an empty slot means someone
    /// already released it — a later utterance that has since finished — and
    /// treating that as "still mine" lets a superseded session publish state
    /// about audio it is not producing.
    fn is_current(&self) -> bool {
        self.slot
            .lock()
            .as_ref()
            .is_some_and(|(cur, _)| *cur == self.id)
    }

    /// Publish a `speech_progress` update for this utterance.
    fn publish_progress(&self) {
        if !self.is_current() {
            return;
        }
        if let Some(bus) = &self.bus {
            let (spoken_ms, queued_ms) = self.progress();
            bus.publish_speech_progress(self.id.clone(), spoken_ms, queued_ms);
        }
    }

    /// Announce that this utterance is over and release the engine's slot.
    ///
    /// See [`finish_utterance`]: it is a no-op unless the slot still holds this
    /// utterance, because a superseded session must not clear the state
    /// belonging to the utterance that replaced it.
    fn finish_now(&self) {
        finish_utterance(&self.slot, self.bus.as_ref(), &self.id);
    }

    /// Complete the utterance: flush the tail through both stages, synthesize
    /// it, and let the ring drain.
    ///
    /// # Errors
    /// See [`SpeakError`].
    pub async fn end(&mut self) -> Result<Utterance, SpeakError> {
        if !self.finished && !self.is_cancelled() {
            let tail = self.normalizer.finish();
            let mut chunks = self.chunker.push(&tail);
            chunks.extend(self.chunker.finish());
            for chunk in chunks {
                self.synthesize(&chunk).await?;
            }
        }
        self.finished = true;

        // Closing the channel ends the pump once it has drained, so every
        // sample is queued before the ring is told the utterance is over.
        self.sink = None;
        if let Some(pump) = self.pump.take() {
            let _ = pump.await;
        }

        if self.is_cancelled() {
            self.stop_progress();
            self.publish_progress();
            self.finish_now();
        } else {
            self.playback.finish(&self.flag).await;
            info!(
                "utterance {}: {} chars in {} chunk(s), {} marks",
                self.id, self.chars, self.chunks, self.marks
            );
            self.publish_progress();
            // Queued is not spoken. Everything above finishes when the last
            // sample reaches the ring, which for any backend faster than
            // realtime is seconds before the listener hears it — so the slot
            // and `speaking_state` are handed to a task that waits for the
            // audio instead. `end` still returns now: `POST /speak` answers
            // `202 Accepted` precisely because the device is still playing.
            self.spawn_playout_watcher();
        }
        Ok(Utterance {
            id: self.id.clone(),
            marks: self.marks,
            chunks: self.chunks,
        })
    }

    /// Follow the audio out of the ring, then close the utterance.
    ///
    /// Detached from the session on purpose: the caller that ended the
    /// utterance — an HTTP handler about to answer, a WebSocket about to send
    /// `done` — drops the session immediately afterwards, and the audio outlives
    /// it. Everything the watcher touches is owned or shared, so nothing here
    /// borrows the session. It takes the progress ticker along, since the
    /// position keeps moving until the last sample is heard.
    fn spawn_playout_watcher(&mut self) {
        let playback = Arc::clone(&self.playback);
        let slot = Arc::clone(&self.slot);
        let bus = self.bus.clone();
        let id = self.id.clone();
        let progress = self.progress.take();
        tokio::spawn(async move {
            if !playback.wait_for_playout(PLAYOUT_STALL_TIMEOUT).await {
                warn!(
                    "utterance {id}: output device stopped consuming with audio queued; \
                     reporting it finished"
                );
            }
            if let Some(progress) = progress {
                progress.abort();
            }
            finish_utterance(&slot, bus.as_ref(), &id);
        });
    }

    /// Stop the progress ticker. Before the stop is announced: a tick after
    /// `speaking_state: false` would report a position in an utterance that is
    /// over.
    fn stop_progress(&mut self) {
        if let Some(progress) = self.progress.take() {
            progress.abort();
        }
    }

    /// Abandon the utterance: stop the pump and drop queued audio.
    pub async fn cancel(&mut self) {
        self.flag.store(true, Ordering::Relaxed);
        self.finished = true;
        self.sink = None;
        // Aborted, not drained. Waiting for the pump to finish on its own is
        // waiting for it to push its backlog through a ring that only empties
        // as fast as the speakers play — a cancel that takes as long as the
        // speech it was meant to cut. The await is only for the abort to land.
        if let Some(pump) = self.pump.take() {
            pump.abort();
            let _ = pump.await;
        }
        self.stop_progress();
        self.playback.cancel();
        self.finish_now();
    }

    /// Synthesize one chunk and queue its audio.
    async fn synthesize(&mut self, chunk: &str) -> Result<(), SpeakError> {
        if chunk.trim().is_empty() || self.is_cancelled() {
            return Ok(());
        }
        let Some(sink) = self.sink.as_mut() else {
            return Ok(());
        };
        // Between chunks only, never before the first: the boundary fades a
        // seam, and there is nothing to fade against at the start.
        if self.chunks > 0 {
            sink.boundary();
        }

        let opts = self.options.as_ref();
        let request = SynthesizeRequest {
            text: chunk,
            voice: opts.voice,
            language: opts.language,
            // The backend contract still carries these — see
            // `docs/protocol/backend/contract.md` — but the daemon has no
            // source for them: rate and delivery are not things a speaking
            // client chooses, and no setting stores them yet. Sent as absent
            // rather than removed from the wire, so a backend that reads them
            // keeps parsing and can be fed again the day a setting exists.
            speed: None,
            instructions: None,
        };
        let result = synthesize_chunk(
            &self.model,
            None,
            self.voices.as_ref(),
            self.bus.as_ref(),
            &request,
            sink,
        )
        .await?;
        self.marks = sink.marks;

        match result {
            Ok(()) => {
                self.chunks += 1;
                Ok(())
            }
            // A cancel races the backend by design: the sink aborts the frame
            // pump, which surfaces here as an error the caller must not report
            // as a failure.
            Err(_) if self.is_cancelled() => Ok(()),
            Err(e) => {
                // Flagged before the flush, like every other stop: otherwise
                // the pump refills the ring from its backlog.
                self.flag.store(true, Ordering::Relaxed);
                self.playback.cancel();
                Err(SpeakError::Synthesis(e.to_string()))
            }
        }
    }
}

/// Synthesize one chunk with the loaded model into `sink`.
///
/// A backend can lose a cloned voice the daemon gave it: a subprocess backend
/// that was started again, a WASM backend whose private `/tmp` went away. It
/// says so before any audio, so the chunk is tried again, once, after giving
/// it the voice again. Only for a voice this instance was given: one it never
/// had is the ordinary `unknown_voice`.
///
/// With `expect`, the loaded model must still be that one: a synthesis that
/// joins its chunks into one file must not mix two models' voices.
///
/// The outer error is the daemon's (no model, a changed model, a voice it
/// could not give again); the inner one is the backend's.
async fn synthesize_chunk(
    model: &SharedLoadedModel,
    expect: Option<&str>,
    voices: Option<&Arc<crate::tts::voices::VoiceLibrary>>,
    bus: Option<&Arc<crate::tts::daemon::events::EventBus>>,
    request: &SynthesizeRequest<'_>,
    sink: &mut (dyn SynthesisSink + Send),
) -> Result<Result<()>, SpeakError> {
    let guard = model.read().await;
    let Some(loaded) = guard.as_ref() else {
        return Err(SpeakError::NotLoaded);
    };
    if let Some(name) = expect
        && loaded.definition.name != name
    {
        return Err(SpeakError::Synthesis(format!(
            "the model changed from {name} to {} partway through",
            loaded.definition.name
        )));
    }
    let mut result = loaded.instance.synthesize(request, sink).await;
    if let (Err(e), Some(voice)) = (&result, request.voice)
        && crate::tts::tts_models::v1::is_unknown_voice(e)
        && loaded.cloned_voices.lock().remove(voice)
    {
        info!(
            "{} no longer has voice {voice}; registering it again",
            loaded.definition.name
        );
        result = match ensure_registered(voices, bus, loaded, voice).await {
            Ok(()) => loaded.instance.synthesize(request, sink).await,
            Err(e) => Err(anyhow::Error::new(e)),
        };
    }
    Ok(result)
}

/// Counts a [`SpeechEngine::synthesize_wav`] as running, for as long as it
/// lives.
struct Synthesizing(Arc<AtomicUsize>);

impl Synthesizing {
    fn start(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(count))
    }
}

impl Drop for Synthesizing {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Drop for SpeakSession {
    fn drop(&mut self) {
        // Whether or not it finished: an `end` or a `cancel` dropped at one of
        // its awaits has set `finished` with the ticker still running.
        self.stop_progress();
        if self.finished {
            return;
        }
        // An abandoned session — a dropped WebSocket, a panicking handler —
        // must not leave audio playing. The pump ends on its own once the sink
        // is gone; the ring is flushed synchronously here because `Drop` has no
        // await.
        self.flag.store(true, Ordering::Relaxed);
        self.sink = None;
        if let Some(pump) = self.pump.take() {
            pump.abort();
        }
        self.playback.cancel();
        self.finish_now();
    }
}

/// Release the engine's utterance slot and announce that `id` stopped speaking.
///
/// Both together, under one lock acquisition, or neither: whoever clears the
/// slot is the one that gets to publish the stop, so a session superseded
/// mid-playout and the utterance that replaced it cannot both report an end —
/// and an applet never sees "not speaking" while speech is playing.
///
/// A no-op when the slot holds someone else, or nobody. Strictly equal, never
/// "the slot is empty": an empty slot means another finisher already ran.
fn finish_utterance(
    slot: &CurrentSlot,
    bus: Option<&Arc<crate::tts::daemon::events::EventBus>>,
    id: &str,
) {
    {
        let mut guard = slot.lock();
        if guard.as_ref().is_none_or(|(cur, _)| cur != id) {
            return;
        }
        *guard = None;
    }
    if let Some(bus) = bus {
        bus.publish_speaking_state(false, Some(id.to_string()));
    }
}

/// One analyzed window of audio, waiting for its turn to be heard.
struct BandFrame {
    /// Interleaved device samples queued ahead of this window, so the window
    /// is sounding once `rendered_samples` passes it.
    due: u64,
    bands: Vec<f32>,
    sample_rate: f32,
    energy: f32,
}

/// Cuts the pump's audio into analysis windows, each stamped with the device
/// position it will be heard at.
///
/// One frame per window, not one per chunk the pump is handed. A chunk is
/// whatever the backend put in one frame — 0.68 s of audio from Qwen — and the
/// analyzer keeps only the last window of what it is given, so a frame per
/// chunk moved the visualizer one and a half times a second, each time drawing
/// whichever 43 ms ended the chunk. Often that was the pause between two words.
///
/// Windows run on across chunks rather than restarting in each, so they are
/// evenly spaced however the backend frames its audio, and every one is full:
/// the FFT takes power-of-two lengths only, and a chunk's odd-sized remainder
/// analyzed on its own came back flat.
struct BandWindows {
    analyzer: super_voice_shared::tts::audio::AudioAnalyzer,
    /// The backend's rate, which the analyzer was built for. Analysis runs
    /// before `push` resamples.
    rate: u32,
    device: DeviceFormat,
    /// Mono samples short of a full window, carried into the next chunk.
    pending: Vec<f32>,
    /// Device position of `pending[0]`.
    pending_due: u64,
}

impl BandWindows {
    fn new(rate: u32, device: DeviceFormat) -> Self {
        Self {
            analyzer: super_voice_shared::tts::audio::AudioAnalyzer::new(rate, ANALYSIS_WINDOW),
            rate,
            device,
            pending: Vec::with_capacity(ANALYSIS_WINDOW),
            pending_due: 0,
        }
    }

    /// Analyze every window `samples` completes.
    ///
    /// `at` is where the chunk's first sample lands in the ring —
    /// [`Playback::queue_position`], read *before* the push. Read after, it
    /// would come too late: `push` waits on ring backpressure, so the chunk
    /// can already be playing by the time it returns, and its first windows
    /// would reach the visualizer behind the voice.
    fn feed(&mut self, samples: &[f32], params: AudioParams, at: u64) -> Vec<BandFrame> {
        if params.sample_rate != self.rate {
            // The analyzer's bins are laid out for one rate, and a remainder
            // at the old rate is no part of a window at the new one.
            *self = Self::new(params.sample_rate, self.device);
        }
        let carried = self.pending.len();
        if carried == 0 {
            self.pending_due = at;
        }
        if params.channels > 1 {
            self.pending
                .extend(mixer::map_channels(samples, params.channels, 1));
        } else {
            self.pending.extend_from_slice(samples);
        }

        let mut frames = Vec::new();
        let mut start = 0;
        while self.pending.len() - start >= ANALYSIS_WINDOW {
            let due = self.due(start, carried, at);
            frames.push(self.frame(&self.pending[start..start + ANALYSIS_WINDOW], due));
            start += ANALYSIS_WINDOW;
        }
        if start > 0 {
            self.pending_due = self.due(start, carried, at);
            self.pending.drain(..start);
        }
        frames
    }

    /// The carried remainder, padded out with silence to a full window.
    ///
    /// For the end of the utterance. Without it the last fraction of a window
    /// is never drawn, and an utterance shorter than a window draws nothing.
    fn flush(&mut self) -> Option<BandFrame> {
        if self.pending.is_empty() {
            return None;
        }
        let mut window = std::mem::take(&mut self.pending);
        window.resize(ANALYSIS_WINDOW, 0.0);
        Some(self.frame(&window, self.pending_due))
    }

    /// Device position of `pending[i]`, for a chunk queued at `at` behind
    /// `carried` samples of the one before.
    ///
    /// Only a window starting at 0 can begin in the carried samples, because
    /// fewer than a window's worth are ever carried.
    fn due(&self, i: usize, carried: usize, at: u64) -> u64 {
        match i.checked_sub(carried) {
            Some(into_chunk) => at + self.device_samples(into_chunk),
            None => self.pending_due,
        }
    }

    /// Interleaved device samples that `frames` mono frames at the backend's
    /// rate become, once `push` has resampled and channel-mapped them.
    fn device_samples(&self, frames: usize) -> u64 {
        frames as u64 * u64::from(self.device.sample_rate) / u64::from(self.rate.max(1))
            * u64::from(self.device.channels.max(1))
    }

    fn frame(&self, window: &[f32], due: u64) -> BandFrame {
        let data = self.analyzer.analyze(window);
        BandFrame {
            due,
            bands: data.bands,
            sample_rate: crate::tts::num_cast::u32_to_f32(self.rate),
            energy: data.total_energy,
        }
    }
}

/// Publish `frequency_bands` in step with the speakers.
///
/// The analysis has to run on the producer side — an FFT on the audio callback
/// would risk the device deadline — but the producer runs a full ring ahead of
/// what is audible, and a backend faster than realtime keeps it there.
/// Publishing where
/// the analysis happens therefore drove a visualizer that went flat two seconds
/// before the voice stopped, which reads as speech having ended. So each frame
/// carries the ring position it belongs to, and this task holds it until the
/// device has played that far.
///
/// Ends when the pump's channel closes and the backlog is drained or the audio
/// has played out, or at once if the utterance is cancelled — a cancel flushes
/// the ring and resets the position counters, so a held frame would otherwise
/// wait for a point the device will never reach again.
async fn publish_bands(
    playback: Arc<Playback>,
    bus: Arc<crate::tts::daemon::events::EventBus>,
    mut rx: mpsc::UnboundedReceiver<BandFrame>,
    cancelled: Arc<AtomicBool>,
) {
    let mut pending: std::collections::VecDeque<BandFrame> = std::collections::VecDeque::new();
    let mut closed = false;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }
        loop {
            match rx.try_recv() {
                Ok(frame) => pending.push_back(frame),
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        let played = playback.stats().rendered_samples;
        // Strictly past, not at: the first chunk is due at 0, and 0 samples
        // rendered means the prebuffer has not even started.
        while pending.front().is_some_and(|f| played > f.due) {
            let Some(frame) = pending.pop_front() else {
                break;
            };
            bus.publish_playback_frequency_bands(&frame.bands, frame.sample_rate, frame.energy);
        }
        // Once the pump is gone and the device has played everything it
        // queued, a frame still held was stamped past the end of the audio and
        // will never come due. Window positions are worked out from the rate
        // ratio, not read back from the resampler, so nothing but the ratio
        // says the audio reaches them; without this, one stamped a sample too
        // far would keep this task polling until the next cancel.
        if closed && (pending.is_empty() || played >= playback.queue_position()) {
            return;
        }
        tokio::time::sleep(BAND_TICK).await;
    }
}

/// Publish `speech_progress` for `id` every [`PROGRESS_INTERVAL`] until the
/// task is aborted or another utterance takes the slot.
///
/// Runs from the start of the utterance to the end of its playout: the
/// position moves whenever audio plays, and audio plays while later chunks are
/// still being synthesized as much as after the last one is queued.
async fn report_progress(
    playback: &Playback,
    slot: &CurrentSlot,
    bus: &Arc<crate::tts::daemon::events::EventBus>,
    id: &str,
) {
    let format = playback.format();
    let per_ms = u64::from(format.sample_rate) * u64::from(format.channels.max(1)) / 1000;
    if per_ms == 0 {
        return;
    }
    loop {
        tokio::time::sleep(PROGRESS_INTERVAL).await;
        if slot.lock().as_ref().is_none_or(|(cur, _)| cur != id) {
            return;
        }
        let spoken_ms = playback.stats().rendered_samples / per_ms;
        let queued_ms = playback.buffered() as u64 / per_ms;
        bus.publish_speech_progress(id.to_string(), spoken_ms, queued_ms);
    }
}

/// Open the default output device on a dedicated thread and return the producer
/// half.
///
/// The thread keeps the `!Send` cpal stream and parks; it ends with the
/// process. This is the whole reason the stream is split from [`Playback`].
async fn open_device_thread() -> Result<Playback, SpeakError> {
    let (tx, rx) = oneshot::channel::<Result<Playback>>();
    std::thread::Builder::new()
        .name("super-tts-playback".into())
        .spawn(move || {
            let opened = (|| {
                use cpal::traits::HostTrait;
                let host = cpal::default_host();
                let device = host
                    .default_output_device()
                    .ok_or_else(|| anyhow!("no default output device"))?;
                let config = crate::tts::audio::playback::output_config(&device)?;
                Playback::open(&device, &config)
            })();

            match opened {
                Ok((playback, stream)) => {
                    if tx.send(Ok(playback)).is_err() {
                        return; // caller gave up; dropping `stream` closes the device
                    }
                    // Hold the stream for the process's life. Parking keeps the
                    // thread off the scheduler entirely.
                    let _stream = stream;
                    loop {
                        std::thread::park();
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        })
        .map_err(|e| SpeakError::Device(e.to_string()))?;

    match rx.await {
        Ok(Ok(p)) => Ok(p),
        Ok(Err(e)) => {
            warn!("playback device unavailable: {e}");
            Err(SpeakError::Device(e.to_string()))
        }
        Err(_) => Err(SpeakError::Device("playback thread died".into())),
    }
}

/// Check a requested `voice` against what the model declares.
///
/// Two rules, both from `docs/protocol/backend/config.md`. The id's *shape*
/// must be a kind the model opted into: a bare id is `preset`, `voice:<uuid>`
/// is `cloned`, and `desc:<text>` is `described`. A preset id must then name
/// one of the model's `[[models.voices]]`.
///
/// The second rule is skipped when the model declares no presets, because the
/// manifest allows an empty list for a model whose voices are all cloned or
/// described — there is then nothing to check against, and refusing every id
/// would break a model that resolves them itself.
pub(crate) fn check_voice(
    definition: &crate::tts::tts_models::ModelDefinition,
    voice: &str,
) -> Result<(), SpeakError> {
    use super_voice_registry_types::manifest::VoiceKind;

    let kind = if voice.starts_with("voice:") {
        VoiceKind::Cloned
    } else if voice.starts_with("desc:") {
        VoiceKind::Described
    } else {
        VoiceKind::Preset
    };
    if !definition.product.voice_kinds.contains(&kind) {
        return Err(SpeakError::UnknownVoice(format!(
            "model '{}' does not accept a {kind} voice id",
            definition.name
        )));
    }
    if kind == VoiceKind::Preset
        && !definition.product.voices.is_empty()
        && !definition.product.voices.iter().any(|v| v.id == voice)
    {
        return Err(SpeakError::UnknownVoice(format!(
            "model '{}' declares no voice '{voice}'",
            definition.name
        )));
    }
    Ok(())
}

/// The coded answer a request to speak, or to synthesize, gets when `err`
/// stops it.
pub(crate) fn speech_refusal(
    err: SpeakError,
) -> super_voice_shared::tts::models::protocol::DaemonResponse {
    use super_voice_shared::tts::models::protocol::{DaemonResponse, ErrorCode};
    match err {
        e @ (SpeakError::EmptyText | SpeakError::TextTooLong | SpeakError::UnknownVoice(_)) => {
            DaemonResponse::error_with_code(ErrorCode::InvalidValue, &e.to_string())
        }
        SpeakError::NotLoaded => {
            DaemonResponse::error_with_code(ErrorCode::ModelNotLoaded, "model_not_loaded")
        }
        SpeakError::RecordingInProgress => DaemonResponse::error_with_code(
            ErrorCode::RecordingInProgress,
            "a recording is capturing, and the microphone would hear the speech; \
             try again when it stops",
        ),
        SpeakError::VoiceRequired(model) => DaemonResponse::error_with_code(
            ErrorCode::VoiceRequired,
            &format!(
                "{model} speaks only in a voice the user chooses, and none is chosen; \
                 set one with POST /tts/pipeline/1/model/{model}/voice"
            ),
        ),
        e => DaemonResponse::error(&e.to_string()),
    }
}

impl crate::tts::daemon::types::SuperTTSDaemon {
    /// Handle `Command::Speak`.
    ///
    /// Returns as soon as the backend has finished producing and every sample
    /// is queued; the device plays out the tail afterwards. Errors are mapped
    /// to the same coded shape every other endpoint uses, so a client switches
    /// on `error_code` rather than parsing prose.
    pub async fn handle_speak(
        &self,
        text: String,
    ) -> super_voice_shared::tts::models::protocol::DaemonResponse {
        self.handle_speak_noting(text, |_| {}).await
    }

    /// [`handle_speak`](Self::handle_speak), telling `begun` the utterance's
    /// id as soon as it starts: see [`Speech::speak_noting`].
    pub async fn handle_speak_noting(
        &self,
        text: String,
        begun: impl FnOnce(&str) + Send,
    ) -> super_voice_shared::tts::models::protocol::DaemonResponse {
        use crate::tts::output::notice::Failure;
        use super_voice_shared::tts::models::protocol::{DaemonResponse, ErrorCode};

        // In line from the moment it arrives, before the load below.
        let ticket = self.speech.arrival();

        // The request is `text`; how it is spoken comes from the user's
        // settings, the same way the streaming path gets it — see
        // [`stored_defaults`](Self::stored_defaults), which both go through.
        //
        // Text that cannot be spoken is refused first: with idle unload on, it
        // would otherwise load the model only to be refused after.
        if let Err(e) = check_text(&text) {
            return DaemonResponse::error_with_code(ErrorCode::InvalidValue, &e.to_string());
        }
        // Nothing is spoken while a recording captures: the microphone would
        // hear it. Before the load, which it would only waste.
        if self.recording.is_capturing() {
            return self.refuse_speech(SpeakError::RecordingInProgress).await;
        }
        // So is a model that cannot speak until the user picks its voice. The
        // backend refuses it too, but only once the model is loaded, which is
        // seconds of GPU work for a `500`.
        if let Some(model) = self.voice_still_to_choose().await {
            return self.refuse_speech(SpeakError::VoiceRequired(model)).await;
        }
        // Started, as far as anyone watching is concerned: what follows can
        // be seconds of loading. Held to the end, so a refusal from here on
        // takes the announcement back.
        let _announced = self.speech.announce(ticket);
        // A model is selected and failed to load: the notice says that, not
        // "no model loaded", which would send the user to load one. The answer
        // keeps the code a missing model gets.
        if let Err(detail) = self.stage().ensure_loaded().await {
            let method = crate::tts::output::notification::method_for_speech(
                self.config.read().await.notification_method,
            );
            let mut notifier = self.notifier.lock().await;
            crate::tts::output::notification::deliver(
                method,
                &mut notifier,
                &Failure::could_not_load_model(&detail),
            )
            .await;
            return DaemonResponse::error_with_code(ErrorCode::ModelNotLoaded, "model_not_loaded");
        }
        let (voice, language) = self.stored_defaults().await;

        // `speak` rather than the session API on purpose: it refuses empty,
        // oversized, and unloaded *before* claiming the utterance slot, so a
        // request that cannot be served never interrupts what is playing.
        let result = self
            .speech
            .speak_noting(
                ticket,
                &self.model,
                &text,
                voice.as_deref(),
                language.as_deref(),
                begun,
            )
            .await;

        match result {
            Ok(utterance) => DaemonResponse::success().with_utterance_id(utterance.id),
            // Accepted, then replaced before it started: the same answer an
            // utterance replaced while speaking gets, since its caller can do
            // nothing different about either.
            Err(SpeakError::Superseded) => {
                DaemonResponse::success().with_utterance_id(format!("utt_{}", uuid::Uuid::new_v4()))
            }
            Err(e) => self.refuse_speech(e).await,
        }
    }

    /// Handle `POST /tts/synthesize`: `text` spoken into a WAV file for the
    /// client, in the voice and language `speak` would use, and played
    /// nowhere.
    ///
    /// Checked as `speak` is, in the same order, but refused without a
    /// desktop notice: the caller is an app showing its own answer, not a
    /// shortcut with no window.
    ///
    /// # Errors
    /// Why it was refused; [`speech_refusal`] is the coded answer.
    pub async fn handle_synthesize(&self, text: &str) -> Result<Vec<u8>, SpeakError> {
        check_text(text)?;
        if let Some(model) = self.voice_still_to_choose().await {
            return Err(SpeakError::VoiceRequired(model));
        }
        if self.stage().ensure_loaded().await.is_err() {
            return Err(SpeakError::NotLoaded);
        }
        let (voice, language) = self.stored_defaults().await;
        self.speech
            .synthesize_wav(&self.model, text, voice.as_deref(), language.as_deref())
            .await
    }

    /// Answer a `speak` that could not be served, with the coded error its
    /// cause maps to.
    async fn refuse_speech(
        &self,
        err: SpeakError,
    ) -> super_voice_shared::tts::models::protocol::DaemonResponse {
        use crate::tts::output::notice::{Failure, Origin};

        // A `speak` is as likely to come from a keyboard shortcut as from an
        // app, and a shortcut has no UI to show the coded error to. So every
        // failure the caller did not cause also gets a desktop notice; the two
        // input-validation errors do not, because whoever sent bad text is by
        // definition looking at the response.
        if let Some(failure) = match &err {
            SpeakError::EmptyText
            | SpeakError::TextTooLong
            | SpeakError::UnknownVoice(_)
            | SpeakError::Superseded => None,
            SpeakError::NotLoaded => Some(Failure::no_model_loaded()),
            SpeakError::Device(detail) => Some(Failure::could_not_open_output(detail)),
            SpeakError::Synthesis(detail) => {
                Some(Failure::synthesis_failed(Origin::Backend, detail))
            }
            SpeakError::VoiceRequired(model) => Some(Failure::no_voice_chosen(model)),
            SpeakError::RecordingInProgress => Some(Failure::recording_in_progress()),
        } {
            let method = crate::tts::output::notification::method_for_speech(
                self.config.read().await.notification_method,
            );
            let mut notifier = self.notifier.lock().await;
            crate::tts::output::notification::deliver(method, &mut notifier, &failure).await;
        }

        speech_refusal(err)
    }

    /// Open a streaming session with the configured defaults already resolved.
    ///
    /// The single door for the HTTP layer. `POST /speak` and
    /// `GET /speak/stream` have to resolve a voice and a language the same way,
    /// and for a while they did not: the one-shot path went through
    /// [`handle_speak`](Self::handle_speak), which calls
    /// [`stored_defaults`](Self::stored_defaults), while the streaming path
    /// called [`SpeechEngine::begin`] directly with whatever the `start` frame
    /// carried. A `start` naming no voice therefore reached the backend with
    /// none, and a model whose voices are all cloned answered every utterance
    /// with *"this model speaks in a cloned voice, so a request has to name
    /// one"* — with the user's chosen voice sitting in the config the whole
    /// time. Both paths come through here now so that cannot drift again.
    ///
    /// # Errors
    /// See [`SpeakError`].
    pub(crate) async fn begin_speech(&self) -> Result<SpeakSession, SpeakError> {
        let ticket = self.speech.arrival();
        // Before the load, for the reasons `handle_speak` gives.
        if self.recording.is_capturing() {
            return Err(SpeakError::RecordingInProgress);
        }
        if let Some(model) = self.voice_still_to_choose().await {
            return Err(SpeakError::VoiceRequired(model));
        }
        // As `handle_speak` announces it, for the same wait.
        let _announced = self.speech.announce(ticket);
        // A load that fails is logged; the session reports the missing model.
        let _ = self.stage().ensure_loaded().await;
        let (voice, language) = self.stored_defaults().await;
        let options = SpeakOptions {
            voice: voice.as_deref(),
            language: language.as_deref(),
        };
        self.speech.begin_with(ticket, &self.model, options).await
    }

    /// How the loaded model is configured to speak: its voice and its language.
    ///
    /// The whole of it. A request carries `text` and nothing else, so this is
    /// not a fallback for what a client left out — it is the only source there
    /// is. Which voice the machine speaks in is the user's setting, changed
    /// through `/tts/pipeline/{stage}/model/{model}/voice` and
    /// `/settings/language`, and an app that can make the machine talk does not
    /// get a say in it by virtue of being able to talk.
    ///
    /// Resolved per utterance rather than cached, so a voice changed while
    /// something is queued takes effect on the next thing said.
    /// The selected model's name, when it speaks only in a voice the user
    /// chooses and none is chosen: its `voice_kinds` offers no preset and it
    /// declares no `default_voice`, so a backend has nothing to fall back on
    /// and refuses the utterance. `None` for any other model, and when nothing
    /// is selected — the load that follows reports that.
    ///
    /// Read from the selection rather than the loaded model, so it can be asked
    /// before anything is loaded. A stored voice the model no longer declares
    /// counts as none, as [`stored_defaults`](Self::stored_defaults) drops it.
    pub(crate) async fn voice_still_to_choose(&self) -> Option<String> {
        use super_voice_registry_types::manifest::VoiceKind;
        let (model, source) = self.stage_selection().await?;
        let def = self.find_model_definition(&source, &model).await?;
        if def.product.voice_kinds.contains(&VoiceKind::Preset)
            || def.product.default_voice.is_some()
        {
            return None;
        }
        let config = self.config.read().await;
        let chosen = config
            .model_voice(&source, &model)
            .is_some_and(|voice| check_voice(&def, voice).is_ok());
        (!chosen).then_some(model)
    }

    pub(crate) async fn stored_defaults(&self) -> (Option<String>, Option<String>) {
        // The definition is copied out and the model guard dropped before the
        // config is read, so the two locks are never held at once and no
        // ordering between them can block a writer.
        let definition = {
            let guard = self.model.read().await;
            guard.as_ref().map(|loaded| loaded.definition.clone())
        };
        let Some(def) = definition else {
            return (None, None);
        };

        let config = self.config.read().await;
        let language = crate::tts::daemon::language::resolve_language(
            def.is_multilingual,
            config.model_language(&def.source, &def.name),
            config.primary_language(),
            &def.supported_languages,
        )
        .wire;
        let voice = config
            .model_voice(&def.source, &def.name)
            .and_then(|stored| {
                // A stored voice the model no longer declares is dropped rather
                // than sent: refusing the utterance over a backend that dropped a
                // voice in an update would take speech away entirely, and the
                // backend's own default is a better answer than silence.
                if let Err(e) = check_voice(&def, stored) {
                    warn!("ignoring the stored voice for {}: {e}", def.name);
                    return None;
                }
                Some(stored.to_owned())
            });
        (voice, language)
    }

    /// Handle `Command::StopSpeaking`. Stopping when nothing is speaking is a
    /// success, not an error — a client that cancels on a keypress should not
    /// have to know whether it won the race.
    pub async fn handle_stop_speaking(
        &self,
    ) -> super_voice_shared::tts::models::protocol::DaemonResponse {
        use super_voice_shared::tts::models::protocol::DaemonResponse;
        match self.stop_speaking().await {
            Some(id) => DaemonResponse::success().with_utterance_id(id),
            None => DaemonResponse::success(),
        }
    }

    /// Stop the utterance speaking, and every request still waiting to start.
    /// The id of the utterance it silenced, if one was speaking.
    pub async fn stop_speaking(&self) -> Option<String> {
        // What is still waiting to start was asked for before the stop.
        self.speech.overrule_waiting();
        self.speech.cancel().await
    }
}

#[cfg(test)]
#[path = "speech_tests.rs"]
mod tests;
