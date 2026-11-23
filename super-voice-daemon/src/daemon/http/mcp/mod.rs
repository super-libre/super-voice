// SPDX-License-Identifier: GPL-3.0-only
//! `/v1/mcp` — Super Voice's MCP server, so an AI assistant can speak to the
//! user and hear them: the tools `speak` and `listen`, over MCP's Streamable
//! HTTP transport.
//!
//! An assistant reaches it on the TCP listener with the key the daemon made
//! it when the user connected it ([`crate::assistants`]). It is a program,
//! not a page, so it sends no `Origin` and cannot pass the origin gate the
//! rest of `/v1` is behind. This router answers before that gate, and is
//! behind the engine's `require_assistant_caller` instead, which admits a
//! TCP caller by its key alone: no `Origin`, a loopback `Host`, and an
//! assistant key as the bearer. A caller on the Unix socket passes it with a
//! session token, as a test or the CLI does.
//!
//! The scope each tool needs is checked against the caller's: `speak` takes
//! `speak`, `listen` takes `transcribe`, and `speak` too for a question.
//! A connected assistant also gets its plugin's tools, `stop`, `status`,
//! `announce`, `session` and `turn`, kept from its model: marked for the app
//! for Codex, hidden by the plugin for the others.
//!
//! Each call names the session it comes from (`identity`), and the speech
//! and the recording a session starts are its own
//! ([`crate::assistants::sessions`]).
//!
//! MCP is JSON-RPC, a method in the body rather than a path, so `/v1/mcp` is
//! not in the `OpenAPI` document. `SURFACE` lists what it serves, for the URL
//! surface contract.
//!
//! Contract: `docs/protocol/mcp.md`.

mod handler;
mod identity;
pub(crate) mod protocol;
mod tools;

use crate::assistants::MCP_PATH;
use crate::daemon::http::state::AppState;
use axum::Router;
use axum::middleware;
use axum::routing::post;
use super_engine_daemon::auth::middleware::{
    require_any_authenticated, require_assistant_caller, require_rate_limit,
};

/// Every path this router serves, with the methods it answers on. Any other
/// method on it is `405`. What the URL surface contract checks the hand-kept
/// list of paths outside the `OpenAPI` document against.
#[cfg(test)]
pub(crate) const SURFACE: &[(&str, &str)] = &[(MCP_PATH, "post")];

/// The live `/v1/mcp` router, its guards applied and its state bound.
///
/// The assistant gate is outermost, since it is what gives an assistant's
/// request its identity; then the rate limit, which counts each assistant by
/// its key; then the token guard. A token of any scope gets in, and each tool
/// checks the scope it needs.
pub(crate) fn router(state: AppState) -> Router {
    let auth = state.auth.clone();
    Router::new()
        .route(MCP_PATH, post(handler::post))
        .with_state(state)
        .layer(middleware::from_fn_with_state(
            auth.clone(),
            require_any_authenticated,
        ))
        .layer(middleware::from_fn_with_state(
            auth.clone(),
            require_rate_limit,
        ))
        .layer(middleware::from_fn_with_state(
            auth,
            require_assistant_caller,
        ))
}
