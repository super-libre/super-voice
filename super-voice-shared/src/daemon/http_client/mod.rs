// SPDX-License-Identifier: GPL-3.0-only
//! HTTP client for the daemon protocol: `super_engine_client::http_client`,
//! plus speech to text's own endpoints.
//!
//! The transport is HTTP/1.1 over the daemon's Unix socket
//! (`super_voice_shared::validation::get_http_socket_path()`). Authentication is
//! per-request: callers pass a session token (obtained from
//! [`crate::daemon::session::obtain`]) and it is attached as
//! `Authorization: Bearer <token>` on every call except [`auth_request`].

mod transcribe;

use std::path::PathBuf;

pub use super_engine_client::http_client::{
    AuthOk, AuthStatusInfo, HttpError, HttpResult, WidgetEvent, auth_request, auth_status,
    events_stream, ping,
};
pub use transcribe::{
    TranscribeEvent, TranscribeOptions, transcribe, transcribe_stop, transcribe_stream,
};

use crate::models::status::VoiceStatus;

/// `GET /status` — each feature's model, device and whether it is at work.
///
/// # Errors
/// Returns an error if the daemon HTTP listener isn't reachable or the
/// response can't be parsed.
pub async fn status(socket_path: PathBuf, token: &str) -> HttpResult<VoiceStatus> {
    super_engine_client::http_client::status(socket_path, token).await
}

/// `GET /settings/hotkey` — the keys the macOS shortcut listener listens
/// for. Needs the `settings` scope.
///
/// # Errors
/// Returns an error if the daemon HTTP listener isn't reachable, refuses the
/// token, or answers without a binding.
pub async fn hotkey(socket_path: PathBuf, token: &str) -> HttpResult<String> {
    let resp = transport::settings_get(socket_path, token, "/settings/hotkey").await?;
    resp.hotkey
        .ok_or_else(|| HttpError::Other("the daemon answered without a hotkey".to_string()))
}

/// Public transport surface for downstream clients that compose their own
/// per-scope endpoint wrappers (e.g. the app): the engine's, plus
/// the `settings_*` calls that read Super STT's [`DaemonResponse`]. Returns
/// [`HttpError`] on transport/auth failure; `401` becomes
/// [`HttpError::InvalidSession`].
pub mod transport {
    use std::path::PathBuf;

    pub use super_engine_client::http_client::transport::*;

    use super::HttpResult;
    use crate::models::protocol::DaemonResponse;

    /// `GET <path>` → `DaemonResponse`. The standard settings read.
    ///
    /// # Errors
    /// Returns [`super::HttpError::InvalidSession`] on `401`;
    /// [`super::HttpError::Other`] on connection, HTTP, or parse failure.
    pub async fn settings_get(
        socket_path: PathBuf,
        token: &str,
        path: &str,
    ) -> HttpResult<DaemonResponse> {
        get_json(socket_path, token, path).await
    }

    /// `POST <path>` with a JSON body → `DaemonResponse`. The standard
    /// settings write.
    ///
    /// # Errors
    /// Returns [`super::HttpError::InvalidSession`] on `401`;
    /// [`super::HttpError::Other`] on connection, HTTP, body encoding, or
    /// parse failure.
    pub async fn settings_post(
        socket_path: PathBuf,
        token: &str,
        path: &str,
        body: &serde_json::Value,
    ) -> HttpResult<DaemonResponse> {
        post_json(socket_path, token, path, body).await
    }

    /// Like [`settings_post`] but without the fixed header timeout — for a
    /// long-running write whose response the daemon only sends once the work
    /// completes (notably `POST /active_model`, a model switch that may stream
    /// multi-GB weights first). See [`post_json_no_timeout`].
    ///
    /// # Errors
    /// Returns [`super::HttpError::InvalidSession`] on `401`;
    /// [`super::HttpError::Other`] on connection, HTTP, body encoding, or
    /// parse failure.
    pub async fn settings_post_no_timeout(
        socket_path: PathBuf,
        token: &str,
        path: &str,
        body: &serde_json::Value,
    ) -> HttpResult<DaemonResponse> {
        post_json_no_timeout(socket_path, token, path, body).await
    }

    /// `DELETE <path>` → `DaemonResponse`.
    ///
    /// # Errors
    /// Returns [`super::HttpError::InvalidSession`] on `401`;
    /// [`super::HttpError::Other`] on connection, HTTP, or parse failure.
    pub async fn settings_delete(
        socket_path: PathBuf,
        token: &str,
        path: &str,
    ) -> HttpResult<DaemonResponse> {
        delete_json(socket_path, token, path).await
    }

    /// `POST <path>` with a JSON body → the response body's bytes, for a
    /// route that answers with a file rather than JSON (`POST
    /// /tts/synthesize`'s WAV). An error status still answers with the JSON
    /// envelope, and maps as every other call's does.
    ///
    /// No header timeout: the answer comes once the work is done, which for
    /// a long text is as long as the model takes. Dropping the future closes
    /// the connection, which is how a caller cancels.
    ///
    /// The engine's own calls all read JSON, so this one is made here, over
    /// the same socket and with the same headers.
    ///
    /// # Errors
    /// Returns [`super::HttpError::InvalidSession`] on `401`;
    /// [`super::HttpError::Other`] on connection, HTTP, or a daemon error.
    pub async fn post_json_for_bytes(
        socket_path: PathBuf,
        token: &str,
        path: &str,
        body: &serde_json::Value,
    ) -> HttpResult<Vec<u8>> {
        use super::HttpError;
        use http_body_util::{BodyExt, Full};
        use hyper::body::Bytes;

        let body = serde_json::to_vec(body)
            .map_err(|e| HttpError::Other(format!("Failed to encode body: {e}")))?;
        let request = hyper::Request::builder()
            .method(hyper::Method::POST)
            .uri(format!("http://localhost/v1{path}"))
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("content-length", body.len().to_string())
            .header("authorization", format!("Bearer {token}"))
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| HttpError::Other(format!("Failed to build request: {e}")))?;
        let stream = tokio::net::UnixStream::connect(&socket_path)
            .await
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                    HttpError::Other(
                        "Daemon HTTP listener not running. Start the daemon first.".to_string(),
                    )
                }
                _ => HttpError::Other(format!("Connection failed: {e}")),
            })?;
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream))
                .await
                .map_err(|e| HttpError::Other(format!("HTTP handshake failed: {e}")))?;
        let driver = tokio::spawn(async move {
            if let Err(e) = connection.await {
                log::debug!("http connection ended: {e}");
            }
        });
        let response = sender
            .send_request(request)
            .await
            .map_err(|e| HttpError::Other(format!("Request failed: {e}")))?;
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|e| HttpError::Other(format!("Failed to read response: {e}")))?
            .to_bytes();
        driver.abort();
        if status.is_success() {
            Ok(bytes.to_vec())
        } else {
            Err(error_for_status(status, &bytes))
        }
    }
}
