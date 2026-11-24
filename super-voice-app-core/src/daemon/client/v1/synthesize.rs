// SPDX-License-Identifier: GPL-3.0-only
//! `/tts/synthesize` — text spoken into a WAV file, for the Clips tab to keep.
//!
//! The daemon answers with the file once the whole text is synthesized, and
//! plays nothing. Dropping the future closes the connection, which stops the
//! synthesis: that is how the tab's Cancel works.

use crate::daemon::client::internal::session::with_settings_token;
use super_voice_shared::daemon::http_client::{HttpResult, transport};

/// `POST /tts/synthesize` — `text`, in the voice and language the user set, as
/// a mono 16-bit WAV file.
pub async fn synthesize(text: String) -> HttpResult<Vec<u8>> {
    let body = serde_json::json!({ "text": text });
    with_settings_token(move |socket, token| {
        let body = body.clone();
        async move { transport::post_json_for_bytes(socket, &token, "/tts/synthesize", &body).await }
    })
    .await
}
