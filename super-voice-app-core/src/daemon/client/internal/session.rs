// SPDX-License-Identifier: GPL-3.0-only
//! Session-token machinery for every app→daemon call.
//!
//! One cached token (minted via the daemon consent popup) authorizes
//! everything the app does. [`with_settings_token`] runs an
//! operation with that token and, on `invalid_session`, drops the cache
//! and retries once with fresh consent.

use super_voice_shared::daemon::http_client::{HttpError, HttpResult};
use super_voice_shared::daemon::session::{self, AppId};
use super_voice_shared::validation::get_http_socket_path;

/// Scope set the app requests. One cached token covers
/// everything the app does: config + registry (`settings`), the
/// test-recording panel (`transcribe`), the test utterance (`speak`), the
/// cloned-voice library (`voices`), and the `/events` subscription
/// (`recording_events` and `playback_events` for the two badges,
/// `audio_visualization` for the meters, `daemon_status` for model-switch /
/// download / install progress).
pub const SETTINGS_SCOPES: &[&str] = &[
    "settings",
    "secrets",
    "transcribe",
    "recording_events",
    "speak",
    "synthesize",
    "voices",
    "playback_events",
    "audio_visualization",
    "daemon_status",
];
/// The Super Voice app's token entry, and every app's unless it says otherwise
/// with [`set_identity`].
pub(crate) const APP_ID_NAME: AppId = session::app_id("super-voice-app");
/// The name the consent dialog shows for the app.
const APP_NAME: &str = "Super Voice App";

/// Who this app is to the daemon, when it is not the Super Voice app: the entry
/// its token is kept under, and the name the consent dialog shows. Set once,
/// before the first daemon call.
static IDENTITY: std::sync::OnceLock<(AppId, &'static str)> = std::sync::OnceLock::new();

/// Make this app a client of its own: its token kept under `app_id`, and the
/// consent dialog naming it `name`. For an app other than the Super Voice app,
/// called before its first daemon call: the daemon binds a token to the binary
/// it was granted to, so two apps sharing an entry would replace each other's
/// token. Returns `false` when an identity was already set.
pub fn set_identity(app_id: &'static str, name: &'static str) -> bool {
    IDENTITY.set((session::app_id(app_id), name)).is_ok()
}

/// The name the consent dialog shows for this app.
pub(crate) fn app_name() -> &'static str {
    IDENTITY.get().map_or(APP_NAME, |(_, name)| name)
}

/// Where a development build keeps its token instead. The daemon binds a
/// token to the binary it was granted to, so a debug build storing its own
/// under [`APP_ID_NAME`] would replace the installed app's token with one the
/// installed app cannot use, and its next launch would ask for consent again.
const SCENE_APP_ID: AppId = session::app_id("super-voice-app-scene");

/// Whether this run keeps its token under [`SCENE_APP_ID`]; set once at start
/// by [`use_scene_token`].
static SCENE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Keep this run's token apart from the installed app's, for a debug build
/// (`just run-app`). The app calls it at start, before its first daemon call.
pub fn use_scene_token() {
    SCENE.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The entry this run's token lives in: [`SCENE_APP_ID`] once
/// [`use_scene_token`] has been called, else the one [`set_identity`] gave,
/// else [`APP_ID_NAME`].
pub fn app_id() -> AppId {
    if SCENE.load(std::sync::atomic::Ordering::Relaxed) {
        SCENE_APP_ID
    } else {
        IDENTITY.get().map_or(APP_ID_NAME, |(id, _)| *id)
    }
}

/// The socket every app→daemon call connects on.
///
/// One resolver rather than a `get_http_socket_path()` per call site: the
/// streaming path in [`crate::daemon::client::v1::transcribe`] does not go
/// through [`with_settings_token`], and both have to reach the same daemon.
/// It is also the single seam the wrapper tests redirect at a fake daemon.
pub(crate) fn socket_path() -> std::path::PathBuf {
    #[cfg(test)]
    if let Some(path) = test_socket::get() {
        return path;
    }
    get_http_socket_path()
}

/// Test-only redirection of [`socket_path`], compiled out of the shipped
/// binary. Set while a `fake_daemon::FakeDaemon` is alive; that guard holds an
/// exclusive lock for its lifetime, so only one test at a time observes it.
#[cfg(test)]
pub(crate) mod test_socket {
    use std::path::PathBuf;
    use std::sync::Mutex;

    static SLOT: Mutex<Option<PathBuf>> = Mutex::new(None);

    pub(crate) fn get() -> Option<PathBuf> {
        SLOT.lock().expect("test socket slot").clone()
    }

    pub(crate) fn set(path: Option<PathBuf>) {
        *SLOT.lock().expect("test socket slot") = path;
    }
}

/// Run an HTTP-protocol operation with the cached settings-scope token.
/// On `invalid_session` the cache is invalidated and the operation
/// retries once with a fresh consent flow. On `rate_limited` it waits a
/// moment and asks once more (see [`rate_limit_wait`]).
pub(crate) async fn with_settings_token<F, Fut, T>(op: F) -> HttpResult<T>
where
    F: Fn(std::path::PathBuf, String) -> Fut,
    Fut: std::future::Future<Output = HttpResult<T>>,
{
    let socket = socket_path();
    let socket_for_op = socket.clone();
    let run = || {
        session::with_token(
            socket.clone(),
            app_id(),
            app_name(),
            SETTINGS_SCOPES,
            |token| op(socket_for_op.clone(), token),
        )
    };
    match run().await {
        Err(e) if is_rate_limited(&e) => {
            let wait = rate_limit_wait(std::time::SystemTime::now());
            log::debug!("The daemon asked to slow down; asking again in {wait:?}");
            tokio::time::sleep(wait).await;
            run().await
        }
        result => result,
    }
}

/// Whether the daemon refused the request as one too many: its
/// `rate_limited` answer, `429`.
#[must_use]
pub fn is_rate_limited(error: &HttpError) -> bool {
    matches!(error, HttpError::Other(message) if message.ends_with("(HTTP 429)"))
}

/// How long to wait before asking again after a `rate_limited` answer.
///
/// The daemon lets one client make 600 requests in a rolling minute and
/// counts the ones it refuses too (`super-voice-daemon/src/limits.rs`, the
/// engine's `ResourceManager`). The app only reaches that in a burst (every
/// page reading at once), which is over in a moment, so a short wait clears
/// it; asking again at once would only add to the count. A spread of up to
/// a second, taken from the clock, keeps the requests refused together from
/// all coming back at the same instant.
#[must_use]
pub fn rate_limit_wait(now: std::time::SystemTime) -> std::time::Duration {
    const WAIT: std::time::Duration = std::time::Duration::from_millis(1500);
    const SPREAD_MS: u32 = 1000;
    if cfg!(test) {
        return std::time::Duration::from_millis(10);
    }
    let spread = now
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_millis() % SPREAD_MS);
    WAIT + std::time::Duration::from_millis(u64::from(spread))
}
