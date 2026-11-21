// SPDX-License-Identifier: GPL-3.0-only
//! MCP's Streamable HTTP transport, without sessions: every message is a
//! `POST` of its own, and nothing is kept between them but the tool calls
//! in flight.
//!
//! 1. A request that answers at once (`initialize`, `ping`, `tools/list`) is
//!    answered with `application/json`.
//! 2. A notification, or a response, is `202` with no body.
//! 3. `tools/call` is answered with `text/event-stream`: a comment at once, a
//!    keepalive comment every two seconds while the tool runs, then one
//!    `message` event with the answer. A `listen` can wait minutes for the
//!    user, and the keepalives keep the connection, and any timer the client
//!    has on it, alive meanwhile.
//!
//! The body is read as bytes and every JSON-RPC error is built here, so a
//! body that is not JSON is answered as MCP says, not as the rest of the
//! daemon answers one.

use super::identity::identity;
use super::protocol::{self, Access, Handled, Plugin, Tool};
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
            Some(AssistantId::ClaudeCode) => Plugin::HidesItsTools,
            Some(AssistantId::Codex) => Plugin::ReadsTheMark,
            _ => Plugin::None,
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
            let cancelled = state.daemon.assistants.calls.cancel(&caller, &id);
            if cancelled > 0 {
                info!("MCP call {id} from {} cancelled", describe(&caller));
            }
            StatusCode::ACCEPTED.into_response()
        }
        Handled::Call { id, tool } => {
            let connection = headers
                .get(SESSION_HEADER)
                .and_then(|value| value.to_str().ok());
            let named = state.daemon.assistants.sessions.resolve(
                &caller,
                identity(&message["params"]),
                connection,
            );
            let who = Who {
                caller: caller.clone(),
                named,
                connection: connection.map(str::to_string),
            };
            state.daemon.assistants.sessions.touch(&who);
            // A key made by hand, and the user's own client, have no switches.
            let (announces, uses_context) = match assistant {
                Some(id) => {
                    let config = state.daemon.config.read().await;
                    (
                        config.assistants.announces_when_needed(id),
                        config.assistants.uses_conversation_context(id),
                    )
                }
                None => (true, true),
            };
            let for_tool = tools::Caller {
                who,
                announces,
                uses_context,
            };
            stream_call(Arc::clone(&state.daemon), caller, for_tool, id, tool)
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

fn json(status: StatusCode, message: &Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        message.to_string(),
    )
        .into_response()
}

/// Run `tool` for request `id`, and stream its answer.
///
/// The tool runs on a task of its own, so it is never dropped midway. The
/// stream is fed from another, which cancels the call when a keepalive finds
/// the client gone.
fn stream_call(
    daemon: Arc<SuperSTTDaemon>,
    caller: Caller,
    for_tool: tools::Caller,
    id: Value,
    tool: Tool,
) -> Response {
    let name = tool.name();
    // The plugin's checks come every second or so: not worth a line each.
    if !matches!(tool, Tool::Status) {
        info!("MCP {name} from {}", describe(&caller));
    }
    let in_flight = daemon.assistants.calls.begin(caller, &id);
    let cancel = in_flight.token();

    let (frames, body) = tokio::sync::mpsc::unbounded_channel::<Result<Bytes, std::io::Error>>();
    // At once, so the client sees the response begin before the first
    // keepalive is due.
    let _ = frames.send(Ok(Bytes::from_static(b": stream-open\n\n")));

    let call = tokio::spawn({
        let cancel = cancel.clone();
        async move {
            let outcome = tools::run(&daemon, tool, &for_tool, &cancel).await;
            // Forgotten once it ends, however it ends.
            drop(in_flight);
            outcome
        }
    });
    tokio::spawn(async move {
        let mut call = call;
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
                    if frames.send(Ok(Bytes::from_static(b": keepalive\n\n"))).is_err() {
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
