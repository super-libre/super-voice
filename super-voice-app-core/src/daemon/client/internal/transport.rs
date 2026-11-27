// SPDX-License-Identifier: GPL-3.0-only
//! The transport every daemon call of the core goes through: the shared
//! client's calls the core makes, each said in the log once it is answered,
//! its method and path, how long it took, and ok or the kind of error. A call
//! the core starts making gets its wrapper here.
//!
//! ```text
//! DEBUG super_voice_app_core::daemon] GET /stt/pipeline/1/model: ok in 4 ms
//! DEBUG super_voice_app_core::daemon] POST /tts/voice: not_found (HTTP 404) after 2 ms
//! ```
//!
//! A path's query is left out (a cloned voice's carries its transcript), and
//! so are bodies, answers and the token. The calls the app repeats on a timer
//! (the keep-alive ping, the GPU and typing checks, the Assistants list) run
//! [`quietly`], at trace, so debug stays readable.

use std::future::Future;
use std::path::PathBuf;
use std::time::Instant;

use serde::de::DeserializeOwned;
use super_voice_shared::daemon::http_client::{
    self, HttpError, HttpResult, TranscribeEvent, TranscribeOptions,
};
use super_voice_shared::models::protocol::DaemonResponse;

/// The log target the requests are said under.
const TARGET: &str = "super_voice_app_core::daemon";

tokio::task_local! {
    /// Set while a poll's requests run: they are said at trace.
    static QUIET: ();
}

/// Run `work`, whose daemon requests are said at trace instead of debug: for
/// a call the app repeats every few seconds.
pub async fn quietly<F: Future>(work: F) -> F::Output {
    QUIET.scope((), work).await
}

/// Make the request `call` is, and say it: `method` and `path`, how long it
/// took, and how it ended.
pub(crate) async fn said<T>(
    method: &str,
    path: &str,
    call: impl Future<Output = HttpResult<T>>,
) -> HttpResult<T> {
    let started = Instant::now();
    let result = call.await;
    let level = if QUIET.try_with(|()| ()).is_ok() {
        log::Level::Trace
    } else {
        log::Level::Debug
    };
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let ms = started.elapsed().as_millis();
    match &result {
        Ok(_) => log::log!(target: TARGET, level, "{method} {path}: ok in {ms} ms"),
        Err(e) => log::log!(
            target: TARGET,
            level,
            "{method} {path}: {} after {ms} ms",
            error_kind(e)
        ),
    }
    result
}

/// What kind of error `error` is, in a few words: the daemon's error code and
/// HTTP status, or the client's own failure without its detail. Never the
/// daemon's message, which may quote what was sent.
#[must_use]
pub fn error_kind(error: &HttpError) -> String {
    let HttpError::Other(message) = error else {
        return error.to_string();
    };
    if let Some(open) = message.rfind(" (HTTP ")
        && message.ends_with(')')
    {
        let head = &message[..open];
        let code = head.split_once(':').map_or(head, |(code, _)| code);
        let is_code = !code.is_empty()
            && code
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        let code = if is_code { code } else { "error" };
        return format!("{code}{}", &message[open..]);
    }
    message
        .split_once(':')
        .map_or(message.as_str(), |(kind, _)| kind)
        .chars()
        .take(60)
        .collect()
}

pub async fn settings_get(
    socket_path: PathBuf,
    token: &str,
    path: &str,
) -> HttpResult<DaemonResponse> {
    let call =
        super_voice_shared::daemon::http_client::transport::settings_get(socket_path, token, path);
    said("GET", path, call).await
}

pub async fn settings_post(
    socket_path: PathBuf,
    token: &str,
    path: &str,
    body: &serde_json::Value,
) -> HttpResult<DaemonResponse> {
    let call = super_voice_shared::daemon::http_client::transport::settings_post(
        socket_path,
        token,
        path,
        body,
    );
    said("POST", path, call).await
}

pub async fn settings_post_no_timeout(
    socket_path: PathBuf,
    token: &str,
    path: &str,
    body: &serde_json::Value,
) -> HttpResult<DaemonResponse> {
    let call = super_voice_shared::daemon::http_client::transport::settings_post_no_timeout(
        socket_path,
        token,
        path,
        body,
    );
    said("POST", path, call).await
}

pub async fn settings_delete(
    socket_path: PathBuf,
    token: &str,
    path: &str,
) -> HttpResult<DaemonResponse> {
    let call = super_voice_shared::daemon::http_client::transport::settings_delete(
        socket_path,
        token,
        path,
    );
    said("DELETE", path, call).await
}

pub async fn get_json<T: DeserializeOwned>(
    socket_path: PathBuf,
    token: &str,
    path: &str,
) -> HttpResult<T> {
    let call =
        super_voice_shared::daemon::http_client::transport::get_json(socket_path, token, path);
    said("GET", path, call).await
}

pub async fn post_json<T: DeserializeOwned>(
    socket_path: PathBuf,
    token: &str,
    path: &str,
    body: &serde_json::Value,
) -> HttpResult<T> {
    let call = super_voice_shared::daemon::http_client::transport::post_json(
        socket_path,
        token,
        path,
        body,
    );
    said("POST", path, call).await
}

pub async fn post_json_no_timeout<T: DeserializeOwned>(
    socket_path: PathBuf,
    token: &str,
    path: &str,
    body: &serde_json::Value,
) -> HttpResult<T> {
    let call = super_voice_shared::daemon::http_client::transport::post_json_no_timeout(
        socket_path,
        token,
        path,
        body,
    );
    said("POST", path, call).await
}

pub async fn patch_json<T: DeserializeOwned>(
    socket_path: PathBuf,
    token: &str,
    path: &str,
    body: &serde_json::Value,
) -> HttpResult<T> {
    let call = super_voice_shared::daemon::http_client::transport::patch_json(
        socket_path,
        token,
        path,
        body,
    );
    said("PATCH", path, call).await
}

pub async fn delete_json<T: DeserializeOwned>(
    socket_path: PathBuf,
    token: &str,
    path: &str,
) -> HttpResult<T> {
    let call =
        super_voice_shared::daemon::http_client::transport::delete_json(socket_path, token, path);
    said("DELETE", path, call).await
}

pub async fn post_bytes<T: DeserializeOwned>(
    socket_path: PathBuf,
    token: &str,
    path: &str,
    content_type: &str,
    body: Vec<u8>,
) -> HttpResult<T> {
    let call = super_voice_shared::daemon::http_client::transport::post_bytes(
        socket_path,
        token,
        path,
        content_type,
        body,
    );
    said("POST", path, call).await
}

pub async fn post_json_for_bytes(
    socket_path: PathBuf,
    token: &str,
    path: &str,
    body: &serde_json::Value,
) -> HttpResult<Vec<u8>> {
    let call = super_voice_shared::daemon::http_client::transport::post_json_for_bytes(
        socket_path,
        token,
        path,
        body,
    );
    said("POST", path, call).await
}

/// `GET /ping`.
pub async fn ping(socket_path: PathBuf, token: &str) -> HttpResult<String> {
    said("GET", "/ping", http_client::ping(socket_path, token)).await
}

/// `GET /status`.
pub async fn status(
    socket_path: PathBuf,
    token: &str,
) -> HttpResult<super_voice_shared::models::status::VoiceStatus> {
    said("GET", "/status", http_client::status(socket_path, token)).await
}

/// `POST /stt/transcribe`, streamed: said once the stream is open.
pub async fn transcribe_stream(
    socket_path: PathBuf,
    token: &str,
    opts: TranscribeOptions,
) -> HttpResult<impl futures_util::Stream<Item = TranscribeEvent> + Send + 'static> {
    let call = http_client::transcribe_stream(socket_path, token, opts);
    said("POST", "/stt/transcribe (stream)", call).await
}

/// `POST /stt/transcribe/stop`.
pub async fn transcribe_stop(socket_path: PathBuf, token: &str) -> HttpResult<DaemonResponse> {
    let call = http_client::transcribe_stop(socket_path, token);
    said("POST", "/stt/transcribe/stop", call).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_errors_kind_leaves_out_what_the_daemon_said() {
        let kind = |e: HttpError| error_kind(&e);
        assert_eq!(
            kind(HttpError::Other("invalid_topic (HTTP 400)".to_string())),
            "invalid_topic (HTTP 400)"
        );
        assert_eq!(
            kind(HttpError::Other(
                "bad_request: the key sk-123 is not valid (HTTP 400)".to_string()
            )),
            "bad_request (HTTP 400)"
        );
        assert_eq!(
            kind(HttpError::Other(
                "The key sk-123 is not valid (HTTP 400)".to_string()
            )),
            "error (HTTP 400)"
        );
        assert_eq!(
            kind(HttpError::Other(
                "Connection failed: No such file or directory (os error 2)".to_string()
            )),
            "Connection failed"
        );
        assert_eq!(
            kind(HttpError::InvalidSession {
                reason: "expired".to_string()
            }),
            "invalid_session (expired)"
        );
        assert_eq!(kind(HttpError::ScopeDenied), "scope_denied (HTTP 403)");
    }

    #[tokio::test]
    async fn a_request_is_answered_as_it_was() {
        let answer = said("GET", "/tts/voice?label=Hi", async {
            Ok::<_, HttpError>(7)
        })
        .await;
        assert_eq!(answer.ok(), Some(7));
        let quiet = quietly(said("GET", "/ping", async {
            Err::<(), _>(HttpError::ScopeDenied)
        }))
        .await;
        assert!(quiet.is_err());
    }
}
