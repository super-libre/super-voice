// SPDX-License-Identifier: GPL-3.0-only

use super_voice_app_core::events::Event;
use super_voice_shared::registry::events::RegistryEvent;

use crate::state::DaemonStatus;
use crate::ui::messages::{
    DaemonMessage, Message, ModelsPageMessage, RecordingMessage, SpeechMessage,
};

/// Classify a daemon [`HttpError`] into the right next `DaemonStatus`.
///
/// A user-triggered `auth_denied` ([`is_user_denied`]) must transition to
/// [`DaemonStatus::Blocked`] so the surrounding auto-retry loop stops firing —
/// otherwise the settings app re-pings every 5s and the daemon's in-memory deny
/// cache keeps logging the same `user_denied_cached`. Every other error is
/// transient (daemon restarting, socket missing, token expiry, …) and gets
/// [`DaemonStatus::Error`], which the caller pairs with the 5s retry. The
/// decision is on the typed variant; the human string is kept only for display.
///
/// [`HttpError`]: super_voice_shared::daemon::http_client::HttpError
/// [`is_user_denied`]: super_voice_shared::daemon::widget_subscription::is_user_denied
pub(super) fn classify_daemon_error(
    err: super_voice_shared::daemon::http_client::HttpError,
) -> DaemonStatus {
    if super_voice_shared::daemon::widget_subscription::is_user_denied(&err) {
        DaemonStatus::Blocked(err.into())
    } else {
        DaemonStatus::Error(err.into())
    }
}

/// The message an event from the daemon is, or `None` for one this app
/// doesn't show.
pub(super) fn message(event: Event) -> Option<Message> {
    Some(match event {
        // A successful (re)subscribe: clears a sticky Blocked or Error badge
        // (a denial the user has since cleared, say) and reads the models'
        // state again, for a model that finished loading before the stream
        // was live.
        Event::Connected => Message::Daemon(DaemonMessage::EventStreamConnected),
        // So the page shows Blocked, with Retry, rather than a dead meter.
        Event::Blocked(reason) => Message::Daemon(DaemonMessage::WidgetBlocked(reason)),
        Event::Recording(on) => Message::Recording(RecordingMessage::WidgetRecordingState(on)),
        Event::MicrophoneLevel { level, is_speech } => {
            Message::Recording(RecordingMessage::WidgetAudioLevel { level, is_speech })
        }
        Event::Speaking {
            is_speaking,
            utterance_id,
        } => Message::Speech(SpeechMessage::WidgetSpeakingState {
            is_speaking,
            utterance_id,
        }),
        Event::PlaybackLevel { level, is_speech } => {
            Message::Speech(SpeechMessage::WidgetAudioLevel { level, is_speech })
        }
        Event::Status(notification) => {
            Message::Daemon(DaemonMessage::DaemonEventsReceived(vec![notification]))
        }
        Event::Install(RegistryEvent::Progress {
            install_id,
            source,
            phase,
            bytes_done,
            bytes_total,
        }) => Message::ModelsPage(ModelsPageMessage::InstallProgress {
            install_id,
            source,
            phase,
            bytes_done,
            bytes_total,
        }),
        Event::Install(RegistryEvent::Completed { source, .. }) => {
            Message::ModelsPage(ModelsPageMessage::InstallCompleted { source })
        }
        Event::Install(RegistryEvent::Failed {
            install_id,
            source,
            phase,
            error,
            reason,
        }) => Message::ModelsPage(ModelsPageMessage::InstallFailed {
            install_id,
            source,
            phase,
            error,
            reason,
        }),
        Event::Install(
            RegistryEvent::RefreshCompleted { .. } | RegistryEvent::RefreshFailed { .. },
        ) => return None,
    })
}

#[cfg(test)]
mod classify_daemon_error_tests {
    //! Pin the decision in `Message::DaemonError`: which error
    //! strings should suppress the 5s auto-retry vs. trigger it.
    //! Locking this in protects against a regression of the deny
    //! spam loop the helper change already fixed for the applet.
    use super::*;
    use super_voice_shared::daemon::http_client::HttpError;

    fn auth_denied(reason: &str) -> HttpError {
        HttpError::AuthDenied {
            reason: reason.to_string(),
        }
    }

    #[test]
    fn user_denied_cached_routes_to_blocked() {
        let next = classify_daemon_error(auth_denied("user_denied_cached"));
        match next {
            DaemonStatus::Blocked(reason) => {
                // Blocked keeps the human string for display.
                assert_eq!(reason, "auth_denied (user_denied_cached)");
            }
            other => panic!("user_denied_cached must route to Blocked, got {other:?}"),
        }
    }

    #[test]
    fn fresh_user_denied_routes_to_blocked() {
        let next = classify_daemon_error(auth_denied("user_denied"));
        assert!(matches!(next, DaemonStatus::Blocked(_)));
    }

    #[test]
    fn dismissed_popup_routes_to_error_so_retry_can_recover() {
        // user_dismissed is recoverable — next attempt pops the
        // popup fresh — so the retry loop must keep firing.
        let next = classify_daemon_error(auth_denied("user_dismissed"));
        assert!(matches!(next, DaemonStatus::Error(_)));
    }

    #[test]
    fn invalid_session_routes_to_error() {
        // Token expiry / exe_changed are transient — let the
        // retry loop drive a fresh consent on the next attempt.
        let next = classify_daemon_error(HttpError::InvalidSession {
            reason: "expired".to_string(),
        });
        assert!(matches!(next, DaemonStatus::Error(_)));
    }

    #[test]
    fn socket_unreachable_routes_to_error() {
        // Daemon restarting / socket missing — pure transient.
        let next = classify_daemon_error(HttpError::Other(
            "Daemon HTTP listener not running. Start the daemon first.".to_string(),
        ));
        assert!(matches!(next, DaemonStatus::Error(_)));
    }
}
