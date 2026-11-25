// SPDX-License-Identifier: GPL-3.0-only
//! `/settings/hotkey` — the keys the macOS shortcut listener listens for.
//!
//! Every call answers with the binding the daemon now has: the one set, or
//! [`DEFAULT_BINDING`] when none is.

use crate::daemon::client::internal::response::require_success;
use crate::daemon::client::internal::session::with_settings_token;
use super_voice_shared::daemon::http_client::HttpResult;
use super_voice_shared::daemon::http_client::transport;
use super_voice_shared::models::hotkey::DEFAULT_BINDING;
use super_voice_shared::models::protocol::DaemonResponse;

fn binding(resp: DaemonResponse) -> String {
    resp.hotkey.unwrap_or_else(|| DEFAULT_BINDING.to_string())
}

/// Read the binding (HTTP `GET /settings/hotkey`).
pub async fn get_hotkey() -> HttpResult<String> {
    with_settings_token(|socket, token| async move {
        let resp = require_success(
            transport::settings_get(socket, &token, "/settings/hotkey").await?,
            "get_hotkey",
        )?;
        Ok(binding(resp))
    })
    .await
}

/// Set the binding (HTTP `POST /settings/hotkey`). The daemon refuses one the
/// listener could not use, saying why.
pub async fn set_hotkey(hotkey: String) -> HttpResult<String> {
    let body = serde_json::json!({ "hotkey": hotkey });
    with_settings_token(move |socket, token| {
        let body = body.clone();
        async move {
            let resp = require_success(
                transport::settings_post(socket, &token, "/settings/hotkey", &body).await?,
                "set_hotkey",
            )?;
            Ok(binding(resp))
        }
    })
    .await
}

/// Go back to the default binding (HTTP `DELETE /settings/hotkey`).
pub async fn clear_hotkey() -> HttpResult<String> {
    with_settings_token(|socket, token| async move {
        let resp = require_success(
            transport::settings_delete(socket, &token, "/settings/hotkey").await?,
            "clear_hotkey",
        )?;
        Ok(binding(resp))
    })
    .await
}
