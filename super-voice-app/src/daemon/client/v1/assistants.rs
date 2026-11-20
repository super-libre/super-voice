// SPDX-License-Identifier: GPL-3.0-only
//! `/assistants` — the AI assistants Super Voice connects to, and the keys
//! made by hand for other MCP clients. The daemon writes each assistant's
//! config and holds the keys, so the app never touches either itself.

use crate::daemon::client::internal::session::with_settings_token;
use super_voice_shared::daemon::http_client::{HttpResult, transport};
use super_voice_shared::models::assistants::{AssistantId, AssistantList, NewKey};

/// `GET /assistants` — every assistant, every key made by hand, and the
/// address an MCP client reaches Super Voice at.
pub async fn list_assistants() -> HttpResult<AssistantList> {
    with_settings_token(|socket, token| async move {
        transport::get_json(socket, &token, "/assistants").await
    })
    .await
}

/// `POST /assistants/{id}/connect` — make the assistant a key and write Super
/// Voice into its config. Answers the list.
///
/// Without the usual timeout: for Claude Code the daemon adds Super Voice's
/// marketplace and installs its plugin, and each may take up to two minutes.
pub async fn connect_assistant(id: AssistantId) -> HttpResult<AssistantList> {
    let path = format!("/assistants/{id}/connect");
    with_settings_token(move |socket, token| {
        let path = path.clone();
        async move {
            transport::post_json_no_timeout(socket, &token, &path, &serde_json::json!({})).await
        }
    })
    .await
}

/// `DELETE /assistants/{id}` — take Super Voice out of the assistant's config
/// and revoke its key. Answers the list.
pub async fn disconnect_assistant(id: AssistantId) -> HttpResult<AssistantList> {
    let path = format!("/assistants/{id}");
    with_settings_token(move |socket, token| {
        let path = path.clone();
        async move { transport::delete_json(socket, &token, &path).await }
    })
    .await
}

/// `PATCH /assistants/{id}` — whether Super Voice says aloud when the
/// assistant needs the user. Answers the list.
pub async fn set_announce_when_needed(id: AssistantId, on: bool) -> HttpResult<AssistantList> {
    let path = format!("/assistants/{id}");
    let body = serde_json::json!({ "announce_when_needed": on });
    with_settings_token(move |socket, token| {
        let path = path.clone();
        let body = body.clone();
        async move { transport::patch_json(socket, &token, &path, &body).await }
    })
    .await
}

/// `PATCH /assistants/{id}` — whether the assistant's `listen` calls are
/// heard with the context they bring from the conversation. Answers the list.
pub async fn set_conversation_context(id: AssistantId, on: bool) -> HttpResult<AssistantList> {
    let path = format!("/assistants/{id}");
    let body = serde_json::json!({ "conversation_context": on });
    with_settings_token(move |socket, token| {
        let path = path.clone();
        let body = body.clone();
        async move { transport::patch_json(socket, &token, &path, &body).await }
    })
    .await
}

/// `POST /assistants/keys` — make a key by hand, for another MCP client. The
/// answer holds the key, which is shown only this once.
pub async fn create_key(name: String) -> HttpResult<NewKey> {
    let body = serde_json::json!({ "name": name });
    with_settings_token(move |socket, token| {
        let body = body.clone();
        async move { transport::post_json(socket, &token, "/assistants/keys", &body).await }
    })
    .await
}

/// `DELETE /assistants/keys/{id}` — revoke a key made by hand. Answers the
/// list.
pub async fn remove_key(id: String) -> HttpResult<AssistantList> {
    let path = format!("/assistants/keys/{id}");
    with_settings_token(move |socket, token| {
        let path = path.clone();
        async move { transport::delete_json(socket, &token, &path).await }
    })
    .await
}
