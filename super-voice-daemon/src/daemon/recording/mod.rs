// SPDX-License-Identifier: GPL-3.0-only

mod preview;
#[cfg(feature = "wasm-backends")]
mod realtime;
pub(crate) mod state;
mod transcribe;

pub(crate) use state::{Claim, RecordingState, Stopped};

use crate::daemon::model_management::idle::PipelineLoad;
use crate::daemon::types::SuperSTTDaemon;
use crate::output::notice;
use crate::{audio::recorder::DaemonAudioRecorder, output::typer::Typer};
use anyhow::{Context, Result};
use chrono::Utc;
use log::{debug, error, info, warn};
use std::collections::VecDeque;
use std::sync::Arc;
use super_voice_shared::models::contexts::ExtraContext;
use super_voice_shared::models::protocol::{DaemonResponse, ErrorCode};
use super_voice_shared::models::recording_stop_mode::RecordingStopMode;
use super_voice_shared::theme::AudioTheme;
use tokio::time::Instant;

/// How a take records: what ends it, whether its previews are typed, the
/// cues it plays, and the context it brought. Settled when the request
/// arrives.
pub struct TakeSettings {
    pub stop_mode: RecordingStopMode,
    pub preview_typing: bool,
    pub cue_theme: AudioTheme,
    /// Context for this take alone, added to the user's.
    pub context: Option<ExtraContext>,
}

/// Say in the log that a dictation brought context of its own, by its size:
/// the words themselves may be private.
pub(crate) fn log_extra_context(extra: Option<&ExtraContext>) {
    if let Some(extra) = extra {
        debug!(
            "This dictation adds context: a {}-character prompt and {} terms",
            extra.prompt.trim().chars().count(),
            super_voice_shared::models::contexts::DictationContext::clean_vocabulary(
                &extra.vocabulary
            )
            .len()
        );
    }
}

/// `client_id` reported on every daemon-mic lifecycle event.
const CLIENT_ID: &str = "daemon_recorder";

/// Outputs from the recorder-spawn phase, consumed by the loop and collect phases.
struct RecordingSession {
    pub(super) recorder_handle: tokio::task::JoinHandle<Result<Vec<f32>>>,
    pub(super) model_processing_interval: std::time::Duration,
    // Whether previews are forced onto the model at all
    // (`ModelDefinition::force_preview_support`). Off unless its manifest
    // turns it on: every sliding-window pass is a transcription Phase 4
    // repeats, and for an online model a billed one.
    pub(super) force_preview_support: bool,
    // Whether the model streams the take through one live session
    // (`ModelDefinition::realtime`): see `realtime.rs`. Known from the
    // selected model's definition before the model has loaded.
    #[cfg(feature = "wasm-backends")]
    pub(super) realtime: bool,
    // Whether previews are typed on screen during this recording: the
    // request's override, or the daemon-wide default when it gave none.
    // Resolved once at the start so a settings change mid-recording neither
    // affects the take in progress nor gets reverted when it ends.
    pub(super) preview_typing: bool,
    pub(super) actually_typed: Arc<std::sync::Mutex<String>>,
    // Shared with the recorder's ring buffer (`get_audio_buffer_ref`), which is
    // a `parking_lot::Mutex` (Tier 3 #5).
    pub(super) preview_buffer: Arc<parking_lot::Mutex<VecDeque<f32>>>,
    // Shared with the recorder's speech-detection state. `recording` is a
    // one-way per-take latch: false after the take means no speech was ever
    // detected, so there is nothing to transcribe.
    pub(super) speech_state: Arc<parking_lot::Mutex<crate::audio::state::RecordingState>>,
    pub(super) device_sample_rate: u32,
    pub(super) start_time: Instant,
    // How long the take may run: see [`RecordingStopMode::take_limit`].
    pub(super) take_limit: std::time::Duration,
}

/// The take's audio for the final decode: cut back to its last speech.
///
/// A take's last moments are the stop key and a breath, and a batch model
/// invents words for them ("…everything works. you"). The cut is judged by the
/// same adaptive threshold the recorder used to hear the speech. A streamed
/// take never comes here: its backend heard the audio live.
fn audio_for_final_decode<'a>(
    full_audio_data: &'a [f32],
    speech_state: &parking_lot::Mutex<crate::audio::state::RecordingState>,
) -> &'a [f32] {
    let threshold = speech_state.lock().get_speech_threshold();
    let audio = crate::audio::trim::trim_trailing_silence(full_audio_data, 16000, threshold);
    if audio.len() < full_audio_data.len() {
        info!(
            "Trimmed {} ms of trailing non-speech before the final decode",
            (full_audio_data.len() - audio.len()) * 1000 / 16000
        );
    }
    audio
}

/// Why a take's final decode gave no transcript.
enum Undecoded {
    /// The transcription model failed to load: why.
    NoModel(String),
    /// The model was in, and the decode failed.
    Failed(transcribe::FinalFailure),
}

/// The recording was stopped before it captured anything.
#[derive(Debug, thiserror::Error)]
#[error("recording cancelled before it started")]
pub(crate) struct RecordingCancelled;

impl SuperSTTDaemon {
    /// Surface a recording failure through the user's configured channel.
    ///
    /// The failure is always reported to the caller through the response and
    /// the `error` event; this is the additional human-facing notice. It fires
    /// for every recording, write mode or not — the trigger is the failure, not
    /// the output mode.
    async fn surface_failure(
        &self,
        typer: &mut Typer,
        failure: &notice::Failure,
        write_mode: bool,
    ) {
        let method = self.config.read().await.notification_method;
        let mut notifier = self.notifier.lock().await;
        crate::output::notification::deliver(method, &mut notifier, typer, failure, write_mode)
            .await;
    }

    /// Tell the user the keyboard could not be set up. See
    /// [`crate::output::notification::deliver_untyped`] for why this
    /// cannot go through [`Self::surface_failure`].
    pub(crate) async fn surface_keyboard_unavailable(&self, detail: &str) {
        let method = self.config.read().await.notification_method;
        let mut notifier = self.notifier.lock().await;
        crate::output::notification::deliver_untyped(
            method,
            &mut notifier,
            &notice::Failure::keyboard_unavailable(detail),
        )
        .await;
    }

    /// Stop the take once it has run past its limit
    /// ([`RecordingStopMode::take_limit`]), and tell the user. Returns whether
    /// it did.
    ///
    /// The user may still be talking when this fires, and nothing they say
    /// from here on is recorded, so they hear of it at once rather than
    /// finding the end missing from their text. Never typed: the preview may
    /// be on screen, and a notice typed into it would be erased with it.
    async fn stop_at_take_limit(&self, session: &RecordingSession) -> bool {
        if session.start_time.elapsed() <= session.take_limit {
            return false;
        }
        let limit = notice::minutes(session.take_limit);
        warn!("The recording reached its {limit} limit; stopping it");
        self.recording.stop();
        let method = self.config.read().await.notification_method;
        let mut notifier = self.notifier.lock().await;
        crate::output::notification::deliver_untyped(
            method,
            &mut notifier,
            &notice::Failure::take_limit_reached(&limit),
        )
        .await;
        true
    }

    /// Stop the recording on request: a second press of the hotkey, or
    /// `POST /stt/transcribe/stop`. Cancels one not yet holding the
    /// microphone; tells one capturing to stop when `manual_stop` (its stop
    /// mode) allows it. It never starts a recording, whatever happened since
    /// the caller looked.
    pub(crate) fn stop_recording(&self, manual_stop: bool) -> DaemonResponse {
        const TRANSCRIBING: &str = "Transcription in progress, please wait";
        if self.recording.is_transcribing() {
            info!("{TRANSCRIBING}");
            return DaemonResponse::success().with_message(TRANSCRIBING.to_string());
        }
        if !manual_stop && self.recording.is_busy() && !self.recording.is_starting() {
            info!("Second press ignored: recording in SilenceOnly mode");
            return DaemonResponse::success()
                .with_message("Manual stop not enabled in current mode".to_string());
        }
        let message = match self.recording.stop() {
            Stopped::NothingToStop => "No recording in progress",
            Stopped::Cancelled => {
                info!("Cancelled the recording before it took the microphone");
                DaemonResponse::RECORDING_STOP_SIGNAL_MSG
            }
            Stopped::Signalled => {
                info!("🛑 Stop triggered while recording");
                DaemonResponse::RECORDING_STOP_SIGNAL_MSG
            }
            Stopped::Transcribing => TRANSCRIBING,
        };
        DaemonResponse::success().with_message(message.to_string())
    }

    /// Internal record handling implementation
    pub async fn handle_record_internal(
        &self,
        typer: &mut Typer,
        write_mode: bool,
        settings: TakeSettings,
        request_language: Option<&str>,
    ) -> DaemonResponse {
        // One recording at a time. The claim is this recording's for its whole
        // take, and dropping it, however the take ends, frees the microphone.
        let Some((claim, _)) = self.recording.reserve() else {
            warn!("Recording request rejected - already recording");
            return DaemonResponse::error_with_code(
                ErrorCode::RecordingInProgress,
                "Recording already in progress. Please wait for current recording to complete.",
            );
        };

        // The models load while the take captures: the user speaks during the
        // load rather than after it, and waits only for what is left of it
        // when they stop.
        //
        // Fail before capture when there is no model to load. The cycle could
        // only end in a discarded recording, so starting the mic would cost the
        // user a full take — beeps, speech, and all — to learn nothing. In
        // write mode the reason lands in the field they are actually looking
        // at.
        let Some(load) = self.load_for_recording().await else {
            warn!("Recording request rejected - no model loaded");
            self.surface_failure(typer, &notice::Failure::no_model_loaded(), write_mode)
                .await;
            return DaemonResponse::error_with_code(
                ErrorCode::ModelNotLoaded,
                "No model is loaded. Load a model and try again.",
            );
        };

        log_extra_context(settings.context.as_ref());

        // Silence any speech before the microphone opens: dictating over the
        // voice is asking it to stop, and the take must not hear it. A request
        // to speak still waiting for its model does not start either.
        if let Some(id) = self.tts.stop_speaking().await {
            info!("Stopped utterance {id} for the recording");
        }

        // Wait for recording to complete and return the transcription.
        let outcome = self
            .record_and_transcribe(claim, typer, write_mode, settings, request_language, load)
            .await;
        // The models' idle time starts when the take ends, however it ended.
        self.finished_using_pipeline();
        match outcome {
            // Cycle completed successfully (empty text = no speech, still success).
            Ok(Ok(transcription)) => {
                if transcription.trim().is_empty() {
                    info!("🎤 Recording completed - No speech detected");
                    DaemonResponse::success()
                        .with_message("Recording completed - No speech detected".to_string())
                        .with_transcription(String::new())
                } else {
                    info!("🎤 Recording completed: '{transcription}'");
                    DaemonResponse::success()
                        .with_message("Recording completed successfully".to_string())
                        .with_transcription(transcription)
                }
            }
            // Capture/transcription failed after the cycle started. `finalize`
            // already freed the microphone and emitted `transcribing_stopped`; surface
            // it as an error so the HTTP layer emits the `error` SSE event
            // instead of a `done` with error text as the transcription.
            Ok(Err(detail)) => {
                warn!("🎤 Recording cycle failed: {detail}");
                DaemonResponse::error(&detail)
            }
            // Setup failed before capture began: no `recording_started`/state(true)
            // went out, and the claim was dropped with the cycle.
            // Stopped before it took the microphone.
            Err(e) if e.is::<RecordingCancelled>() => DaemonResponse::success()
                .with_message(DaemonResponse::RECORDING_CANCELLED_MSG.to_string())
                .with_transcription(String::new()),
            Err(e) => {
                error!("🎤 Recording setup failed: {e}");
                self.surface_failure(
                    typer,
                    &notice::Failure::could_not_start_recording(&format!("{e:#}")),
                    write_mode,
                )
                .await;
                DaemonResponse::error(&format!("Recording failed: {e}"))
            }
        }
    }

    /// Record audio directly in daemon and transcribe
    ///
    /// # Errors
    ///
    /// Returns an error if recording setup fails, audio processing fails,
    /// or if model execution encounters a fatal error.
    ///
    /// # Panics
    ///
    /// Panics if internal locks (e.g., audio theme or buffers) are poisoned.
    /// Run one record→transcribe cycle on `claim`, which it frees when the
    /// cycle ends. The outer `Result` is a *setup* failure
    /// (before capture began); the inner
    /// `Result` is the cycle outcome — `Ok(text)` on success, `Err(detail)` when
    /// capture or transcription failed after the cycle started. A failure is
    /// never returned as `Ok` success text and is never typed into the user's
    /// window (finalize already reported it via `transcribing_stopped`).
    ///
    /// `load` is the models loading while the take captures. A failed load
    /// ends the take at once: nothing it captured could be transcribed.
    pub(crate) async fn record_and_transcribe(
        &self,
        claim: Claim<'_>,
        typer: &mut Typer,
        write_mode: bool,
        settings: TakeSettings,
        request_language: Option<&str>,
        mut load: PipelineLoad,
    ) -> Result<Result<String, String>> {
        info!("Starting direct audio recording in daemon with simplified architecture");

        // Phase 1: take the microphone and spawn the recorder.
        let extra = settings.context.clone();
        let extra = extra.as_ref();
        let (session, capture_started) = self.spawn_recorder(&claim, settings).await?;
        // Announce capture once the microphone is capturing, not when the
        // recorder is spawned: the start cue plays first, and a widget lit up
        // during it invites speech the take never hears. A recorder that fails
        // first drops the sender; the event still goes out, so the
        // `recording_stopped` Phase 3 sends for the failure follows a start.
        let _ = capture_started.await;
        self.emit_recording_started(write_mode);

        // Phase 2: stream incremental transcripts while capturing.
        //
        // A realtime model streams through one live session for the whole take,
        // which also produces the final transcript — so Phase 4 is skipped when
        // it succeeds. Every other model has no incremental output, so previews
        // are simulated by re-transcribing a sliding window — when the model
        // asks for it (`session.force_preview_support`); otherwise the loop only waits for
        // capture to end.
        //
        // A realtime session needs its model from the first frame, so a take
        // whose realtime model is still loading captures until it is in, and
        // the session then catches up on what was said meanwhile.
        #[cfg(feature = "wasm-backends")]
        let streamed = {
            if session.realtime {
                self.capture_until_loaded(&session, &mut load).await;
            }
            self.run_realtime_stream_loop(&session, typer, write_mode, request_language, extra)
                .await
        };
        #[cfg(not(feature = "wasm-backends"))]
        let streamed: Option<String> = None;
        if streamed.is_none() {
            let language = request_language;
            self.run_preview_loop(&session, &mut load, typer, write_mode, language, extra)
                .await;
        }

        // Cloned out before `collect_and_clear_preview` consumes the session.
        let speech_state = Arc::clone(&session.speech_state);

        // Phase 3: await the recorder; the mic releases here.
        let full_audio_data = match self
            .collect_and_clear_preview(&claim, session, typer, write_mode)
            .await
        {
            Ok(data) => {
                self.emit_mic_stopped();
                data
            }
            Err(e) => {
                // The recorder failed after capture started. Tell widgets the
                // mic is done, then close the cycle as a failed transcription —
                // surfaced as an inner `Err`, never as success text.
                self.emit_mic_stopped();
                warn!("Recording capture failed: {e}");
                self.finalize_recording_session(claim, "", false, Some(e.to_string()));
                self.surface_failure(
                    typer,
                    &notice::Failure::recording_failed(&format!("{e:#}")),
                    write_mode,
                )
                .await;
                return Ok(Err(format!("recording error: {e}")));
            }
        };

        // The model failed to load while the take captured, which ended it.
        if let Some(detail) = load.failure() {
            return Ok(self
                .end_take_without_model(claim, typer, write_mode, &detail)
                .await);
        }

        // Nothing was spoken. Stopping a recording without speaking is not a
        // failure, so the backend is never asked to transcribe silence and
        // nothing is typed — the cycle simply completes with no text.
        if !speech_state.lock().recording {
            info!("🎤 No speech detected during the take; skipping transcription");
            if write_mode {
                // Typed nothing, but the per-recording transcript state still
                // has to be cleared or it feeds the next recording's window
                // stitching.
                typer.reset_after_recording();
            }
            // `transcribing_started` is deliberately NOT emitted: decode never
            // begins. `finalize_recording_session` still emits `final_stt` with
            // empty text plus the closing `transcribing_stopped`, which is what
            // the events contract requires (docs/protocol/endpoints/v1/events.md).
            self.finalize_recording_session(claim, "", true, None);
            return Ok(Ok(String::new()));
        }

        // Phase 4: final transcription. A realtime session already produced it
        // while capturing; the event pair is still emitted so the events
        // contract sees a `transcribing_started` for every
        // `transcribing_stopped` `finalize_recording_session` sends.
        self.emit_transcribing_started();
        let raw_transcript = match streamed {
            Some(text) => text,
            None => match self
                .transcribe_final_once_loaded(
                    load,
                    audio_for_final_decode(&full_audio_data, &speech_state),
                    request_language,
                    extra,
                )
                .await
            {
                Ok(text) => text,
                Err(Undecoded::NoModel(detail)) => {
                    return Ok(self
                        .end_take_without_model(claim, typer, write_mode, &detail)
                        .await);
                }
                Err(Undecoded::Failed(e)) => {
                    // A transcription failure is not typed into the user's
                    // focused window (the old code typed "[STT error: …]"):
                    // finalize reports it via `transcribing_stopped`, and it
                    // surfaces as an inner `Err` so the HTTP layer emits the
                    // contract's `error` event.
                    warn!("Final transcription failed: {e}");
                    self.finalize_recording_session(claim, "", false, Some(e.to_string()));
                    // `e.origin` is why the notice can say whether the daemon or
                    // the backend is the one refusing; the chain form is the
                    // reason it shows.
                    self.surface_failure(
                        typer,
                        &notice::Failure::transcription_failed(e.origin, &e.detail()),
                        write_mode,
                    )
                    .await;
                    return Ok(Err(format!("STT error: {e}")));
                }
            },
        };

        // Best-effort cleanup of the transcript before it is typed. Both
        // producers converge here on purpose: a streamed take never calls
        // `transcribe_final`, so keeping this inside that call made a realtime
        // model the one model that typed raw text while post-processing was
        // enabled. Previews stay raw by design — only the final transcript
        // is rewritten.
        let language = self.resolve_active_language(request_language).await;
        let transcription_result = self
            .post_process_final(raw_transcript, language, extra)
            .await;

        // Phase 5: type the final transcript.
        if write_mode {
            info!("Writing transcription via {}", typer.write_method_name());
            typer.process_final_text(&transcription_result).await;
            // An assistant's prompt that holds it was spoken.
            self.assistants.sessions.dictated(&transcription_result);
        }

        // Phase 6: finalize the cycle.
        self.finalize_recording_session(claim, &transcription_result, true, None);

        Ok(Ok(transcription_result))
    }

    /// [`transcribe_final`](Self::transcribe_final), once the transcription
    /// model is in: a take may end before its load does, and the rest of the
    /// load is the one wait left to the user.
    async fn transcribe_final_once_loaded(
        &self,
        load: PipelineLoad,
        audio: &[f32],
        request_language: Option<&str>,
        extra: Option<&ExtraContext>,
    ) -> std::result::Result<String, Undecoded> {
        load.wait().await.map_err(Undecoded::NoModel)?;
        self.transcribe_final(audio, request_language, extra)
            .await
            .map_err(Undecoded::Failed)
    }

    /// End a take whose transcription model failed to load: it cannot be
    /// transcribed, so say why, as the failure notice of a recording refused
    /// for it did before recordings captured during the load.
    pub(in crate::daemon) async fn end_take_without_model(
        &self,
        claim: Claim<'_>,
        typer: &mut Typer,
        write_mode: bool,
        detail: &str,
    ) -> Result<String, String> {
        warn!("Recording ended - its model failed to load: {detail}");
        let error = format!("Could not load the model: {detail}");
        self.finalize_recording_session(claim, "", false, Some(error.clone()));
        self.surface_failure(
            typer,
            &notice::Failure::could_not_load_model(detail),
            write_mode,
        )
        .await;
        Err(error)
    }

    /// Take the microphone on `claim` and create the audio recorder. Returns
    /// the recorder and the receiver it stops on, which a stop reaches from
    /// the moment the microphone is taken, while the recorder is still being
    /// built.
    pub(super) async fn setup_recording_session(
        &self,
        claim: &Claim<'_>,
        cue_theme: AudioTheme,
    ) -> Result<(DaemonAudioRecorder, tokio::sync::broadcast::Receiver<()>)> {
        // Cancelled by a stop while it waited for its model.
        let stop = claim.start()?;

        // Create audio recorder with this take's cue theme and the current
        // volume. Construction runs the cpal cold-start (default host/output
        // device/config) and a `std::thread::sleep` device-verification spin —
        // up to ~1.6s of real blocking on a cold start. Run it on a blocking
        // thread so it doesn't park a runtime worker and stall concurrent
        // SSE/event/status handling exactly when the user starts talking
        // (audit 2 Tier 1 #3).
        let current_volume = self.get_volume_f32();
        let mut recorder = tokio::task::spawn_blocking(move || {
            DaemonAudioRecorder::new_with_theme(cue_theme, current_volume)
        })
        .await
        .context("Audio recorder construction task panicked")?
        .context("Failed to create audio recorder")?;

        // Initialize the recorder for threaded operation
        recorder.prepare_for_threaded_recording();

        Ok((recorder, stop))
    }

    /// Emit "mic capture ended" on the widget bus the instant audio capture
    /// stops (Phase 3), independent of the transcription that follows so
    /// widgets drop the live visualization immediately. The recording stays
    /// under way until it is finalized.
    pub(super) fn emit_mic_stopped(&self) {
        self.events
            .publish_recording_stopped(crate::daemon::events::RecordingStoppedEvent {
                client_id: CLIENT_ID.to_string(),
                timestamp: Utc::now().to_rfc3339(),
            });
        self.broadcast_recording_state_change(false);
    }

    /// Emit "model decode has begun" (Phase 4).
    pub(super) fn emit_transcribing_started(&self) {
        self.events
            .publish_transcribing_started(crate::daemon::events::TranscribingStartedEvent {
                client_id: CLIENT_ID.to_string(),
                timestamp: Utc::now().to_rfc3339(),
            });
    }

    /// Emit "capture has begun" (Phase 1): the mic state and `recording_started`.
    fn emit_recording_started(&self, write_mode: bool) {
        self.broadcast_recording_state_change(true);
        self.events
            .publish_recording_started(crate::daemon::events::RecordingStartedEvent {
                client_id: CLIENT_ID.to_string(),
                timestamp: Utc::now().to_rfc3339(),
                write_mode,
            });
    }

    /// Finalize the cycle: free the microphone, then emit the transcription
    /// result + the transcribing-stopped lifecycle event, so a client that
    /// reacts to them finds no recording under way. Called exactly once per
    /// cycle that reached the transcription phase.
    pub(super) fn finalize_recording_session(
        &self,
        claim: Claim<'_>,
        final_text: &str,
        transcription_success: bool,
        error: Option<String>,
    ) {
        drop(claim);

        // Final transcription text — success only; failures are conveyed by
        // transcribing_stopped's `error`. Widgets need `global_transcriptions`.
        if transcription_success {
            self.events.publish_final_stt(final_text.to_string(), 1.0);
        }

        // Transcribing phase ended (widget bus).
        self.events
            .publish_transcribing_stopped(crate::daemon::events::TranscribingStoppedEvent {
                client_id: CLIENT_ID.to_string(),
                timestamp: Utc::now().to_rfc3339(),
                transcription_success,
                error,
            });
    }
}
