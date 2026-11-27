// SPDX-License-Identifier: GPL-3.0-only
//! MCP's Streamable HTTP transport: every message is a `POST` of its own.
//! The answer to `initialize` carries an `Mcp-Session-Id`, which only tells
//! a client's sessions apart and matches a cancel to its call: a request
//! with none, or one from before a restart, is served all the same.
//!
//! 1. A request that answers at once (`initialize`, `ping`, `tools/list`) is
//!    answered with `application/json`.
//! 2. A notification, or a response, is `202` with no body.
//! 3. `tools/call` is answered with `text/event-stream`: a comment at once, a
//!    keepalive every two seconds while the tool runs, then one `message`
//!    event with the answer. A `listen` can wait minutes for the user, and the
//!    keepalives keep the connection alive meanwhile. A request that names a
//!    `progressToken` gets each keepalive as a `notifications/progress`, which
//!    a client counts as the tool's progress: Claude Code stops a call that
//!    sends none for five minutes. Any other request gets a comment.
//!
//! The body is read as bytes and every JSON-RPC error is built here, so a
//! body that is not JSON is answered as MCP says, not as the rest of the
//! daemon answers one.

use super::identity::identity;
use super::protocol::{self, Access, Handled, Plugin, Tool, TurnEvent};
use super::tools;
use crate::assistants::calls::Caller;
use crate::assistants::sessions::Who;
use crate::daemon::assistant_handlers::key_assistant;
use crate::daemon::http::state::AppState;
use crate::daemon::types::SuperSTTDaemon;
use axum::Extension;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use log::{info, warn};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use super_engine_daemon::auth::{AuthContext, PeerIdentity};
use super_voice_shared::models::assistants::AssistantId;

/// The header a client names the protocol version in, once it has agreed
/// one.
const PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";

/// The header the daemon names a connection in, at `initialize`, and a
/// client on a revision before 2026-07-28 sends back on every request. It
/// tells apart the sessions of a client that says nothing else of itself.
const SESSION_HEADER: &str = "mcp-session-id";

/// How long a tool call's stream may go without a byte. Short, as
/// `/v1/stt/transcribe`'s is: a client may drop a connection that has been
/// silent for a few seconds.
const KEEPALIVE: Duration = Duration::from_secs(2);

/// The token a request's `_meta` asks its progress to be reported under, if
/// it names one: a string or an integer, as MCP has it.
fn progress_token(params: &Value) -> Option<Value> {
    let token = &params["_meta"]["progressToken"];
    (token.is_string() || token.is_i64() || token.is_u64()).then(|| token.clone())
}

/// The keepalive a tool call's stream sends the `count`th time, from one:
/// under `token`, the progress it reports, else a comment.
fn keepalive(token: Option<&Value>, count: u64) -> Bytes {
    match token {
        Some(token) => super_engine_daemon::events::sse_frame(
            "message",
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": { "progressToken": token, "progress": count },
            })
            .to_string(),
        ),
        None => Bytes::from_static(b": keepalive\n\n"),
    }
}

/// The connection a request names with its `Mcp-Session-Id`, if it names one.
fn connection_of(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(SESSION_HEADER)
        .and_then(|value| value.to_str().ok())
}

/// What a call by `caller` needs to know: its session, named as the call
/// and its connection say, and marked active; the switches the user set for
/// `assistant`; and the hook that made the call, if one did.
async fn call_context(
    state: &AppState,
    caller: &Caller,
    assistant: Option<AssistantId>,
    params: &Value,
    headers: &HeaderMap,
) -> tools::CallContext {
    let sessions = &state.daemon.assistants.sessions;
    let connection = connection_of(headers);
    let named = sessions.resolve(caller, identity(params), connection);
    let who = Who {
        caller: caller.clone(),
        named,
        connection: connection.map(str::to_string),
    };
    sessions.touch(&who);
    // A key made by hand, and the user's own client, have no switches.
    let (announces, uses_context, shows_talk, autosends) = match assistant {
        Some(id) => {
            let config = state.daemon.config.read().await;
            (
                config.assistants.announces_when_needed(id),
                config.assistants.uses_conversation_context(id),
                config.assistants.shows_talk(id),
                config.assistants.autosends_dictated_prompts(id),
            )
        }
        None => (true, true, true, true),
    };
    tools::CallContext {
        who,
        announces,
        uses_context,
        shows_talk,
        autosends,
        hook: protocol::hook_of(params),
    }
}

/// `POST /v1/mcp`: one JSON-RPC message.
pub(crate) async fn post(
    State(state): State<AppState>,
    Extension(session): Extension<AuthContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(version) = headers.get(PROTOCOL_VERSION_HEADER) {
        let version = String::from_utf8_lossy(version.as_bytes());
        if !protocol::PROTOCOL_VERSIONS.contains(&version.as_ref()) {
            return json(
                StatusCode::BAD_REQUEST,
                &protocol::error(
                    &Value::Null,
                    protocol::INVALID_REQUEST,
                    &format!(
                        "Unsupported MCP-Protocol-Version {version}. This server speaks {}.",
                        protocol::PROTOCOL_VERSIONS.join(", ")
                    ),
                ),
            );
        }
    }
    let message = match protocol::parse(&body) {
        Ok(message) => message,
        Err(refusal) => return json(StatusCode::BAD_REQUEST, &refusal),
    };
    let caller = caller_of(&session);
    let assistant = match &caller {
        Caller::Key(key) => key_assistant(&state.auth, key),
        Caller::Session(_) => None,
    };
    let access = Access {
        may_speak: session.meta.scopes.iter().any(|s| s == "speak"),
        may_listen: session.meta.scopes.iter().any(|s| s == "transcribe"),
        plugin: match assistant {
            Some(AssistantId::Codex) => Plugin::ReadsTheMark,
            Some(AssistantId::ClaudeCode | AssistantId::OpenCode | AssistantId::Antigravity) => {
                Plugin::HidesItsTools
            }
            None => Plugin::None,
        },
    };
    match protocol::handle(&message, access) {
        Handled::Reply(reply)
            if message["method"] == "initialize" && reply.get("result").is_some() =>
        {
            let mut response = json(StatusCode::OK, &reply);
            if let Ok(id) =
                header::HeaderValue::from_str(&uuid::Uuid::new_v4().simple().to_string())
            {
                response.headers_mut().insert(SESSION_HEADER, id);
            }
            response
        }
        Handled::Reply(reply) => json(StatusCode::OK, &reply),
        Handled::Refuse(refusal) => json(StatusCode::BAD_REQUEST, &refusal),
        Handled::Accepted => StatusCode::ACCEPTED.into_response(),
        Handled::Cancelled(id) => {
            let connection = connection_of(&headers);
            let cancelled = state
                .daemon
                .assistants
                .calls
                .cancel(&caller, connection, &id);
            if cancelled > 0 {
                info!("MCP call {id} from {} cancelled", describe(&caller));
            }
            StatusCode::ACCEPTED.into_response()
        }
        Handled::Call { id, tool } => {
            let call = call_context(&state, &caller, assistant, &message["params"], &headers).await;
            if matches!(tool, Tool::Listen { .. }) {
                info!(
                    "{}",
                    listen_log(
                        &assistant_of(&state, &caller, assistant),
                        &session_of(&state, &call),
                        &message["params"]["arguments"],
                        call.uses_context,
                    )
                );
            }
            let progress = progress_token(&message["params"]);
            stream_call(Arc::clone(&state.daemon), caller, call, id, tool, progress)
        }
    }
}

/// Who `session` is, for the calls in flight: an assistant by its key, any
/// other caller by its token.
fn caller_of(session: &AuthContext) -> Caller {
    match &session.meta.grantee {
        PeerIdentity::Assistant { key_id } => Caller::Key(key_id.clone()),
        _ => Caller::Session(session.token.clone()),
    }
}

/// `caller`, for the log: never a session's token.
fn describe(caller: &Caller) -> String {
    match caller {
        Caller::Key(key) => format!("key {key}"),
        Caller::Session(_) => "a session".to_string(),
    }
}

/// Who made a call, for the log: the assistant, a key made by hand by the
/// name the user gave it, or a client with a session. Never a key or a token.
fn assistant_of(state: &AppState, caller: &Caller, assistant: Option<AssistantId>) -> String {
    match (caller, assistant) {
        (_, Some(id)) => id.display_name().to_string(),
        (Caller::Key(key), None) => state
            .auth
            .assistant_keys()
            .into_iter()
            .find(|made| made.id == *key)
            .map_or_else(
                || describe(caller),
                |made| format!("the key {:?}", made.name),
            ),
        (Caller::Session(_), None) => "a client with a session token".to_string(),
    }
}

/// The session a call is for, for the log: what names it, as the
/// Assistants page shows it, its id, and the agent within it that made the
/// call.
fn session_of(state: &AppState, call: &tools::CallContext) -> String {
    let named = &call.who.named;
    let session = match (
        state.daemon.assistants.sessions.label(&call.who),
        named.id.as_deref(),
    ) {
        (Some(label), Some(id)) => format!("session {label:?} ({id})"),
        (Some(label), None) => format!("session {label:?}"),
        (None, Some(id)) => format!("session {id}"),
        (None, None) => "its one session".to_string(),
    };
    match &named.agent {
        Some(agent) => format!("{session}, agent {agent}"),
        None => session,
    }
}

/// What the log says of a `listen` call as it arrives, before anything
/// records: who it is from, `session`, and the context its client sent in
/// `args`, whole, as it came, before the daemon cuts it to fit. The user
/// reads it to see what an assistant sends. One field a line, each as JSON,
/// so a line break in one stays on its line.
fn listen_log(from: &str, session: &str, args: &Value, uses_context: bool) -> String {
    let field = |value: &Value| {
        if value.is_null() {
            "none".to_string()
        } else {
            value.to_string()
        }
    };
    let by = if args["asked"].as_bool() == Some(true) {
        "the user"
    } else {
        "the model"
    };
    let unused = if uses_context {
        ""
    } else {
        ", heard without its context: the user turned the conversation's context off"
    };
    let context = &args["context"];
    format!(
        "MCP listen from {from}, {session}, asked by {by}{unused}\n  \
         question: {}\n  prompt: {}\n  vocabulary: {}",
        field(&args["question"]),
        field(&context["prompt"]),
        field(&context["vocabulary"]),
    )
}

fn json(status: StatusCode, message: &Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        message.to_string(),
    )
        .into_response()
}

/// Run `tool` for request `id`, and stream its answer, with its progress
/// under `progress` when the request named a token.
///
/// The tool runs on a task of its own, so it is never dropped midway. The
/// stream is fed from another, which cancels the call when a keepalive finds
/// the client gone.
fn stream_call(
    daemon: Arc<SuperSTTDaemon>,
    caller: Caller,
    call: tools::CallContext,
    id: Value,
    tool: Tool,
    progress: Option<Value>,
) -> Response {
    let name = tool.name();
    // The plugin's checks come every second or so, and a draft's at each
    // pause in typing: not worth a line each. A `listen` has its own line,
    // with its context, as it arrives.
    if !matches!(
        tool,
        Tool::Status | Tool::Listen { .. } | Tool::Turn(TurnEvent::Draft { .. })
    ) {
        info!("MCP {name} from {}", describe(&caller));
    }
    let in_flight = daemon
        .assistants
        .calls
        .begin(caller, call.who.connection.clone(), &id);
    let cancel = in_flight.token();

    let (frames, body) = tokio::sync::mpsc::unbounded_channel::<Result<Bytes, std::io::Error>>();
    // At once, so the client sees the response begin before the first
    // keepalive is due.
    let _ = frames.send(Ok(Bytes::from_static(b": stream-open\n\n")));

    let call = tokio::spawn({
        let cancel = cancel.clone();
        async move {
            let outcome = tools::run(&daemon, tool, &call, &cancel).await;
            // Forgotten once it ends, however it ends.
            drop(in_flight);
            outcome
        }
    });
    tokio::spawn(async move {
        let mut call = call;
        let mut keepalives = 0;
        loop {
            tokio::select! {
                biased;
                outcome = &mut call => {
                    let outcome = match outcome {
                        Ok(Some(outcome)) => outcome,
                        // Cancelled: a cancelled request gets no answer.
                        Ok(None) => break,
                        Err(e) => {
                            warn!("MCP {name} failed: {e}");
                            Err(format!("Super Voice could not {name}: {e}"))
                        }
                    };
                    let answer = protocol::result(&id, protocol::tool_result(outcome));
                    let _ = frames.send(Ok(super_engine_daemon::events::sse_frame(
                        "message",
                        &answer.to_string(),
                    )));
                    break;
                }
                () = tokio::time::sleep(KEEPALIVE) => {
                    keepalives += 1;
                    if frames.send(Ok(keepalive(progress.as_ref(), keepalives))).is_err() {
                        info!("MCP {name}: the client went away; cancelling");
                        cancel.cancel();
                        break;
                    }
                }
            }
        }
    });

    let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(body);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-store")
        // No proxy holds the events back.
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

#[cfg(test)]
mod tests {
    use super::{keepalive, listen_log, progress_token};
    use serde_json::{Value, json};

    /// A `listen`'s log line has what its client sent, each field on a line
    /// of its own, whole.
    #[test]
    fn a_listens_log_says_what_its_client_sent() {
        let args = json!({
            "question": "Which one?",
            "context": {
                "prompt": "Topic: the release.\nFiles: CHANGELOG.md",
                "vocabulary": ["Super Voice", "GPUI"],
            },
        });
        assert_eq!(
            listen_log("Claude Code", "session \"super-voice\" (s1)", &args, true),
            "MCP listen from Claude Code, session \"super-voice\" (s1), asked by the model\n  \
             question: \"Which one?\"\n  \
             prompt: \"Topic: the release.\\nFiles: CHANGELOG.md\"\n  \
             vocabulary: [\"Super Voice\",\"GPUI\"]"
        );

        let talk = json!({ "asked": true });
        assert_eq!(
            listen_log("Codex", "its one session", &talk, false),
            "MCP listen from Codex, its one session, asked by the user, heard without its \
             context: the user turned the conversation's context off\n  \
             question: none\n  prompt: none\n  vocabulary: none"
        );
    }

    #[test]
    fn a_request_names_its_progress_token_in_meta() {
        let named = |token: Value| json!({ "name": "listen", "_meta": { "progressToken": token } });
        assert_eq!(progress_token(&named(json!(7))), Some(json!(7)));
        assert_eq!(progress_token(&named(json!("t-1"))), Some(json!("t-1")));
        assert_eq!(progress_token(&named(json!(1.5))), None, "not an integer");
        assert_eq!(progress_token(&named(json!({}))), None);
        assert_eq!(progress_token(&json!({ "name": "listen" })), None);
    }

    #[test]
    fn a_keepalive_reports_progress_under_the_token_else_is_a_comment() {
        assert_eq!(&keepalive(None, 1)[..], b": keepalive\n\n");
        let sent = String::from_utf8(keepalive(Some(&json!(7)), 3).to_vec()).expect("text");
        let data = sent
            .strip_prefix("event: message\ndata: ")
            .and_then(|rest| rest.strip_suffix("\n\n"))
            .unwrap_or_else(|| panic!("not one message event: {sent}"));
        assert_eq!(
            serde_json::from_str::<Value>(data).expect("JSON"),
            json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": { "progressToken": 7, "progress": 3 },
            })
        );
    }
}
