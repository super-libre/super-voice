// SPDX-License-Identifier: GPL-3.0-only
//! `/assistants` — the AI assistants Super Voice connects to: list, connect,
//! disconnect, whether each asks its questions aloud, and the keys made by
//! hand for other MCP clients. What they are and how Super Voice gets into
//! their configs is [`crate::assistants`]; the work is
//! `daemon::assistant_handlers`.
//!
//! The routes that make a key or show one are for Super Voice's own clients
//! alone: an assistant must not be able to make itself another key, and
//! neither must any other program the user once let in. They are
//! [`official_routes`], which the router puts behind
//! `require_official_client` as well as the `settings` scope.
//!
//! Contract: `docs/protocol/endpoints/v1/assistants.md`.

use crate::assistants::Error;
use crate::daemon::http::internal::helpers::dispatch::json_response;
use crate::daemon::http::state::AppState;
use crate::daemon::http::wire::{ErrorEnvelope, ReasonEnvelope};
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use super_voice_shared::models::assistants::{AssistantId, AssistantList, NewKey};
use super_voice_shared::models::protocol::{DaemonResponse, ErrorCode};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

/// The body of `PATCH /assistants/{id}`.
#[derive(Deserialize, ToSchema)]
pub(crate) struct AssistantChange {
    /// Whether Super Voice says aloud when the assistant needs the user.
    pub(crate) announce_when_needed: bool,
}

/// The body of `POST /assistants/keys`.
#[derive(Deserialize, ToSchema)]
pub(crate) struct KeyRequest {
    /// What the user calls the MCP client the key is for. Not blank, and not
    /// an assistant's id.
    #[schema(example = "My editor")]
    pub(crate) name: String,
}

/// `GET /assistants/{id}/key` — the key an assistant sends.
#[derive(Serialize, ToSchema)]
pub(crate) struct AssistantKeyValue {
    /// The key. The assistant sends it as `Authorization: Bearer <key>`.
    #[schema(example = "sva_0f3c9a1e")]
    pub(crate) key: String,
}

/// A coded refusal.
fn refusal(code: ErrorCode, message: &str) -> Response {
    json_response(&DaemonResponse::error_with_code(code, message)).into_response()
}

/// What failed, as its coded refusal.
fn error(e: Error) -> Response {
    match e {
        Error::NotFound => refusal(ErrorCode::AssistantNotFound, "assistant_not_found"),
        Error::UnknownKey => refusal(ErrorCode::NotFound, "unknown_key"),
        Error::NameRequired => refusal(ErrorCode::InvalidValue, "name_required"),
        Error::NameReserved => refusal(ErrorCode::InvalidValue, "name_reserved"),
        Error::Config(detail) => refusal(
            ErrorCode::Internal,
            &format!("could_not_write_assistant_config: {detail}"),
        ),
        Error::Plugin(detail) => refusal(
            ErrorCode::Internal,
            &format!("could_not_change_plugin: {detail}"),
        ),
        Error::Key(detail) => refusal(
            ErrorCode::Internal,
            &format!("could_not_save_key: {detail}"),
        ),
    }
}

/// The list, or the coded refusal.
fn answer(result: Result<AssistantList, Error>) -> Response {
    match result {
        Ok(list) => axum::Json(list).into_response(),
        Err(e) => error(e),
    }
}

/// The `404` for a path that names no assistant.
fn unknown_assistant() -> Response {
    refusal(ErrorCode::NotFound, "unknown_assistant")
}

#[utoipa::path(
    get,
    path = "/assistants",
    tag = "assistants",
    summary = "List the AI assistants",
    description = "\
The AI assistants Super Voice can connect to, `claude-code`, `codex`, `opencode` and \
`gemini`, and where each stands on this computer: `not_found` when the daemon finds \
neither its program nor its config, `found` when it is here and Super Voice is not in \
its config, and `connected` when Super Voice is in its config with a key of its own. \
`announce_when_needed` says whether Super Voice says aloud when the assistant needs the \
user. Only Claude Code announces, through its plugin. `sessions` are its open sessions on \
the MCP server, oldest first: one conversation each, with its folder when the assistant \
says, and whether speech it started is playing or it is recording.

Also the keys made by hand for other MCP clients, without the keys themselves, each with \
its sessions, and `address`, where an MCP client reaches Super Voice.

Read from the assistants' configs on every request, so an entry changed by hand shows \
here at once.",
    security(("session_token" = ["settings"])),
    responses(
        (status = 200, description = "Every assistant, every key made by hand, and the address.", body = AssistantList),
        (status = 401, description = "Token unknown, expired, or its binary changed.", body = ReasonEnvelope),
        (status = 403, description = "The token lacks the `settings` scope.", body = ErrorEnvelope),
        (status = 429, description = "Per-client rate limit hit; back off and retry.", body = ErrorEnvelope),
    ),
)]
pub(crate) async fn list_assistants(State(state): State<AppState>) -> Response {
    answer(Ok(state.daemon.list_assistants(&state.auth).await))
}

#[utoipa::path(
    post,
    path = "/assistants/{id}/connect",
    tag = "assistants",
    summary = "Connect an AI assistant",
    description = "\
Makes the assistant a key of its own, or uses the one it has, and writes Super Voice \
into its config. For Claude Code the daemon installs Super Voice's plugin, with \
`claude plugin install`, and the plugin gets its key from `voice assistant-header \
claude-code`. Codex gets its key the same way, from `voice assistant-header codex`. \
`OpenCode` and Gemini CLI have the key written into their configs. Connecting one that \
is connected already writes its entry again. Answers the whole list, as `GET \
/assistants` does.

The assistant reads its config when it starts, so one already running is connected \
from its next start.

An assistant the daemon finds no sign of is `409 assistant_not_found`. When its config \
cannot be changed, or the plugin cannot be installed, it is `500`, and a key made for \
this request is revoked again.

For Super Voice's own clients alone, since it makes a key: any other caller is `403` \
with reason `not_official_client`.",
    params(("id" = AssistantId, Path, description = "`claude-code`, `codex`, `opencode` or `gemini`.")),
    security(("session_token" = ["settings"])),
    responses(
        (status = 200, description = "Connected. Every assistant.", body = AssistantList),
        (status = 401, description = "Token unknown, expired, or its binary changed.", body = ReasonEnvelope),
        (status = 403, description = "The token lacks the `settings` scope (`scope_denied`), or the caller is not one of Super Voice's own clients (`auth_denied`, reason `not_official_client`).", body = ReasonEnvelope),
        (status = 404, description = "No assistant has that id (`unknown_assistant`).", body = ErrorEnvelope),
        (status = 409, description = "The daemon finds no sign of the assistant on this computer (`assistant_not_found`).", body = ErrorEnvelope),
        (status = 429, description = "Per-client rate limit hit; back off and retry.", body = ErrorEnvelope),
        (status = 500, description = "The config could not be changed, the plugin could not be installed, or the key could not be saved.", body = ErrorEnvelope),
    ),
)]
pub(crate) async fn connect_assistant(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let Ok(id) = id.parse::<AssistantId>() else {
        return unknown_assistant();
    };
    answer(state.daemon.connect_assistant(&state.auth, id).await)
}

#[utoipa::path(
    delete,
    path = "/assistants/{id}",
    tag = "assistants",
    summary = "Disconnect an AI assistant",
    description = "\
Takes Super Voice out of the assistant's config, revokes its key, and cancels its \
calls in flight. For Claude Code the daemon uninstalls Super Voice's plugin, with \
`claude plugin uninstall`. Disconnecting one that is not connected changes nothing. \
Answers the whole list, as `GET /assistants` does.

The key is revoked even when the config cannot be changed, which is `500`: the entry \
left behind does nothing without it.",
    params(("id" = AssistantId, Path, description = "`claude-code`, `codex`, `opencode` or `gemini`.")),
    security(("session_token" = ["settings"])),
    responses(
        (status = 200, description = "Disconnected, or not connected. Every assistant.", body = AssistantList),
        (status = 401, description = "Token unknown, expired, or its binary changed.", body = ReasonEnvelope),
        (status = 403, description = "The token lacks the `settings` scope.", body = ErrorEnvelope),
        (status = 404, description = "No assistant has that id (`unknown_assistant`).", body = ErrorEnvelope),
        (status = 429, description = "Per-client rate limit hit; back off and retry.", body = ErrorEnvelope),
        (status = 500, description = "The config could not be changed, or the plugin could not be uninstalled. The key is revoked.", body = ErrorEnvelope),
    ),
)]
pub(crate) async fn disconnect_assistant(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let Ok(id) = id.parse::<AssistantId>() else {
        return unknown_assistant();
    };
    answer(state.daemon.disconnect_assistant(&state.auth, id).await)
}

#[utoipa::path(
    patch,
    path = "/assistants/{id}",
    tag = "assistants",
    summary = "Change whether an assistant is announced",
    description = "\
Sets whether Super Voice says aloud when the assistant needs the user, like Claude \
Code's own notices that it needs a permission or an answer. On until the user turns it \
off. The next announcement reads it, in every session. Answers the whole list, as \
`GET /assistants` does.",
    params(("id" = AssistantId, Path, description = "`claude-code`, `codex`, `opencode` or `gemini`.")),
    request_body = AssistantChange,
    security(("session_token" = ["settings"])),
    responses(
        (status = 200, description = "Changed. Every assistant.", body = AssistantList),
        (status = 400, description = "The body is not `{\"announce_when_needed\": <bool>}` (`invalid_value`).", body = ErrorEnvelope),
        (status = 401, description = "Token unknown, expired, or its binary changed.", body = ReasonEnvelope),
        (status = 403, description = "The token lacks the `settings` scope.", body = ErrorEnvelope),
        (status = 404, description = "No assistant has that id (`unknown_assistant`).", body = ErrorEnvelope),
        (status = 429, description = "Per-client rate limit hit; back off and retry.", body = ErrorEnvelope),
    ),
)]
pub(crate) async fn change_assistant(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<AssistantChange>,
) -> Response {
    let Ok(id) = id.parse::<AssistantId>() else {
        return unknown_assistant();
    };
    answer(Ok(state
        .daemon
        .set_assistant_announces(&state.auth, id, body.announce_when_needed)
        .await))
}

#[utoipa::path(
    post,
    path = "/assistants/keys",
    tag = "assistants",
    summary = "Make a key for another MCP client",
    description = "\
Makes a key, under the name given, for an MCP client that is not one of the assistants \
the daemon connects itself. The key may speak and hear the user, as an assistant's \
does. The answer carries the key itself, and this is the only time it is shown: the \
list names the key by its `id` and `name` alone.

The name is trimmed. A blank one is `400 name_required`, and an assistant's id is \
`400 name_reserved`, since the daemon names an assistant's key itself.

For Super Voice's own clients alone, since it makes a key: any other caller is `403` \
with reason `not_official_client`.",
    request_body = KeyRequest,
    security(("session_token" = ["settings"])),
    responses(
        (status = 200, description = "The key, with the address.", body = NewKey),
        (status = 400, description = "The name is blank (`name_required`) or an assistant's id (`name_reserved`), or the body is not `{\"name\": <string>}` (`invalid_value`).", body = ErrorEnvelope),
        (status = 401, description = "Token unknown, expired, or its binary changed.", body = ReasonEnvelope),
        (status = 403, description = "The token lacks the `settings` scope (`scope_denied`), or the caller is not one of Super Voice's own clients (`auth_denied`, reason `not_official_client`).", body = ReasonEnvelope),
        (status = 429, description = "Per-client rate limit hit; back off and retry.", body = ErrorEnvelope),
        (status = 500, description = "The key could not be saved.", body = ErrorEnvelope),
    ),
)]
pub(crate) async fn create_key(
    State(state): State<AppState>,
    axum::Json(body): axum::Json<KeyRequest>,
) -> Response {
    match state
        .daemon
        .create_assistant_key(&state.auth, &body.name)
        .await
    {
        Ok(key) => axum::Json(key).into_response(),
        Err(e) => error(e),
    }
}

#[utoipa::path(
    delete,
    path = "/assistants/keys/{id}",
    tag = "assistants",
    summary = "Revoke a key made by hand",
    description = "\
Revokes a key made with `POST /assistants/keys`, and cancels its calls in flight. It \
stops working at once. An assistant's own key is not one of these: disconnecting the \
assistant revokes it. Answers the whole list, as `GET /assistants` does.",
    params(("id" = String, Path, description = "The key's `id`, from the list.", example = "0f3c9a1e")),
    security(("session_token" = ["settings"])),
    responses(
        (status = 200, description = "Revoked. Every assistant, and the keys left.", body = AssistantList),
        (status = 401, description = "Token unknown, expired, or its binary changed.", body = ReasonEnvelope),
        (status = 403, description = "The token lacks the `settings` scope.", body = ErrorEnvelope),
        (status = 404, description = "No key made by hand has that id (`unknown_key`).", body = ErrorEnvelope),
        (status = 429, description = "Per-client rate limit hit; back off and retry.", body = ErrorEnvelope),
    ),
)]
pub(crate) async fn revoke_key(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    answer(state.daemon.revoke_custom_key(&state.auth, &id).await)
}

#[utoipa::path(
    get,
    path = "/assistants/{id}/key",
    tag = "assistants",
    summary = "Show an assistant's key",
    description = "\
The key the daemon made for the assistant when it was connected. `voice \
assistant-header <id>` asks for it, for an assistant that gets its key by running that \
command, as Claude Code's plugin and Codex do. An assistant that is not connected has \
none: `404 not_connected`.

For Super Voice's own clients alone, since it shows a key: any other caller is `403` \
with reason `not_official_client`.",
    params(("id" = AssistantId, Path, description = "`claude-code`, `codex`, `opencode` or `gemini`.")),
    security(("session_token" = ["settings"])),
    responses(
        (status = 200, description = "The key.", body = AssistantKeyValue),
        (status = 401, description = "Token unknown, expired, or its binary changed.", body = ReasonEnvelope),
        (status = 403, description = "The token lacks the `settings` scope (`scope_denied`), or the caller is not one of Super Voice's own clients (`auth_denied`, reason `not_official_client`).", body = ReasonEnvelope),
        (status = 404, description = "No assistant has that id (`unknown_assistant`), or it is not connected (`not_connected`).", body = ErrorEnvelope),
        (status = 429, description = "Per-client rate limit hit; back off and retry.", body = ErrorEnvelope),
    ),
)]
pub(crate) async fn assistant_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let Ok(id) = id.parse::<AssistantId>() else {
        return unknown_assistant();
    };
    match state.daemon.assistant_key(&state.auth, id) {
        Some(key) => axum::Json(AssistantKeyValue { key }).into_response(),
        None => refusal(ErrorCode::NotFound, "not_connected"),
    }
}

/// The routes any `settings` token reaches.
pub(crate) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_assistants))
        .routes(routes!(disconnect_assistant, change_assistant))
        .routes(routes!(revoke_key))
}

/// The routes that make a key or show one, for Super Voice's own clients
/// alone.
pub(crate) fn official_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(connect_assistant))
        .routes(routes!(create_key))
        .routes(routes!(assistant_key))
}
