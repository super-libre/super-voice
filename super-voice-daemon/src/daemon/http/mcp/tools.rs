// SPDX-License-Identifier: GPL-3.0-only
//! The tools: `speak` and `listen`, run on the daemon itself.
//!
//! A tool runs on a task of its own, so it ends as the daemon's work does,
//! never midway because its client went away. It is cancelled through its
//! [`CancellationToken`], by `notifications/cancelled`, by its connection
//! closing, or by its key being revoked, and then answers nothing:
//!
//! 1. `speak` goes on speaking, as `POST /v1/tts/speak` does when its client
//!    hangs up. Only the wait for it ends.
//! 2. `listen` asking its question stops the question, if it is still the one
//!    speaking.
//! 3. `listen` recording stops the recording, and the take is finished all
//!    the same, so the microphone is freed as at any other stop.

use super::protocol::Tool;
use crate::daemon::events::Topic;
use crate::daemon::types::SuperSTTDaemon;
use crate::output::typer::Typer;
use std::sync::Arc;
use std::time::Duration;
use super_voice_shared::models::protocol::{DaemonResponse, ErrorCode};
use super_voice_shared::models::recording_stop_mode::RecordingStopMode;
use super_voice_shared::tts::models::protocol::{
    DaemonResponse as SpeechResponse, ErrorCode as SpeechErrorCode,
};
use tokio::task::{JoinError, JoinHandle};
use tokio_util::sync::CancellationToken;

/// How often `listen` looks again whether its question is still playing,
/// besides when speech starts or stops.
const TICK: Duration = Duration::from_millis(100);

/// What `tool` comes to: its text, or the error the model sees. `None` when
/// it was cancelled, which is answered with nothing.
pub(crate) async fn run(
    daemon: &Arc<SuperSTTDaemon>,
    tool: Tool,
    cancel: &CancellationToken,
) -> Option<Result<String, String>> {
    match tool {
        Tool::Speak { text } => speak(daemon, text, cancel).await,
        Tool::Listen { question } => listen(daemon, question, cancel).await,
    }
}

async fn speak(
    daemon: &Arc<SuperSTTDaemon>,
    text: String,
    cancel: &CancellationToken,
) -> Option<Result<String, String>> {
    let mut spoken = start_speaking(daemon, text);
    tokio::select! {
        biased;
        joined = &mut spoken => Some(spoken_outcome(joined).map(|_| "Speaking.".to_string())),
        // The speech goes on: it is on a task of its own.
        () = cancel.cancelled() => None,
    }
}

/// `text` spoken on a task of its own, so it is never dropped midway: a
/// speech dropped while it is produced stops speaking.
fn start_speaking(daemon: &Arc<SuperSTTDaemon>, text: String) -> JoinHandle<SpeechResponse> {
    let tts = daemon.tts.clone();
    tokio::spawn(async move { tts.handle_speak(text).await })
}

/// The utterance a finished speech is speaking, or what the model is told
/// when it could not speak.
fn spoken_outcome(joined: Result<SpeechResponse, JoinError>) -> Result<Option<String>, String> {
    let response = joined.map_err(|e| format!("Super Voice could not speak: {e}"))?;
    if response.status == "success" {
        return Ok(response.utterance_id);
    }
    Err(match response.error_code {
        Some(SpeechErrorCode::ModelNotLoaded) => {
            "Super Voice has no text to speech model loaded. The user can choose one in the \
             Super Voice app."
                .to_string()
        }
        Some(SpeechErrorCode::RecordingInProgress) => {
            "Super Voice is recording the user, so it cannot speak until the recording ends."
                .to_string()
        }
        Some(SpeechErrorCode::VoiceRequired) => {
            "The text to speech model speaks only in a voice the user chooses, and none is \
             chosen. The user can choose one in the Super Voice app."
                .to_string()
        }
        _ => response
            .message
            .unwrap_or_else(|| "Super Voice could not speak.".to_string()),
    })
}

/// How asking the question went.
enum Asked {
    /// It was spoken, and has finished.
    Done,
    Failed(String),
    Cancelled,
}

async fn listen(
    daemon: &Arc<SuperSTTDaemon>,
    question: Option<String>,
    cancel: &CancellationToken,
) -> Option<Result<String, String>> {
    if let Some(question) = question {
        match ask(daemon, question, cancel).await {
            Asked::Done => {}
            Asked::Failed(e) => return Some(Err(e)),
            Asked::Cancelled => return None,
        }
    }
    if cancel.is_cancelled() {
        return None;
    }
    // Never the toggling `record` command: that would stop someone else's
    // recording. This one reserves the microphone or is refused.
    let mut typer = Typer::without_keyboard();
    let take = daemon.handle_record_internal(
        &mut typer,
        false,
        // Ends when the user stops talking, and their shortcut still stops
        // it early.
        RecordingStopMode::SilenceAndManual,
        false,
        daemon.get_audio_theme(),
        None,
    );
    let mut take = std::pin::pin!(take);
    let response = tokio::select! {
        biased;
        response = &mut take => response,
        () = cancel.cancelled() => {
            // Cancels it while it is starting; stops its capture after. The
            // take is finished, never dropped, so the microphone is freed as
            // at any other stop.
            let _ = daemon.recording.stop();
            let _ = take.await;
            return None;
        }
    };
    Some(heard(response))
}

/// What the user said, or what the model is told when nothing was recorded.
fn heard(response: DaemonResponse) -> Result<String, String> {
    if response.status == "success" {
        return Ok(response
            .transcription
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| "(no speech detected)".to_string()));
    }
    Err(match response.error_code {
        Some(ErrorCode::ModelNotLoaded) => {
            "Super Voice has no speech to text model loaded. The user can choose one in the \
             Super Voice app."
                .to_string()
        }
        Some(ErrorCode::RecordingInProgress) => {
            "Super Voice is already recording. Try again once that recording ends.".to_string()
        }
        _ => response
            .message
            .unwrap_or_else(|| "Super Voice could not record.".to_string()),
    })
}

/// Speak `question`, and wait for it to finish playing, so the recording
/// after it hears the answer and not the question.
async fn ask(daemon: &Arc<SuperSTTDaemon>, question: String, cancel: &CancellationToken) -> Asked {
    // Before speaking, so the question's end is never missed.
    let mut states = daemon.events.subscribe(Topic::SpeakingState);
    let mut spoken = start_speaking(daemon, question);
    let utterance = tokio::select! {
        biased;
        joined = &mut spoken => match spoken_outcome(joined) {
            Ok(utterance) => utterance,
            Err(e) => return Asked::Failed(e),
        },
        () = cancel.cancelled() => {
            silence_once_spoken(Arc::clone(daemon), spoken);
            return Asked::Cancelled;
        }
    };
    // A question a newer speech overtook was never spoken, and has no
    // utterance of its own playing.
    let Some(utterance) = utterance else {
        return Asked::Done;
    };
    let mut listening = true;
    loop {
        if daemon.tts.speech.current().as_deref() != Some(utterance.as_str()) {
            return Asked::Done;
        }
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                silence_if_speaking(daemon, &utterance).await;
                return Asked::Cancelled;
            }
            // Speech started or stopped: look again.
            changed = states.recv_json_str(), if listening => {
                if matches!(changed, Err(tokio::sync::broadcast::error::RecvError::Closed)) {
                    listening = false;
                }
            }
            () = tokio::time::sleep(TICK) => {}
        }
    }
}

/// Stop `utterance`, if it is still the one speaking. Speech the user or
/// another client started since is left alone.
async fn silence_if_speaking(daemon: &SuperSTTDaemon, utterance: &str) {
    if daemon.tts.speech.current().as_deref() == Some(utterance) {
        let _ = daemon.tts.stop_speaking().await;
    }
}

/// Stop the question `spoken` is still producing once it plays: the call
/// asking it was cancelled, and the question would otherwise be heard after.
fn silence_once_spoken(daemon: Arc<SuperSTTDaemon>, spoken: JoinHandle<SpeechResponse>) {
    tokio::spawn(async move {
        if let Ok(Some(utterance)) = spoken_outcome(spoken.await) {
            silence_if_speaking(&daemon, &utterance).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::types::test_daemon;

    #[tokio::test]
    async fn listen_with_no_model_says_so_and_records_nothing() {
        let daemon = Arc::new(test_daemon().await);
        let outcome = run(
            &daemon,
            Tool::Listen { question: None },
            &CancellationToken::new(),
        )
        .await;
        let Some(Err(text)) = outcome else {
            panic!("expected an error, got {outcome:?}");
        };
        assert!(text.contains("speech to text model"), "{text}");
        assert!(!daemon.recording.is_busy());
    }

    #[tokio::test]
    async fn speak_with_no_model_is_an_error() {
        let daemon = Arc::new(test_daemon().await);
        let outcome = run(
            &daemon,
            Tool::Speak {
                text: "Hello.".to_string(),
            },
            &CancellationToken::new(),
        )
        .await;
        assert!(matches!(outcome, Some(Err(_))), "{outcome:?}");
    }

    /// A call cancelled before it records answers nothing, and never opens
    /// the microphone.
    #[tokio::test]
    async fn a_cancelled_listen_answers_nothing() {
        let daemon = Arc::new(test_daemon().await);
        let cancel = CancellationToken::new();
        cancel.cancel();
        let outcome = run(&daemon, Tool::Listen { question: None }, &cancel).await;
        assert_eq!(outcome, None);
        assert!(!daemon.recording.is_busy());
    }

    #[test]
    fn what_was_heard_is_the_text_or_says_there_was_none() {
        let said = DaemonResponse::success().with_transcription("Yes.".to_string());
        assert_eq!(heard(said), Ok("Yes.".to_string()));
        let silence = DaemonResponse::success().with_transcription("  ".to_string());
        assert_eq!(heard(silence), Ok("(no speech detected)".to_string()));
        let busy = DaemonResponse::error_with_code(ErrorCode::RecordingInProgress, "busy");
        assert!(heard(busy).is_err_and(|e| e.contains("already recording")));
    }
}
