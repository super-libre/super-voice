// SPDX-License-Identifier: GPL-3.0-only
//! The tools: `speak` and `listen`, and the plugin's `stop`, `status`,
//! `announce` and `session`, run on the daemon itself.
//!
//! What a session speaks and records is its own ([`Sessions`]): `stop` and
//! `status` see only the caller's.
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
use crate::assistants::sessions::{Sessions, Who};
use crate::daemon::events::Topic;
use crate::daemon::recording::{Stopped, TakeSettings};
use crate::daemon::types::SuperSTTDaemon;
use crate::output::typer::Typer;
use std::sync::Arc;
use std::time::Duration;
use super_voice_shared::models::contexts::ExtraContext;
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

/// Who a call is for: its session, whether the user lets its assistant
/// announce, and whether its `listen` calls are heard with the context they
/// bring.
pub(crate) struct Caller {
    pub(crate) who: Who,
    pub(crate) announces: bool,
    pub(crate) uses_context: bool,
}

/// What `tool` comes to: its text, or the error the model sees. `None` when
/// it was cancelled, which is answered with nothing.
pub(crate) async fn run(
    daemon: &Arc<SuperSTTDaemon>,
    tool: Tool,
    caller: &Caller,
    cancel: &CancellationToken,
) -> Option<Result<String, String>> {
    let who = &caller.who;
    match tool {
        Tool::Speak { text } => speak(daemon, who, text, cancel).await,
        Tool::Listen { question, context } => {
            let extra = take_context(question.as_deref(), context, caller.uses_context);
            listen(daemon, who, question, extra, cancel).await
        }
        Tool::Stop => Some(Ok(stop(daemon, who).await)),
        Tool::Status => Some(Ok(status(daemon, who))),
        Tool::Announce { text } if caller.announces => speak(daemon, who, text, cancel).await,
        Tool::Announce { .. } => Some(Ok(
            "Not said: the user turned announcing off for this assistant.".to_string(),
        )),
        Tool::Session { started } => {
            let sessions = &daemon.assistants.sessions;
            Some(Ok(if started {
                sessions.open(who);
                "Started.".to_string()
            } else {
                sessions.close(who);
                "Ended.".to_string()
            }))
        }
    }
}

async fn speak(
    daemon: &Arc<SuperSTTDaemon>,
    who: &Who,
    text: String,
    cancel: &CancellationToken,
) -> Option<Result<String, String>> {
    let mut spoken = start_speaking(daemon, text);
    tokio::select! {
        biased;
        joined = &mut spoken => Some(spoken_outcome(joined).map(|utterance| {
            if let Some(utterance) = utterance {
                daemon.assistants.sessions.spoke(who, utterance);
            }
            "Speaking.".to_string()
        })),
        // The speech goes on: it is on a task of its own.
        () = cancel.cancelled() => None,
    }
}

/// Stop `who`'s own speech, if it still plays, and end its own recording,
/// which is transcribed all the same. Another session's are left alone.
async fn stop(daemon: &SuperSTTDaemon, who: &Who) -> String {
    let sessions = &daemon.assistants.sessions;
    let mut stopped = Vec::new();
    if let Some(utterance) = sessions.utterance(who)
        && daemon.tts.speech.current().as_deref() == Some(utterance.as_str())
    {
        silence_if_speaking(daemon, &utterance).await;
        stopped.push("speech");
    }
    if sessions.is_recording(who)
        && matches!(
            daemon.recording.stop(),
            Stopped::Signalled | Stopped::Cancelled
        )
    {
        stopped.push("recording");
    }
    if stopped.is_empty() {
        "Nothing of this session's was playing or recording.".to_string()
    } else {
        format!("Stopped its {}.", stopped.join(" and "))
    }
}

/// Whether `who`'s own speech plays, and whether it records, as JSON.
fn status(daemon: &SuperSTTDaemon, who: &Who) -> String {
    let sessions = &daemon.assistants.sessions;
    let speaking = sessions.utterance(who).is_some_and(|utterance| {
        daemon.tts.speech.current().as_deref() == Some(utterance.as_str())
    });
    serde_json::json!({ "speaking": speaking, "listening": sessions.is_recording(who) }).to_string()
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

/// What a `listen` take is heard with besides the user's own context: the
/// conversation's, with the question it answers at the front of its prompt.
/// Nothing when the user turned the conversation's context off for the
/// assistant.
fn take_context(
    question: Option<&str>,
    context: Option<ExtraContext>,
    uses_context: bool,
) -> Option<ExtraContext> {
    if !uses_context {
        return None;
    }
    let mut context = context.unwrap_or_default();
    if let Some(question) = question.map(str::trim).filter(|q| !q.is_empty()) {
        let answering = format!("The user is answering: {question}");
        let prompt = context.prompt.trim();
        context.prompt = if prompt.is_empty() {
            answering
        } else {
            format!("{answering}\n{prompt}")
        };
    }
    (!context.is_empty()).then_some(context)
}

async fn listen(
    daemon: &Arc<SuperSTTDaemon>,
    who: &Who,
    question: Option<String>,
    extra: Option<ExtraContext>,
    cancel: &CancellationToken,
) -> Option<Result<String, String>> {
    if let Some(question) = question {
        match ask(daemon, who, question, cancel).await {
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
    let _recording = Sessions::recording(&daemon.assistants.sessions, who);
    let mut typer = Typer::without_keyboard();
    let take = daemon.handle_record_internal(
        &mut typer,
        false,
        TakeSettings {
            // Ends when the user stops talking, and their shortcut still
            // stops it early.
            stop_mode: RecordingStopMode::SilenceAndManual,
            preview_typing: false,
            cue_theme: daemon.get_audio_theme(),
            context: extra,
        },
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
async fn ask(
    daemon: &Arc<SuperSTTDaemon>,
    who: &Who,
    question: String,
    cancel: &CancellationToken,
) -> Asked {
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
    daemon.assistants.sessions.spoke(who, utterance.clone());
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
    use crate::assistants::calls::Caller as By;
    use crate::assistants::sessions::Named;
    use crate::daemon::types::test_daemon;

    fn caller() -> Caller {
        Caller {
            who: Who {
                caller: By::Key("k".to_string()),
                named: Named::default(),
                connection: None,
            },
            announces: true,
            uses_context: true,
        }
    }

    /// A take is heard with the conversation's context and the question it
    /// answers, unless the user turned the conversation's context off.
    #[test]
    fn a_take_is_heard_with_its_conversation() {
        let context = || {
            Some(ExtraContext {
                prompt: "About the release.".to_string(),
                vocabulary: vec!["agy".to_string()],
            })
        };
        let heard = take_context(Some("Which one? "), context(), true).expect("a context");
        assert_eq!(
            heard.prompt,
            "The user is answering: Which one?\nAbout the release."
        );
        assert_eq!(heard.vocabulary, ["agy"]);
        let asked = take_context(Some("Which one?"), None, true).expect("the question");
        assert_eq!(asked.prompt, "The user is answering: Which one?");
        assert_eq!(take_context(None, None, true), None);
        assert_eq!(take_context(Some("Which one?"), context(), false), None);
    }

    #[tokio::test]
    async fn listen_with_no_model_says_so_and_records_nothing() {
        let daemon = Arc::new(test_daemon().await);
        let outcome = run(
            &daemon,
            Tool::Listen {
                question: None,
                context: None,
            },
            &caller(),
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
            &caller(),
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
        let outcome = run(
            &daemon,
            Tool::Listen {
                question: None,
                context: None,
            },
            &caller(),
            &cancel,
        )
        .await;
        assert_eq!(outcome, None);
        assert!(!daemon.recording.is_busy());
    }

    /// `stop` and `status` see only the caller's own speech and recording.
    #[tokio::test]
    async fn stop_and_status_see_only_the_callers_own() {
        let daemon = Arc::new(test_daemon().await);
        let mine = caller();
        let cancel = CancellationToken::new();
        let Some(Ok(status)) = run(&daemon, Tool::Status, &mine, &cancel).await else {
            panic!("status answers");
        };
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&status).expect("JSON"),
            serde_json::json!({ "speaking": false, "listening": false })
        );
        // An utterance of its own that is not the one playing is not stopped.
        daemon
            .assistants
            .sessions
            .spoke(&mine.who, "utt_gone".to_string());
        let stopped = run(&daemon, Tool::Stop, &mine, &cancel).await;
        assert_eq!(
            stopped,
            Some(Ok(
                "Nothing of this session's was playing or recording.".to_string()
            ))
        );
    }

    /// With announcing off, `announce` says nothing and says why.
    #[tokio::test]
    async fn announce_says_nothing_when_announcing_is_off() {
        let daemon = Arc::new(test_daemon().await);
        let quiet = Caller {
            announces: false,
            ..caller()
        };
        let outcome = run(
            &daemon,
            Tool::Announce {
                text: "Claude needs your permission.".to_string(),
            },
            &quiet,
            &CancellationToken::new(),
        )
        .await;
        let Some(Ok(text)) = outcome else {
            panic!("expected an answer, got {outcome:?}");
        };
        assert!(text.contains("turned announcing off"), "{text}");
        assert!(daemon.tts.speech.current().is_none());
    }

    /// A session opens and closes on its plugin's word.
    #[tokio::test]
    async fn a_session_opens_and_closes() {
        let daemon = Arc::new(test_daemon().await);
        let cancel = CancellationToken::new();
        run(&daemon, Tool::Session { started: true }, &caller(), &cancel).await;
        assert_eq!(daemon.assistants.sessions.list().len(), 1);
        run(
            &daemon,
            Tool::Session { started: false },
            &caller(),
            &cancel,
        )
        .await;
        assert!(daemon.assistants.sessions.list().is_empty());
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
