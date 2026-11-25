// SPDX-License-Identifier: GPL-3.0-only
//! `/stt/transcribe` — streaming speech-to-text via SSE.

use crate::daemon::client::internal::response::require_unit;
use crate::daemon::client::internal::session::{
    SETTINGS_SCOPES, app_id, app_name, socket_path, with_settings_token,
};
use super_voice_shared::daemon::http_client;
use super_voice_shared::daemon::http_client::HttpResult;
use super_voice_shared::daemon::session;
use super_voice_shared::models::protocol::PreviewSource;

/// Result type for streaming record responses.
pub enum RecordEvent {
    /// Intermediate preview text during recording. The text replaces (not
    /// appends to) any previously-displayed preview; `source` says what it
    /// spans — everything heard so far, or only the last few seconds.
    Preview {
        text: String,
        source: Option<PreviewSource>,
    },
    /// Final transcription result.
    Final(Result<String, String>),
}

/// Start a recording and stream `RecordEvent`s as the daemon emits SSE
/// events on `POST /stt/transcribe`. Closing the returned stream early
/// signals the daemon to stop the recording.
///
/// Nothing is typed: the transcript comes back here, as the daemon gave it,
/// empty when nothing was said. Only [`stop_record_command`] or the daemon's
/// time limit ends the take, never a pause: the Behavior tab's test and a note
/// on the Notes tab both have a Stop button.
///
/// On `invalid_session` from the initial connect (the cached settings
/// token expired or was revoked), we forget the cache, mint a fresh
/// token, and retry once — mirroring the behavior of
/// [`session::with_token`] for non-streaming calls. Without this,
/// once the cached token went stale the settings UI's transcription
/// test panel would surface `"Error: invalid_session (expired)"`
/// forever, since the cache is never invalidated.
pub fn record_command_stream() -> impl futures_util::Stream<Item = RecordEvent> + Send + 'static {
    use futures_util::StreamExt;

    let (channel, events) = tokio::sync::mpsc::channel(32);
    // The take runs inside the returned stream, not on a task of its own:
    // dropping the stream drops the connection with it, and that is what
    // stops the recording on the daemon.
    let take = futures_util::stream::once(run_take(channel)).filter_map(|()| async { None });
    let events = futures_util::stream::unfold(events, |mut events| async move {
        events.recv().await.map(|event| (event, events))
    });
    futures_util::stream::select(events, take)
}

/// One take, from opening the stream to its final event, sent on `channel`.
async fn run_take(channel: tokio::sync::mpsc::Sender<RecordEvent>) {
    use futures_util::StreamExt;
    use super_voice_shared::daemon::http_client::{HttpError, TranscribeEvent, TranscribeOptions};

    let result: Result<(), String> = async {
        let socket = socket_path();
        let opts = TranscribeOptions {
            wait: true,
            write_mode: false,
            stop_mode: Some("manual_only".to_string()),
            // The test panel renders live preview text, so it asks the
            // daemon to stream incremental `preview` frames.
            stream_realtime: true,
            context: None,
        };

        // Try to open the stream with the cached token; on
        // InvalidSession, drop the cache and re-auth once.
        let mut stream = {
            let token = session::obtain(socket.clone(), app_id(), app_name(), SETTINGS_SCOPES)
                .await
                .map_err(|e| e.to_string())?;
            match http_client::transcribe_stream(socket.clone(), &token, opts.clone()).await {
                Ok(s) => Box::pin(s),
                Err(HttpError::InvalidSession { .. }) => {
                    let _ = session::forget(app_id());
                    let token =
                        session::obtain(socket.clone(), app_id(), app_name(), SETTINGS_SCOPES)
                            .await
                            .map_err(|e| e.to_string())?;
                    Box::pin(
                        http_client::transcribe_stream(socket, &token, opts)
                            .await
                            .map_err(|e| e.to_string())?,
                    )
                }
                Err(e) => return Err(e.to_string()),
            }
        };

        while let Some(event) = stream.next().await {
            match event {
                TranscribeEvent::Preview { text, source } => {
                    let _ = channel.send(RecordEvent::Preview { text, source }).await;
                }
                TranscribeEvent::Done(text) => {
                    let _ = channel.send(RecordEvent::Final(Ok(text))).await;
                    return Ok(());
                }
                TranscribeEvent::Error(msg) => {
                    let _ = channel.send(RecordEvent::Final(Err(msg))).await;
                    return Ok(());
                }
            }
        }
        let _ = channel
            .send(RecordEvent::Final(Err(
                "transcribe stream ended unexpectedly".to_string(),
            )))
            .await;
        Ok(())
    }
    .await;

    if let Err(e) = result {
        let _ = channel.send(RecordEvent::Final(Err(e))).await;
    }
}

/// Send a stop signal to a running recording (HTTP `POST /stt/transcribe/stop`).
pub async fn stop_record_command() -> HttpResult<()> {
    with_settings_token(|socket, token| async move {
        let resp = http_client::transcribe_stop(socket, &token).await?;
        require_unit(resp, "stop_record_command")
    })
    .await
}
