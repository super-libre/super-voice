// SPDX-License-Identifier: GPL-3.0-only
//! Unit tests for the speak path's request validation.

use super::{SpeakError, check_voice};
use crate::tts::tts_models::ModelDefinition;
use std::time::Duration;
use super_voice_registry_types::manifest::{Device, VoiceEntry, VoiceKind, VoiceModel};

/// A model declaring `kinds` and the preset ids in `voices`.
fn model(kinds: Vec<VoiceKind>, voices: &[&str]) -> ModelDefinition {
    ModelDefinition {
        name: "kokoro-82m".into(),
        source: "github.com/super-libre/super-voice-kokoro".into(),
        is_multilingual: true,
        primary_language: "en".into(),
        supported_languages: vec!["en".into(), "ja".into()],
        estimated_vram_bytes: 0,
        processing_interval: Duration::from_millis(0),
        supported_devices: vec![Device::Cpu],
        realtime: false,
        provider: None,
        product: VoiceModel {
            voice_kinds: kinds,
            voices: voices
                .iter()
                .map(|v| VoiceEntry {
                    id: (*v).to_string(),
                    label: Some((*v).to_string()),
                    language: None,
                    tags: Vec::new(),
                })
                .collect(),
            ..VoiceModel::new(super_voice_registry_types::manifest::ModelRole::Synthesis)
        },
    }
}

#[test]
fn a_declared_preset_is_accepted() {
    let m = model(vec![VoiceKind::Preset], &["af_heart", "jf_alpha"]);
    assert!(check_voice(&m, "af_heart").is_ok());
    assert!(check_voice(&m, "jf_alpha").is_ok());
}

/// The gap this check exists to close: a manifest that ships a voice pack
/// without declaring it left the id reaching the backend unchecked.
#[test]
fn a_preset_the_model_does_not_declare_is_refused() {
    let m = model(vec![VoiceKind::Preset], &["af_heart"]);
    let err = check_voice(&m, "jf_alpha").expect_err("undeclared voice is refused");
    assert!(matches!(err, SpeakError::UnknownVoice(_)));
    assert!(
        err.to_string().contains("jf_alpha"),
        "the message names the voice asked for: {err}"
    );
}

/// An empty `voices` list is what the manifest uses for a model whose voices
/// are all cloned or described. There is nothing to check a preset against,
/// so it passes rather than being refused out of hand.
#[test]
fn a_model_declaring_no_presets_accepts_any_preset_id() {
    let m = model(vec![VoiceKind::Preset], &[]);
    assert!(check_voice(&m, "anything").is_ok());
}

#[test]
fn an_id_shape_the_model_did_not_opt_into_is_refused() {
    let m = model(vec![VoiceKind::Preset], &["af_heart"]);
    for id in ["voice:6f1b2c3d", "desc:a calm narrator"] {
        let err = check_voice(&m, id).expect_err("shape not opted into is refused");
        assert!(matches!(err, SpeakError::UnknownVoice(_)), "{id}");
    }
}

#[test]
fn opted_into_shapes_are_accepted_and_not_checked_against_presets() {
    let m = model(
        vec![VoiceKind::Preset, VoiceKind::Cloned, VoiceKind::Described],
        &["af_heart"],
    );
    // Neither id names a preset, and neither has to: only a bare id does.
    assert!(check_voice(&m, "voice:6f1b2c3d").is_ok());
    assert!(check_voice(&m, "desc:a calm narrator").is_ok());
    assert!(check_voice(&m, "af_heart").is_ok());
    assert!(check_voice(&m, "af_undeclared").is_err());
}

/// A cloned-only model refuses the bare ids it cannot resolve, which is the
/// same rule read from the other side.
#[test]
fn a_cloned_only_model_refuses_a_bare_id() {
    let m = model(vec![VoiceKind::Cloned], &[]);
    assert!(check_voice(&m, "af_heart").is_err());
    assert!(check_voice(&m, "voice:6f1b2c3d").is_ok());
}

// --- Cloned-voice registration ---------------------------------------------

use crate::tts::daemon::speech::SpeechEngine;
use crate::tts::daemon::types::LoadedModel;
use crate::tts::tts_models::synthesize::{ModelInfo, ModelInfoData, ModelState, Synthesize};
use crate::tts::tts_models::v1::{RegisterVoiceRequest, SynthesisSink, SynthesizeRequest};
use crate::tts::voices::VoiceLibrary;
use std::sync::{Arc, Mutex};

/// What one `register_voice` call carried.
#[derive(Debug, Clone, PartialEq)]
struct Registration {
    voice: String,
    transcript: Option<String>,
    pcm_bytes: usize,
}

/// Shared with the test, so what the backend was handed can be read back
/// through the `Box<dyn Synthesize>` the daemon holds it as.
#[derive(Debug, Default)]
struct Log {
    registered: Mutex<Vec<Registration>>,
    released: Mutex<Vec<String>>,
}

/// A backend that records registrations instead of synthesizing anything.
struct RecordingBackend {
    info: ModelInfoData,
    log: Arc<Log>,
}

impl ModelInfo for RecordingBackend {
    fn info(&self) -> &ModelInfoData {
        &self.info
    }
}

impl ModelState for RecordingBackend {
    fn device(&self) -> String {
        "cpu".to_string()
    }
}

#[async_trait::async_trait]
impl Synthesize for RecordingBackend {
    async fn synthesize(
        &self,
        _request: &SynthesizeRequest<'_>,
        _sink: &mut (dyn SynthesisSink + Send),
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn register_voice(&self, request: &RegisterVoiceRequest<'_>) -> anyhow::Result<()> {
        self.log.registered.lock().unwrap().push(Registration {
            voice: request.voice.to_string(),
            transcript: request.transcript.map(str::to_owned),
            pcm_bytes: request.pcm.len(),
        });
        Ok(())
    }

    async fn unregister_voice(&self, voice: &str) -> anyhow::Result<()> {
        self.log.released.lock().unwrap().push(voice.to_string());
        Ok(())
    }
}

/// A mono 16-bit WAV of `seconds` at the canonical rate.
fn clip(seconds: f32) -> Vec<u8> {
    let frames = crate::tts::num_cast::f32_to_usize(
        seconds * crate::tts::num_cast::u32_to_f32(crate::tts::voices::CLIP_SAMPLE_RATE),
    );
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: crate::tts::voices::CLIP_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut cursor, spec).unwrap();
        for i in 0..frames {
            w.write_sample(i16::try_from(i % 500).unwrap()).unwrap();
        }
        w.finalize().unwrap();
    }
    cursor.into_inner()
}

/// A model that clones, with `budget` seconds of reference audio and an
/// optional transcript requirement.
fn cloning_model(budget: f32, needs_transcript: bool) -> ModelDefinition {
    let mut def = model(vec![VoiceKind::Preset, VoiceKind::Cloned], &["af_heart"]);
    def.product.clone_ref_seconds = Some(budget);
    def.product.clone_needs_transcript = needs_transcript;
    def
}

/// A loaded model backed by the recording backend, plus the log to inspect.
fn loaded(definition: ModelDefinition) -> (LoadedModel, Arc<Log>) {
    let log = Arc::new(Log::default());
    let backend = RecordingBackend {
        info: ModelInfoData::new(
            definition.name.clone(),
            definition.source.clone(),
            true,
            false,
            Duration::from_millis(0),
        ),
        log: Arc::clone(&log),
    };
    (LoadedModel::new(definition, Box::new(backend)), log)
}

/// A detached engine reading voices from a library under `dir`.
fn engine(dir: &std::path::Path) -> (SpeechEngine, Arc<VoiceLibrary>) {
    let library = Arc::new(VoiceLibrary::new(dir.join("voices")));
    let engine = SpeechEngine::detached(crate::tts::audio::playback::DeviceFormat {
        sample_rate: 24_000,
        channels: 1,
    })
    .with_voices(Arc::clone(&library));
    (engine, library)
}

/// The clip is pushed once, trimmed to what the model declared, and the second
/// utterance costs nothing — which is the whole reason registration is a
/// separate route from synthesis.
#[tokio::test]
async fn registers_a_cloned_voice_once_and_trims_it_to_the_model_s_budget() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, library) = engine(dir.path());
    let voice = library
        .create(&clip(6.0), "Ada", Some("the reference sentence"))
        .expect("stores the clip");
    let (model, log) = loaded(cloning_model(2.0, false));

    engine
        .ensure_cloned_voice(&model, &voice.voice_id())
        .await
        .expect("registers");

    let registered = log.registered.lock().unwrap().clone();
    assert_eq!(registered.len(), 1, "one registration: {registered:?}");
    assert_eq!(registered[0].voice, voice.voice_id());
    assert_eq!(
        registered[0].transcript.as_deref(),
        Some("the reference sentence"),
        "the transcript rides along even when the model did not demand it"
    );
    assert_eq!(
        registered[0].pcm_bytes,
        2 * 2 * crate::tts::voices::CLIP_SAMPLE_RATE as usize,
        "two seconds of mono s16 at the canonical rate, not the six stored"
    );
    assert!(
        model.cloned_voices.lock().contains(&voice.voice_id()),
        "the instance remembers what it was given"
    );

    engine
        .ensure_cloned_voice(&model, &voice.voice_id())
        .await
        .expect("second use is a no-op");
    assert_eq!(
        log.registered.lock().unwrap().len(),
        1,
        "a cached voice is not pushed again"
    );
}

/// A preset id never touches the library — the daemon must not require one to
/// exist for models that do not clone.
#[tokio::test]
async fn a_preset_voice_never_reaches_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _library) = engine(dir.path());
    let (model, log) = loaded(cloning_model(10.0, false));

    engine
        .ensure_cloned_voice(&model, "af_heart")
        .await
        .expect("a preset needs no registration");
    assert!(log.registered.lock().unwrap().is_empty());
}

/// A `voice:<uuid>` the library never stored is bad input, not a synthesis
/// failure: it must not reach the backend or raise a desktop notice.
#[tokio::test]
async fn a_voice_the_library_does_not_have_is_refused_as_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _library) = engine(dir.path());
    let (model, log) = loaded(cloning_model(10.0, false));

    let err = engine
        .ensure_cloned_voice(&model, "voice:2f8a2d0e-0000-4000-8000-000000000000")
        .await
        .expect_err("an unknown voice is refused");
    assert!(
        matches!(err, SpeakError::UnknownVoice(_)),
        "expected UnknownVoice, got {err}"
    );
    assert!(log.registered.lock().unwrap().is_empty());
}

/// In-context cloning needs the words as well as the audio. Refusing when the
/// voice was stored without them says so once, at the point the pair is
/// assembled, instead of failing every request that follows.
#[tokio::test]
async fn a_model_that_clones_in_context_refuses_a_clip_with_no_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, library) = engine(dir.path());
    let silent = library.create(&clip(3.0), "No words", None).unwrap();
    let spoken = library
        .create(&clip(3.0), "With words", Some("hello"))
        .unwrap();
    let (model, log) = loaded(cloning_model(10.0, true));

    let err = engine
        .ensure_cloned_voice(&model, &silent.voice_id())
        .await
        .expect_err("a transcript-less clip is refused");
    assert!(matches!(err, SpeakError::UnknownVoice(_)), "got {err}");
    assert!(
        err.to_string().contains("No words"),
        "the message names the voice the user has to fix: {err}"
    );
    assert!(log.registered.lock().unwrap().is_empty());

    engine
        .ensure_cloned_voice(&model, &spoken.voice_id())
        .await
        .expect("the same model accepts a clip that has one");
    assert_eq!(log.registered.lock().unwrap().len(), 1);
}

/// A daemon built without a library cannot resolve a cloned id, and says so
/// rather than pushing an empty clip.
#[tokio::test]
async fn an_engine_with_no_library_refuses_a_cloned_id() {
    let engine = SpeechEngine::detached(crate::tts::audio::playback::DeviceFormat {
        sample_rate: 24_000,
        channels: 1,
    });
    let (model, log) = loaded(cloning_model(10.0, false));

    let err = engine
        .ensure_cloned_voice(&model, "voice:2f8a2d0e-0000-4000-8000-000000000000")
        .await
        .expect_err("no library, no cloned voice");
    assert!(matches!(err, SpeakError::UnknownVoice(_)), "got {err}");
    assert!(log.registered.lock().unwrap().is_empty());
}

// --- Visualizer windows ---

use super::{ANALYSIS_WINDOW, BandWindows};
use crate::tts::audio::playback::DeviceFormat;
use crate::tts::audio::playback::mixer;
use super_voice_shared::tts::audio::frames::{AudioParams, SampleFormat};

/// Qwen's rate, which is also Kokoro's.
const BACKEND_RATE: u32 = 24_000;

fn backend(channels: u16) -> AudioParams {
    AudioParams {
        sample_rate: BACKEND_RATE,
        channels,
        format: SampleFormat::S16Le,
    }
}

/// `frames` of a 440 Hz tone, interleaved across `channels`.
fn tone(frames: usize, channels: u16) -> Vec<f32> {
    (0..frames)
        .flat_map(|i| {
            let t = crate::tts::num_cast::usize_to_f32(i)
                / crate::tts::num_cast::u32_to_f32(BACKEND_RATE);
            let s = (std::f32::consts::TAU * 440.0 * t).sin() * 0.5;
            std::iter::repeat_n(s, usize::from(channels))
        })
        .collect()
}

/// One 32 KiB s16le frame from Qwen: what the pump used to turn into a single
/// visualizer frame, which is how the applet came to draw 1.5 times a second.
#[test]
fn one_long_chunk_is_analyzed_window_by_window() {
    let device = DeviceFormat {
        sample_rate: 48_000,
        channels: 2,
    };
    let mut windows = BandWindows::new(BACKEND_RATE, device);

    let frames = windows.feed(&tone(16_384, 1), backend(1), 1_000);

    assert_eq!(frames.len(), 16_384 / ANALYSIS_WINDOW);
    // Each window is 1024 frames at 24 kHz: 2048 at the device's rate, and
    // two interleaved samples apiece.
    let dues: Vec<u64> = frames.iter().map(|f| f.due).collect();
    let expected: Vec<u64> = (0..16).map(|k| 1_000 + k * 4_096).collect();
    assert_eq!(
        dues, expected,
        "stamped in device samples, from where the chunk lands"
    );
    assert!(
        frames.iter().all(|f| f.energy > 0.0),
        "every window is a full one, which the FFT can take"
    );
}

/// Backends frame their audio however they like, so windows have to run on
/// across frames or their spacing follows the backend instead of the clock.
#[test]
fn windows_run_on_across_chunks() {
    let device = DeviceFormat {
        sample_rate: BACKEND_RATE,
        channels: 1,
    };
    let mut windows = BandWindows::new(BACKEND_RATE, device);

    // The mock's 960-sample responses, queued back to back.
    let mut dues = Vec::new();
    for k in 0..4_u64 {
        dues.extend(
            windows
                .feed(&tone(960, 1), backend(1), k * 960)
                .iter()
                .map(|f| f.due),
        );
    }
    dues.extend(windows.flush().map(|f| f.due));

    assert_eq!(dues, vec![0, 1_024, 2_048, 3_072]);
}

/// A remainder is padded out rather than dropped or handed to the FFT short:
/// an utterance shorter than a window still moves the visualizer.
#[test]
fn the_remainder_is_flushed_as_a_full_window() {
    let device = DeviceFormat {
        sample_rate: 48_000,
        channels: 1,
    };
    let mut windows = BandWindows::new(BACKEND_RATE, device);

    assert!(windows.feed(&tone(960, 1), backend(1), 500).is_empty());
    let last = windows.flush().expect("the remainder is analyzed");
    assert_eq!(last.due, 500);
    assert!(last.energy > 0.0, "a padded window still carries the tone");
    assert!(windows.flush().is_none(), "and only once");
}

/// A stereo backend is a window per 1024 *frames*, so a window spans the same
/// stretch of time whatever the channel count.
#[test]
fn a_stereo_backend_is_windowed_by_frame() {
    let device = DeviceFormat {
        sample_rate: BACKEND_RATE,
        channels: 2,
    };
    let mut windows = BandWindows::new(BACKEND_RATE, device);

    let frames = windows.feed(&tone(2_048, 2), backend(2), 0);

    let dues: Vec<u64> = frames.iter().map(|f| f.due).collect();
    assert_eq!(dues, vec![0, 2_048]);
}

/// A window's stamp is worked out from the rate ratio, before `push` has
/// resampled anything. It must still land on audio that `push` will actually
/// queue, or the publisher would hold that frame for a position never played.
#[test]
fn every_window_lands_inside_the_audio_push_will_queue() {
    for device_rate in [48_000, 44_100, 22_050, 16_000] {
        let device = DeviceFormat {
            sample_rate: device_rate,
            channels: 2,
        };
        // Qwen's full frame and its short last one, Kokoro-sized odd ones, and
        // the mock's.
        for len in [16_384, 15_232, 7_001, 2_047, 960, 240] {
            let mut windows = BandWindows::new(BACKEND_RATE, device);
            let samples = tone(len, 1);
            let queued = mixer::prepare(&samples, backend(1), device).unwrap().len() as u64;
            let mut dues: Vec<u64> = windows
                .feed(&samples, backend(1), 0)
                .iter()
                .map(|f| f.due)
                .collect();
            dues.extend(windows.flush().map(|f| f.due));
            assert!(
                dues.iter().all(|&d| d < queued),
                "{len} samples to {device_rate} Hz queue {queued}, windows at {dues:?}"
            );
        }
    }
}

/// The pump creates the windows once per utterance, at the first chunk's rate.
/// Should a later chunk arrive at another, the bins have to follow it.
#[test]
fn a_change_of_rate_starts_the_windows_over() {
    let device = DeviceFormat {
        sample_rate: 48_000,
        channels: 1,
    };
    let mut windows = BandWindows::new(BACKEND_RATE, device);
    assert!(windows.feed(&tone(512, 1), backend(1), 0).is_empty());

    let other = AudioParams {
        sample_rate: 48_000,
        ..backend(1)
    };
    let frames = windows.feed(&tone(1_024, 1), other, 1_024);

    assert_eq!(frames.len(), 1, "the 24 kHz remainder is not mixed in");
    assert_eq!(frames[0].due, 1_024);
    assert!((frames[0].sample_rate - 48_000.0).abs() < f32::EPSILON);
}

/// A backend that knows only the cloned voices it was given since it last
/// lost them, and refuses any other the way the contract says, before any
/// audio. `amnesiac` makes it forget each voice as soon as it is given one.
struct ForgetfulBackend {
    info: ModelInfoData,
    log: Arc<Log>,
    known: Arc<Mutex<Vec<String>>>,
    attempts: Arc<Mutex<usize>>,
    amnesiac: bool,
}

impl ModelInfo for ForgetfulBackend {
    fn info(&self) -> &ModelInfoData {
        &self.info
    }
}

impl ModelState for ForgetfulBackend {
    fn device(&self) -> String {
        "cpu".to_string()
    }
}

#[async_trait::async_trait]
impl Synthesize for ForgetfulBackend {
    async fn synthesize(
        &self,
        request: &SynthesizeRequest<'_>,
        _sink: &mut (dyn SynthesisSink + Send),
    ) -> anyhow::Result<()> {
        *self.attempts.lock().unwrap() += 1;
        match request.voice {
            Some(v) if v.starts_with("voice:") && !self.known.lock().unwrap().iter().any(|k| k == v) => {
                Err(crate::tts::tts_models::v1::synthesize_error(
                    400,
                    br#"{"status":"error","message":"unknown_voice","detail":"This cloned voice is not registered."}"#,
                ))
            }
            _ => Ok(()),
        }
    }

    async fn register_voice(&self, request: &RegisterVoiceRequest<'_>) -> anyhow::Result<()> {
        self.log.registered.lock().unwrap().push(Registration {
            voice: request.voice.to_string(),
            transcript: request.transcript.map(str::to_owned),
            pcm_bytes: request.pcm.len(),
        });
        if !self.amnesiac {
            self.known.lock().unwrap().push(request.voice.to_string());
        }
        Ok(())
    }

    async fn unregister_voice(&self, voice: &str) -> anyhow::Result<()> {
        self.log.released.lock().unwrap().push(voice.to_string());
        Ok(())
    }
}

/// What a test reads back from a [`ForgetfulBackend`].
struct Forgetful {
    model: crate::tts::daemon::types::SharedLoadedModel,
    log: Arc<Log>,
    known: Arc<Mutex<Vec<String>>>,
    attempts: Arc<Mutex<usize>>,
}

async fn forgetful(amnesiac: bool) -> Forgetful {
    let definition = cloning_model(2.0, false);
    let log = Arc::new(Log::default());
    let known = Arc::new(Mutex::new(Vec::new()));
    let attempts = Arc::new(Mutex::new(0));
    let backend = ForgetfulBackend {
        info: ModelInfoData::new(
            definition.name.clone(),
            definition.source.clone(),
            true,
            false,
            Duration::from_millis(0),
        ),
        log: Arc::clone(&log),
        known: Arc::clone(&known),
        attempts: Arc::clone(&attempts),
        amnesiac,
    };
    let model: crate::tts::daemon::types::SharedLoadedModel = Arc::default();
    crate::tts::daemon::types::put_model(&model, LoadedModel::new(definition, Box::new(backend)))
        .await;
    Forgetful {
        model,
        log,
        known,
        attempts,
    }
}

/// A backend that loses a cloned voice it was given (a subprocess backend
/// started again, a WASM backend whose private `/tmp` went away) is given it
/// again, and the utterance is spoken rather than failed.
#[tokio::test]
async fn a_backend_that_lost_a_cloned_voice_is_given_it_again() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, library) = engine(dir.path());
    let voice = library.create(&clip(3.0), "Ada", None).unwrap().voice_id();
    let backend = forgetful(false).await;

    engine
        .speak(&backend.model, "Hello there.", Some(&voice), None)
        .await
        .expect("the first utterance registers and speaks");
    assert_eq!(backend.log.registered.lock().unwrap().len(), 1);

    backend.known.lock().unwrap().clear(); // the backend loses it
    engine
        .speak(&backend.model, "Hello again.", Some(&voice), None)
        .await
        .expect("the voice is given again and the utterance spoken");
    let registered = backend.log.registered.lock().unwrap().clone();
    assert_eq!(registered.len(), 2, "registered again: {registered:?}");
    assert!(registered.iter().all(|r| r.voice == voice));
    assert_eq!(
        *backend.attempts.lock().unwrap(),
        3,
        "one chunk, one refused chunk, and its retry"
    );
}

/// The retry is once. A backend that refuses the voice again right after
/// being given it fails the utterance instead of looping.
#[tokio::test]
async fn a_voice_refused_again_after_the_retry_fails_the_utterance() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, library) = engine(dir.path());
    let voice = library.create(&clip(3.0), "Ada", None).unwrap().voice_id();
    let backend = forgetful(true).await;

    let error = engine
        .speak(&backend.model, "Hello there.", Some(&voice), None)
        .await
        .expect_err("the backend never keeps the voice");
    assert!(
        matches!(error, SpeakError::Synthesis(ref m) if m.contains("not registered")),
        "{error:?}"
    );
    assert_eq!(
        backend.log.registered.lock().unwrap().len(),
        2,
        "registered before the utterance, and once more on the refusal"
    );
    assert_eq!(
        *backend.attempts.lock().unwrap(),
        2,
        "tried, and retried once"
    );
}

/// A shared model holding the recording backend, the way the daemon holds one.
async fn shared(definition: ModelDefinition) -> crate::tts::daemon::types::SharedLoadedModel {
    let (loaded, _log) = loaded(definition);
    let model: crate::tts::daemon::types::SharedLoadedModel = Arc::default();
    crate::tts::daemon::types::put_model(&model, loaded).await;
    model
}

/// Two requests that waited on the same model load are let through in either
/// order. The one that arrived later wins even when it is let through first:
/// the earlier one, reaching the slot after it, is dropped rather than
/// replacing it.
#[tokio::test]
async fn the_request_that_arrived_last_wins_whichever_is_let_through_first() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _library) = engine(dir.path());
    let model = shared(model(vec![VoiceKind::Preset], &["af_heart"])).await;

    let first = engine.arrival();
    let second = engine.arrival();

    let newer = engine
        .speak_with(second, &model, "The newer one.", None, None)
        .await
        .expect("the newer request speaks");
    let older = engine
        .speak_with(first, &model, "The older one.", None, None)
        .await;

    assert!(
        matches!(older, Err(SpeakError::Superseded)),
        "the older request must not speak after the newer: {older:?}"
    );
    assert!(newer.id.starts_with("utt_"));
}

/// Speaking says the utterance's id as it starts, before its text is made:
/// the same id it answers with once it is done, and the one in flight then.
#[tokio::test]
async fn speaking_says_its_utterance_as_it_starts() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _library) = engine(dir.path());
    let model = shared(model(vec![VoiceKind::Preset], &["af_heart"])).await;

    let mut begun = None;
    let mut in_flight = None;
    let spoken = engine
        .speak_noting(engine.arrival(), &model, "Hello there.", None, None, |id| {
            begun = Some(id.to_string());
            in_flight = engine.current();
        })
        .await
        .expect("it speaks");

    assert_eq!(begun.as_deref(), Some(spoken.id.as_str()));
    assert_eq!(in_flight, begun, "the utterance is in flight as it starts");
}

/// A stop overrules what is still waiting to start: a request asked for
/// before it does not start speaking once its wait ends.
#[tokio::test]
async fn a_stop_overrules_a_request_still_waiting_to_start() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _library) = engine(dir.path());
    let model = shared(model(vec![VoiceKind::Preset], &["af_heart"])).await;

    let waiting = engine.arrival();
    engine.overrule_waiting();
    let late = engine
        .speak_with(waiting, &model, "Too late.", None, None)
        .await;
    assert!(matches!(late, Err(SpeakError::Superseded)), "{late:?}");

    // Whatever arrives after the stop speaks as usual.
    engine
        .speak(&model, "After the stop.", None, None)
        .await
        .expect("a request after the stop speaks");
}

// --- Speech kept as a file ---------------------------------------------------

use super_voice_shared::tts::audio::frames::{Frame, FrameKind};

/// What a [`ToneBackend`] did, and what it waits on.
#[derive(Default)]
struct ToneLog {
    /// The text of every synthesis request, in order.
    texts: Mutex<Vec<String>>,
    /// Released once per request when set: lets a test hold a synthesis
    /// mid-way.
    gate: Option<Arc<tokio::sync::Semaphore>>,
    /// Told when a request has started, before it waits on the gate.
    started: Option<Arc<tokio::sync::Notify>>,
}

/// A backend that answers each request with a constant tone, ten samples per
/// character, mono at 24 kHz, or with no audio at all when `silent`.
struct ToneBackend {
    info: ModelInfoData,
    log: Arc<ToneLog>,
    silent: bool,
}

impl ModelInfo for ToneBackend {
    fn info(&self) -> &ModelInfoData {
        &self.info
    }
}

impl ModelState for ToneBackend {
    fn device(&self) -> String {
        "cpu".to_string()
    }
}

#[async_trait::async_trait]
impl Synthesize for ToneBackend {
    async fn synthesize(
        &self,
        request: &SynthesizeRequest<'_>,
        sink: &mut (dyn SynthesisSink + Send),
    ) -> anyhow::Result<()> {
        self.log
            .texts
            .lock()
            .unwrap()
            .push(request.text.to_string());
        if let Some(started) = &self.log.started {
            started.notify_one();
        }
        if let Some(gate) = &self.log.gate {
            gate.acquire().await.unwrap().forget();
        }
        sink.on_params(AudioParams {
            sample_rate: 24_000,
            channels: 1,
            format: SampleFormat::F32Le,
        })?;
        if self.silent {
            return Ok(());
        }
        let samples = 10 * request.text.chars().count();
        let payload: Vec<u8> = std::iter::repeat_n(0.5_f32, samples)
            .flat_map(f32::to_le_bytes)
            .collect();
        sink.on_frame(Frame {
            kind: FrameKind::Audio,
            payload,
        })
    }
}

/// A shared model answering with tones, named `name`.
async fn tone_model(
    name: &str,
    log: Arc<ToneLog>,
    silent: bool,
) -> crate::tts::daemon::types::SharedLoadedModel {
    let model: crate::tts::daemon::types::SharedLoadedModel = Arc::default();
    crate::tts::daemon::types::put_model(&model, tone_loaded(name, log, silent)).await;
    model
}

fn tone_loaded(name: &str, log: Arc<ToneLog>, silent: bool) -> LoadedModel {
    let mut definition = model(vec![VoiceKind::Preset], &["af_heart"]);
    definition.name = name.to_string();
    let backend = ToneBackend {
        info: ModelInfoData::new(
            definition.name.clone(),
            definition.source.clone(),
            true,
            false,
            Duration::from_millis(0),
        ),
        log,
        silent,
    };
    LoadedModel::new(definition, Box::new(backend))
}

fn wav_of(bytes: &[u8]) -> (hound::WavSpec, usize) {
    let reader = hound::WavReader::new(std::io::Cursor::new(bytes)).expect("a WAV");
    (reader.spec(), reader.len() as usize)
}

/// Every sentence is synthesized, and the audio comes back as one mono
/// 16-bit file at the model's rate, as long as all of it together.
#[tokio::test]
async fn a_synthesis_comes_back_as_one_file_of_every_sentence() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _library) = engine(dir.path());
    let log = Arc::new(ToneLog::default());
    let model = tone_model("tone", Arc::clone(&log), false).await;

    let wav = engine
        .synthesize_wav(
            &model,
            "The first sentence. And the second one.",
            None,
            None,
        )
        .await
        .expect("a file");

    let texts = log.texts.lock().unwrap().clone();
    assert!(!texts.is_empty());
    let chars: usize = texts.iter().map(|t| t.chars().count()).sum();
    let (spec, samples) = wav_of(&wav);
    assert_eq!(spec.channels, 1);
    assert_eq!(spec.sample_rate, 24_000);
    assert_eq!(spec.bits_per_sample, 16);
    assert_eq!(
        samples,
        10 * chars,
        "every chunk's audio, end to end: {texts:?}"
    );
}

/// A synthesis is not an utterance: it holds no utterance slot, and nothing
/// shows it as speaking. While it runs the model counts as in use.
#[tokio::test]
async fn a_synthesis_is_not_an_utterance_but_keeps_the_model_in_use() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _library) = engine(dir.path());
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let started = Arc::new(tokio::sync::Notify::new());
    let log = Arc::new(ToneLog {
        gate: Some(Arc::clone(&gate)),
        started: Some(Arc::clone(&started)),
        ..ToneLog::default()
    });
    let model = tone_model("tone", log, false).await;
    let engine = Arc::new(engine);

    let running = tokio::spawn({
        let engine = Arc::clone(&engine);
        let model = Arc::clone(&model);
        async move { engine.synthesize_wav(&model, "Held.", None, None).await }
    });
    started.notified().await;
    assert!(engine.is_synthesizing(), "in use while it runs");
    assert!(engine.current().is_none(), "no utterance slot is taken");

    gate.add_permits(16);
    running.await.unwrap().expect("a file");
    assert!(!engine.is_synthesizing(), "and no longer once done");
}

/// Progress is reported as soon as the first chunk is heard, while the later
/// ones are still being synthesized. It used to wait for synthesis to finish,
/// and the applet, which swaps its loading animation for the visualizer on the
/// first `spoken_ms > 0`, kept the animation up over most of a minute of a long
/// read-aloud.
#[tokio::test]
async fn progress_is_reported_while_later_chunks_are_still_synthesizing() {
    use crate::tts::audio::playback::State;
    use crate::tts::daemon::events::{EventBus, Topic};

    let dir = tempfile::tempdir().unwrap();
    let (engine, _library) = engine(dir.path());
    let bus = Arc::new(EventBus::new());
    let mut rx = bus.subscribe(Topic::SpeechProgress);
    let engine = Arc::new(engine.with_events(Arc::clone(&bus)));
    // One permit: the first chunk goes through, the second waits at the gate.
    let gate = Arc::new(tokio::sync::Semaphore::new(1));
    let log = Arc::new(ToneLog {
        gate: Some(Arc::clone(&gate)),
        ..ToneLog::default()
    });
    let model = tone_model("tone", Arc::clone(&log), false).await;

    // Two paragraphs, two chunks: a paragraph break ends a chunk whatever its
    // length. The first is long enough to fill the ring's prebuffer, which a
    // chunk cut at the usual opening target is not, and nothing is heard
    // before the prebuffer fills.
    let first = "This sentence pads the first paragraph out to a long first chunk. ".repeat(5);
    let first = first.trim_end();
    let second = "The second paragraph waits at the gate.";
    let text = format!("{first}\n\n{second}");
    let speaking = tokio::spawn({
        let engine = Arc::clone(&engine);
        let model = Arc::clone(&model);
        async move { engine.speak(&model, &text, None, None).await }
    });

    for _ in 0..1000 {
        if log.texts.lock().unwrap().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert_eq!(
        log.texts.lock().unwrap().clone(),
        [first, second],
        "each paragraph is a chunk, and the second is held"
    );

    // Play some of the first chunk, as the device would.
    let playback = engine
        .playback_handle()
        .await
        .expect("the engine opened a pipeline");
    for _ in 0..1000 {
        if playback.state() == State::Playing {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert_eq!(playback.state(), State::Playing, "the first chunk plays");
    let mut buf = vec![0.0_f32; 240];
    playback.render(&mut buf);

    let spoken = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let (_, json) = rx.recv_json().await.expect("a progress event");
            let spoken = json["spoken_ms"].as_u64().expect("spoken_ms");
            if spoken > 0 {
                return spoken;
            }
        }
    })
    .await
    .expect("progress with spoken_ms > 0 while the second chunk is held");
    assert_eq!(spoken, 10, "240 samples at 24 kHz");
    assert!(
        !speaking.is_finished(),
        "the second chunk is still being synthesized"
    );

    gate.add_permits(16);
    speaking
        .await
        .unwrap()
        .expect("the utterance completes once the gate opens");
}

/// Text that cannot be spoken is refused before the backend hears of it.
#[tokio::test]
async fn a_synthesis_of_nothing_is_refused_before_the_backend() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _library) = engine(dir.path());
    let log = Arc::new(ToneLog::default());
    let model = tone_model("tone", Arc::clone(&log), false).await;

    let err = engine
        .synthesize_wav(&model, "   ", None, None)
        .await
        .expect_err("nothing to say");
    assert!(matches!(err, SpeakError::EmptyText), "{err:?}");
    assert!(log.texts.lock().unwrap().is_empty());
}

/// One file is one model's voice: a chunk asked of a model other than the
/// one the synthesis began with is refused before the backend hears of it,
/// so a model switched partway fails the synthesis instead of finishing it
/// in another voice.
#[tokio::test]
async fn a_chunk_for_a_model_no_longer_loaded_is_refused() {
    let log = Arc::new(ToneLog::default());
    let model = tone_model("second", Arc::clone(&log), false).await;
    let request = SynthesizeRequest {
        text: "Hello there.",
        voice: None,
        language: None,
        speed: None,
        instructions: None,
    };
    let mut sink = crate::tts::daemon::synthesis::CollectSink::default();

    let err = super::synthesize_chunk(&model, Some("first"), None, None, &request, &mut sink)
        .await
        .expect_err("the model changed");

    assert!(
        matches!(&err, SpeakError::Synthesis(m) if m.contains("changed from first to second")),
        "{err:?}"
    );
    assert!(log.texts.lock().unwrap().is_empty());
}

/// A model that answers with no audio at all is a failure, not an empty file.
#[tokio::test]
async fn a_synthesis_with_no_audio_is_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _library) = engine(dir.path());
    let model = tone_model("silent", Arc::new(ToneLog::default()), true).await;

    let err = engine
        .synthesize_wav(&model, "Say nothing.", None, None)
        .await
        .expect_err("no audio");
    assert!(
        matches!(&err, SpeakError::Synthesis(m) if m.contains("no audio")),
        "{err:?}"
    );
}

// --- Announced before the wait -----------------------------------------------

use crate::tts::daemon::events::{AnyReceiver, EventBus, Topic};

/// Every `speaking_state` published so far, as `(is_speaking, utterance_id)`.
async fn speaking_states(rx: &mut AnyReceiver) -> Vec<(bool, Option<String>)> {
    let mut out = Vec::new();
    while let Ok(Ok((_, json))) =
        tokio::time::timeout(Duration::from_millis(50), rx.recv_json()).await
    {
        out.push((
            json["is_speaking"].as_bool().expect("is_speaking"),
            json["utterance_id"].as_str().map(str::to_string),
        ));
    }
    out
}

/// An engine publishing to a bus, with a receiver for `speaking_state`.
fn announcing_engine(dir: &std::path::Path) -> (SpeechEngine, AnyReceiver) {
    let (engine, _library) = engine(dir);
    let bus = Arc::new(EventBus::new());
    let rx = bus.subscribe(Topic::SpeakingState);
    (engine.with_events(bus), rx)
}

/// Announced on arrival, an utterance still starts once, and speaks under the
/// id it was announced with.
#[tokio::test]
async fn an_announced_utterance_starts_once_under_its_announced_id() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, mut rx) = announcing_engine(dir.path());
    let model = tone_model("tone", Arc::new(ToneLog::default()), false).await;

    let ticket = engine.arrival();
    let announced = engine.announce(ticket);
    let utterance = engine
        .speak_with(ticket, &model, "Hello there.", None, None)
        .await
        .expect("it speaks");
    drop(announced);

    assert_eq!(
        speaking_states(&mut rx).await,
        [(true, Some(utterance.id))],
        "one start under the announced id, and no stop while its audio is queued"
    );
}

/// A request announced and then given up before it speaks, refused or its
/// client gone, says it stopped.
#[tokio::test]
async fn an_announced_request_that_never_speaks_says_it_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, mut rx) = announcing_engine(dir.path());

    drop(engine.announce(engine.arrival()));

    let states = speaking_states(&mut rx).await;
    assert_eq!(states.len(), 2, "{states:?}");
    assert_eq!((states[0].0, states[1].0), (true, false), "{states:?}");
    assert_eq!(states[0].1, states[1].1, "the same utterance");
}

/// While another request is still announced, one given up says nothing: the
/// stop would drop the other's animation. The last one says it.
#[tokio::test]
async fn a_request_given_up_while_another_waits_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, mut rx) = announcing_engine(dir.path());

    let first = engine.announce(engine.arrival());
    let second = engine.announce(engine.arrival());
    drop(first);
    let states = speaking_states(&mut rx).await;
    assert_eq!(
        states.iter().map(|s| s.0).collect::<Vec<_>>(),
        [true, true],
        "no stop while the second waits: {states:?}"
    );

    drop(second);
    let states = speaking_states(&mut rx).await;
    assert_eq!(
        states.iter().map(|s| s.0).collect::<Vec<_>>(),
        [false],
        "{states:?}"
    );
}

/// A stop ends the announced requests at once rather than when their wait
/// does, and they say nothing more when it ends.
#[tokio::test]
async fn a_stop_ends_announced_requests_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, mut rx) = announcing_engine(dir.path());
    let model = tone_model("tone", Arc::new(ToneLog::default()), false).await;

    let ticket = engine.arrival();
    let announced = engine.announce(ticket);
    engine.overrule_waiting();
    let states = speaking_states(&mut rx).await;
    assert_eq!(
        states.iter().map(|s| s.0).collect::<Vec<_>>(),
        [true, false],
        "{states:?}"
    );

    let late = engine
        .speak_with(ticket, &model, "Too late.", None, None)
        .await;
    assert!(matches!(late, Err(SpeakError::Superseded)), "{late:?}");
    drop(announced);
    assert!(
        speaking_states(&mut rx).await.is_empty(),
        "said once already"
    );
}

/// An announced request a newer one overtook says no stop: the newer one is
/// speaking.
#[tokio::test]
async fn an_announced_request_overtaken_by_a_newer_one_says_no_stop() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, mut rx) = announcing_engine(dir.path());
    let model = tone_model("tone", Arc::new(ToneLog::default()), false).await;

    let older = engine.arrival();
    let announced = engine.announce(older);
    let newer = engine
        .speak_with(engine.arrival(), &model, "The newer one.", None, None)
        .await
        .expect("the newer one speaks");
    let overtaken = engine
        .speak_with(older, &model, "The older one.", None, None)
        .await;
    assert!(
        matches!(overtaken, Err(SpeakError::Superseded)),
        "{overtaken:?}"
    );
    drop(announced);

    let states = speaking_states(&mut rx).await;
    assert_eq!(
        states.iter().map(|s| s.0).collect::<Vec<_>>(),
        [true, true],
        "{states:?}"
    );
    assert_eq!(states[1].1.as_deref(), Some(newer.id.as_str()));
}
