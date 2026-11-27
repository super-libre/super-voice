// SPDX-License-Identifier: GPL-3.0-only
//! `/status` — each feature's model and device, and whether the daemon may
//! type into other apps.
//!
//! The app reads per-stage detail from [`super::pipeline`]; it reads this for
//! what only the status says: `stt.can_type`.

use crate::daemon::client::internal::session::with_settings_token;
use crate::daemon::client::internal::transport;
use super_voice_shared::daemon::http_client::HttpResult;
use super_voice_shared::models::status::VoiceStatus;

/// The daemon's status (HTTP `GET /status`).
pub async fn get_status() -> HttpResult<VoiceStatus> {
    with_settings_token(|socket, token| async move { transport::status(socket, &token).await })
        .await
}

/// Whether the daemon may type into other apps: `None` when it can't tell
/// (Linux), and from a daemon older than the field.
pub async fn get_can_type() -> HttpResult<Option<bool>> {
    get_status().await.map(|status| status.stt.can_type)
}
