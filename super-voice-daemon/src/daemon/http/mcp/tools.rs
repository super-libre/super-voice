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

use super::protocol::{Hook, SessionEvent, Tool, TurnEvent};
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
/// announce, whether its `listen` calls are heard with the context they
/// bring, whether its plugin shows Talk, and the hook that made the call, if
/// one did.
pub(crate) struct Caller {
    pub(crate) who: Who,
    pub(crate) announces: bool,
    pub(crate) uses_context: bool,
    pub(crate) shows_talk: bool,
    pub(crate) hook: Option<Hook>,
}

/// Why `speak` and `listen` are refused in a turn the user typed.
const NOT_SPOKEN_TO: &str = "The user typed this prompt, so answer in writing. Super Voice \
    speaks and listens only while the user is talking to you by voice.";

/// What the assistant is told beside a prompt the user said aloud.
const SPOKEN_NOTE: &str = "The user said this aloud, through Super Voice. Your reply will be \
    read aloud to them: keep it short and plain to listen to, and end with any question you have.";

/// The refusal for `who`, when its plugin said the turn under way was typed.
fn not_spoken_to(daemon: &SuperSTTDaemon, who: &Who) -> Option<String> {
    (daemon.assistants.sessions.spoken(who) == Some(false)).then(|| NOT_SPOKEN_TO.to_string())
}

/// `answer`, or for a hook, which reads the text as its own output,
/// nothing.
fn for_caller(caller: &Caller, answer: String) -> String {
    if caller.hook.is_some() {
        String::new()
    } else {
        answer
    }
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
    let sessions = &daemon.assistants.sessions;
    match tool {
        Tool::Speak { asked: false, .. } | Tool::Listen { asked: false, .. }
            if not_spoken_to(daemon, who).is_some() =>
        {
            not_spoken_to(daemon, who).map(Err)
        }
        Tool::Speak { text, .. } => speak(daemon, who, text, cancel).await,
        Tool::Listen {
            question, context, ..
        } => {
            let extra = take_context(question.as_deref(), context, caller.uses_context);
            listen(daemon, who, question, extra, cancel).await
        }
        Tool::Stop => Some(Ok(stop(daemon, who).await)),
        Tool::Status => Some(Ok(status(daemon, who, caller.shows_talk))),
        Tool::Announce { text } if caller.announces => speak(daemon, who, text, cancel)
            .await
            .map(|said| said.map(|answer| for_caller(caller, answer))),
        Tool::Announce { .. } => Some(Ok(for_caller(
            caller,
            "Not said: the user turned announcing off for this assistant.".to_string(),
        ))),
        Tool::Session(event) => Some(Ok(for_caller(
            caller,
            match event {
                SessionEvent::Start => {
                    sessions.open(who);
                    "Started."
                }
                SessionEvent::End => {
                    sessions.close(who);
                    "Ended."
                }
                SessionEvent::Seen => {
                    sessions.touch(who);
                    "Seen."
                }
            }
            .to_string(),
        ))),
        Tool::Turn(TurnEvent::Prompt { text, .. }) if read_request(&text).is_some() => {
            let request = read_request(&text)?;
            // A turn of its own, typed, that ends before the assistant runs.
            sessions.set_spoken(who, false);
            let outcome = read_again(daemon, who, request).await;
            Some(Ok(read_answer(caller.hook, outcome)))
        }
        Tool::Turn(TurnEvent::Prompt { text, spoken }) => {
            // The same prompt said again keeps its turn: its dictation is
            // used up by now.
            let again = sessions.prompted(who, &text);
            let spoken = spoken
                .or(again)
                .unwrap_or_else(|| sessions.was_dictated(&text, std::time::Instant::now()));
            sessions.set_spoken(who, spoken);
            Some(Ok(prompt_answer(caller.hook, spoken, again.is_none())))
        }
        Tool::Turn(TurnEvent::Reply { text }) => {
            sessions.replied(who, &text);
            let read = sessions.spoken(who) == Some(true) && !text.trim().is_empty();
            if read {
                match speak(daemon, who, text, cancel).await {
                    Some(Ok(_)) => {}
                    other => return other,
                }
            }
            Some(Ok(for_caller(
                caller,
                serde_json::json!({ "read": read }).to_string(),
            )))
        }
    }
}

/// The answer to a prompt's turn: for a Codex hook, the context that goes
/// beside a spoken prompt, or nothing; for a plugin, whether it was spoken
/// and, when the turn is `new`, that context. A plugin whose hook runs before
/// each model call adds it to the turn once.
fn prompt_answer(hook: Option<Hook>, spoken: bool, new: bool) -> String {
    match hook {
        Some(Hook::Codex) if spoken => serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "UserPromptSubmit",
                "additionalContext": SPOKEN_NOTE,
            },
        })
        .to_string(),
        Some(Hook::Codex) => String::new(),
        None => serde_json::json!({
            "spoken": spoken,
            "note": (spoken && new).then_some(SPOKEN_NOTE),
        })
        .to_string(),
    }
}

/// What a `/read` prompt asks for.
#[derive(Debug, PartialEq)]
enum ReadRequest {
    /// Plain `/read`: the last reply, or to stop what the session says.
    Last,
    /// `/read 3`: the reply that many before the last one.
    Back(usize),
    /// `/read` with something other than a number from 0 to 9.
    Unclear,
}

/// How a prompt asks to read a reply again: as a command, or as the
/// plugin's skill is named, typed or picked from the assistant's list: agy
/// names it `/super-voice:read`, and Codex `$super-voice:read`.
const READ_COMMANDS: [&str; 4] = ["/read", "/super-voice:read", "$read", "$super-voice:read"];

/// What `prompt` asks, when it is one of [`READ_COMMANDS`], alone or with
/// what follows it after a space.
fn read_request(prompt: &str) -> Option<ReadRequest> {
    let prompt = prompt.trim();
    let rest = READ_COMMANDS
        .iter()
        .find_map(|command| prompt.strip_prefix(command))?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim();
    Some(match rest.parse::<usize>() {
        _ if rest.is_empty() => ReadRequest::Last,
        Ok(back) if rest.len() == 1 => ReadRequest::Back(back),
        _ => ReadRequest::Unclear,
    })
}

/// What `/read` takes.
const READ_USAGE: &str = "Read takes a number from 0, the last reply, to 9. Alone, it reads \
    the last reply, or stops the one being read.";

/// How long `/read` waits for its reply to start being read, so what keeps
/// it from being read is told at once. A long reply goes on being made after.
const READ_START: Duration = Duration::from_secs(2);

/// Read one of `who`'s kept replies aloud again, or stop the one it reads.
/// What to tell the user that it did, or why it could not.
async fn read_again(
    daemon: &Arc<SuperSTTDaemon>,
    who: &Who,
    request: ReadRequest,
) -> Result<String, String> {
    let back = match request {
        ReadRequest::Unclear => return Err(READ_USAGE.to_string()),
        ReadRequest::Last if is_speaking(daemon, who) => {
            stop(daemon, who).await;
            return Ok("Stopped reading.".to_string());
        }
        ReadRequest::Last => 0,
        ReadRequest::Back(back) => back,
    };
    let reply = match daemon.assistants.sessions.reply(who, back) {
        Ok(reply) => reply,
        Err(0) => return Err("There is no reply to read yet.".to_string()),
        Err(1) => return Err("There is only one reply to read: 0.".to_string()),
        Err(count) => {
            return Err(format!(
                "There are {count} replies to read: 0 to {}.",
                count - 1
            ));
        }
    };
    let mut spoken = start_speaking(daemon, who, reply);
    let started = match tokio::time::timeout(READ_START, &mut spoken).await {
        Ok(joined) => spoken_outcome(joined).map(|_| ()),
        // Still being made: it is read as it is.
        Err(_) => Ok(()),
    };
    started.map(|()| {
        if back == 0 {
            "Reading the last reply aloud.".to_string()
        } else {
            format!("Reading reply {back} aloud.")
        }
    })
}

/// The answer to a `/read` prompt, which the assistant never sees: for a
/// Codex hook, the prompt blocked with what was done, or why not, as the
/// reason; for a plugin, that as what answered it, and whether it `failed`,
/// so a plugin that can only show it as an error shows it only then.
fn read_answer(hook: Option<Hook>, outcome: Result<String, String>) -> String {
    let failed = outcome.is_err();
    let said = outcome.unwrap_or_else(|error| error);
    match hook {
        Some(Hook::Codex) => serde_json::json!({ "decision": "block", "reason": said }),
        None => serde_json::json!({ "spoken": false, "answered": said, "failed": failed }),
    }
    .to_string()
}

async fn speak(
    daemon: &Arc<SuperSTTDaemon>,
    who: &Who,
    text: String,
    cancel: &CancellationToken,
) -> Option<Result<String, String>> {
    let mut spoken = start_speaking(daemon, who, text);
    tokio::select! {
        biased;
        // The utterance is the session's from its start: a newer one of the
        // session's may have started by now, and stays the one recorded.
        joined = &mut spoken => Some(spoken_outcome(joined).map(|_| "Speaking.".to_string())),
        // The speech goes on: it is on a task of its own.
        () = cancel.cancelled() => None,
    }
}

/// Stop `who`'s own speech, if it still plays, and end its own recording,
/// which is transcribed all the same. Another session's are left alone.
async fn stop(daemon: &SuperSTTDaemon, who: &Who) -> String {
    let sessions = &daemon.assistants.sessions;
    let mut stopped = Vec::new();
    // Its speech still waiting for the model, or still being made, is called
    // off; what of it already plays is silenced.
    let making = sessions.speaking(who);
    if let Some(task) = &making {
        task.abort();
    }
    let playing = sessions
        .utterance(who)
        .filter(|utterance| daemon.tts.speech.current().as_deref() == Some(utterance.as_str()));
    if let Some(utterance) = &playing {
        silence_if_speaking(daemon, utterance).await;
    }
    if making.is_some() || playing.is_some() {
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

/// Whether `who`'s own speech plays, or is on its way: waiting for the
/// model, or being made.
fn is_speaking(daemon: &SuperSTTDaemon, who: &Who) -> bool {
    let sessions = &daemon.assistants.sessions;
    sessions.speaking(who).is_some()
        || sessions.utterance(who).is_some_and(|utterance| {
            daemon.tts.speech.current().as_deref() == Some(utterance.as_str())
        })
}

/// Whether `who`'s own speech plays and whether it records, and whether the
/// user shows its plugin's Talk, as JSON.
fn status(daemon: &SuperSTTDaemon, who: &Who, shows_talk: bool) -> String {
    serde_json::json!({
        "speaking": is_speaking(daemon, who),
        "listening": daemon.assistants.sessions.is_recording(who),
        "talk": shows_talk,
    })
    .to_string()
}

/// `text` spoken on a task of its own, so it is never dropped midway: a
/// speech dropped while it is produced stops speaking. The task is `who`'s
/// from the moment it is asked for, and its utterance from the moment it
/// starts, so `who`'s `stop` finds the speech while it waits for the model
/// and while the rest of a long text is still being made.
fn start_speaking(
    daemon: &Arc<SuperSTTDaemon>,
    who: &Who,
    text: String,
) -> JoinHandle<SpeechResponse> {
    let tts = daemon.tts.clone();
    let owner = Arc::clone(daemon);
    let speaker = who.clone();
    let task = tokio::spawn(async move {
        tts.handle_speak_noting(text, move |utterance| {
            owner
                .assistants
                .sessions
                .spoke(&speaker, utterance.to_string());
        })
        .await
    });
    daemon
        .assistants
        .sessions
        .asked_to_speak(who, task.abort_handle());
    task
}

/// The utterance a finished speech is speaking, or what the model is told
/// when it could not speak. A speech stopped before it played has none.
fn spoken_outcome(joined: Result<SpeechResponse, JoinError>) -> Result<Option<String>, String> {
    let response = match joined {
        Ok(response) => response,
        // Stopped before anything of it played: nothing of it is playing.
        Err(e) if e.is_cancelled() => return Ok(None),
        Err(e) => return Err(format!("Super Voice could not speak: {e}")),
    };
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
    let mut spoken = start_speaking(daemon, who, question);
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
            shows_talk: true,
            hook: None,
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
                asked: false,
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
                asked: false,
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
                asked: false,
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
            serde_json::json!({ "speaking": false, "listening": false, "talk": true })
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

    /// A session's speech is its own from the moment it is asked for:
    /// `status` and `stop` see it while it still waits for the model.
    #[tokio::test]
    async fn stop_reaches_speech_still_waiting_to_start() {
        let daemon = Arc::new(test_daemon().await);
        let mine = caller();
        let waiting = tokio::spawn(std::future::pending::<SpeechResponse>());
        daemon
            .assistants
            .sessions
            .asked_to_speak(&mine.who, waiting.abort_handle());
        assert!(is_speaking(&daemon, &mine.who));
        assert!(!is_speaking(
            &daemon,
            &Who {
                named: Named {
                    id: Some("other".into()),
                    ..Named::default()
                },
                ..caller().who
            }
        ));

        assert_eq!(stop(&daemon, &mine.who).await, "Stopped its speech.");
        let joined = waiting.await;
        assert!(joined.as_ref().is_err_and(JoinError::is_cancelled));
        assert_eq!(
            spoken_outcome(joined),
            Ok(None),
            "a stopped speech has nothing playing"
        );
        assert!(!is_speaking(&daemon, &mine.who));
        assert_eq!(
            stop(&daemon, &mine.who).await,
            "Nothing of this session's was playing or recording."
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
        run(
            &daemon,
            Tool::Session(SessionEvent::Start),
            &caller(),
            &cancel,
        )
        .await;
        assert_eq!(daemon.assistants.sessions.list().len(), 1);
        run(
            &daemon,
            Tool::Session(SessionEvent::End),
            &caller(),
            &cancel,
        )
        .await;
        assert!(daemon.assistants.sessions.list().is_empty());
        run(
            &daemon,
            Tool::Session(SessionEvent::Seen),
            &caller(),
            &cancel,
        )
        .await;
        assert_eq!(daemon.assistants.sessions.list().len(), 1);
    }

    /// A typed turn refuses speaking and listening; a spoken one, or one no
    /// plugin has said anything of, does not. A prompt holding the user's
    /// dictation is spoken.
    #[tokio::test]
    async fn a_typed_turn_is_answered_in_writing() {
        let daemon = Arc::new(test_daemon().await);
        let cancel = CancellationToken::new();
        let prompt = |text: &str, spoken| {
            Tool::Turn(TurnEvent::Prompt {
                text: text.to_string(),
                spoken,
            })
        };
        let typed = run(&daemon, prompt("Hi.", Some(false)), &caller(), &cancel)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&typed).unwrap(),
            serde_json::json!({ "spoken": false, "note": null })
        );
        let refused = run(
            &daemon,
            Tool::Speak {
                text: "Hello.".to_string(),
                asked: false,
            },
            &caller(),
            &cancel,
        )
        .await;
        assert_eq!(refused, Some(Err(NOT_SPOKEN_TO.to_string())));

        daemon.assistants.sessions.dictated("Open the notes");
        let spoken = run(&daemon, prompt("open the notes.", None), &caller(), &cancel)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&spoken).unwrap(),
            serde_json::json!({ "spoken": true, "note": SPOKEN_NOTE })
        );
        assert_eq!(daemon.assistants.sessions.spoken(&caller().who), Some(true));
        // Said again before each model call, it stays spoken, and the note
        // is added once.
        let again = run(&daemon, prompt("open the notes.", None), &caller(), &cancel)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&again).unwrap(),
            serde_json::json!({ "spoken": true, "note": null })
        );
    }

    /// A Codex hook reads the answer as its own output: the context beside a
    /// spoken prompt, and nothing else.
    #[tokio::test]
    async fn a_hook_gets_its_own_output() {
        let daemon = Arc::new(test_daemon().await);
        let cancel = CancellationToken::new();
        let hook = Caller {
            hook: Some(Hook::Codex),
            ..caller()
        };
        let prompt = |spoken| {
            Tool::Turn(TurnEvent::Prompt {
                text: "Hi.".to_string(),
                spoken: Some(spoken),
            })
        };
        let spoken = run(&daemon, prompt(true), &hook, &cancel)
            .await
            .unwrap()
            .unwrap();
        let output: serde_json::Value = serde_json::from_str(&spoken).unwrap();
        assert_eq!(
            output["hookSpecificOutput"]["hookEventName"],
            "UserPromptSubmit"
        );
        assert_eq!(
            output["hookSpecificOutput"]["additionalContext"],
            SPOKEN_NOTE
        );
        assert_eq!(
            run(&daemon, prompt(false), &hook, &cancel).await,
            Some(Ok(String::new()))
        );
        // A typed turn's reply is not read, and the hook hears nothing.
        let reply = Tool::Turn(TurnEvent::Reply {
            text: "Done.".to_string(),
        });
        assert_eq!(
            run(&daemon, reply, &hook, &cancel).await,
            Some(Ok(String::new()))
        );
        let seen = Tool::Session(SessionEvent::Seen);
        assert_eq!(
            run(&daemon, seen, &hook, &cancel).await,
            Some(Ok(String::new()))
        );
    }

    /// `/read`, or `$read`, alone or with one digit; anything else after it
    /// is unclear, and a prompt that only starts with the word is not one.
    #[test]
    fn read_asks_for_a_reply_by_how_far_back_it_is() {
        assert_eq!(read_request(" /read "), Some(ReadRequest::Last));
        assert_eq!(read_request("$read"), Some(ReadRequest::Last));
        assert_eq!(read_request("/read 0"), Some(ReadRequest::Back(0)));
        assert_eq!(read_request("$read  9\n"), Some(ReadRequest::Back(9)));
        assert_eq!(
            read_request("$super-voice:read 2"),
            Some(ReadRequest::Back(2))
        );
        assert_eq!(read_request("/super-voice:read"), Some(ReadRequest::Last));
        assert_eq!(read_request("/read 10"), Some(ReadRequest::Unclear));
        assert_eq!(read_request("/read +1"), Some(ReadRequest::Unclear));
        assert_eq!(read_request("/read the notes"), Some(ReadRequest::Unclear));
        assert_eq!(read_request("/readme"), None);
        assert_eq!(read_request("read 1"), None);
        assert_eq!(read_request("Please /read 1"), None);
    }

    /// A `/read` prompt is answered by Super Voice, as a typed turn the
    /// assistant never sees: Codex's hook blocks it with the answer.
    #[tokio::test]
    async fn a_read_prompt_is_answered_without_the_assistant() {
        let daemon = Arc::new(test_daemon().await);
        let cancel = CancellationToken::new();
        let codex = Caller {
            hook: Some(Hook::Codex),
            ..caller()
        };
        let ask = async |text: &str, caller: &Caller| {
            let prompt = Tool::Turn(TurnEvent::Prompt {
                text: text.to_string(),
                spoken: None,
            });
            let answer = run(&daemon, prompt, caller, &cancel)
                .await
                .unwrap()
                .unwrap();
            serde_json::from_str::<serde_json::Value>(&answer).unwrap()
        };
        let answered =
            |said: &str| serde_json::json!({ "spoken": false, "answered": said, "failed": true });

        assert_eq!(
            ask("/read", &caller()).await,
            answered("There is no reply to read yet.")
        );
        for text in ["First.", "Second."] {
            let reply = Tool::Turn(TurnEvent::Reply {
                text: text.to_string(),
            });
            run(&daemon, reply, &caller(), &cancel).await;
        }
        assert_eq!(
            ask("/read 5", &caller()).await,
            answered("There are 2 replies to read: 0 to 1.")
        );
        assert_eq!(ask("/read x", &caller()).await, answered(READ_USAGE));
        // The test daemon has no model to read it with.
        let unread = ask("/read 1", &caller()).await;
        assert!(
            unread["answered"]
                .as_str()
                .is_some_and(|said| said.contains("no text to speech model")),
            "{unread}"
        );
        assert_eq!(unread["failed"], true);
        assert_eq!(
            daemon.assistants.sessions.spoken(&caller().who),
            Some(false)
        );

        assert_eq!(
            ask("$read 9", &codex).await,
            serde_json::json!({
                "decision": "block",
                "reason": "There are 2 replies to read: 0 to 1.",
            })
        );
        // A prompt that only starts like it goes to the assistant.
        assert_eq!(
            ask("/readme is out of date", &caller()).await,
            serde_json::json!({ "spoken": false, "note": null })
        );
    }

    /// A plugin learns whether `/read` did what was asked, or what kept it
    /// from it; Codex, which shows the reason either way, is not told.
    #[test]
    fn a_read_answer_says_whether_it_failed() {
        let answer = |hook, outcome| {
            serde_json::from_str::<serde_json::Value>(&read_answer(hook, outcome)).unwrap()
        };
        assert_eq!(
            answer(None, Ok("Stopped reading.".to_string())),
            serde_json::json!({ "spoken": false, "answered": "Stopped reading.", "failed": false })
        );
        assert_eq!(answer(None, Err(READ_USAGE.to_string()))["failed"], true);
        assert_eq!(
            answer(Some(Hook::Codex), Ok("Reading reply 2 aloud.".to_string())),
            serde_json::json!({ "decision": "block", "reason": "Reading reply 2 aloud." })
        );
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
