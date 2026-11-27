// SPDX-License-Identifier: GPL-3.0-only
//! `/ping` — daemon connectivity checks.
//!
//! The daemon's `/status` is [`super::status`]: the app reads it for whether
//! the daemon may type, and per-stage detail from [`super::pipeline`].

use crate::daemon::client::internal::session::with_settings_token;
use crate::daemon::client::internal::transport;
use super_voice_shared::daemon::http_client::HttpResult;

/// Test daemon connection (HTTP `/ping`).
pub async fn test_daemon_connection() -> HttpResult<()> {
    with_settings_token(
        |socket, token| async move { transport::ping(socket, &token).await.map(|_| ()) },
    )
    .await
}

/// Ping daemon to check connectivity (HTTP `/ping`).
pub async fn ping_daemon() -> HttpResult<String> {
    with_settings_token(|socket, token| async move { transport::ping(socket, &token).await }).await
}
