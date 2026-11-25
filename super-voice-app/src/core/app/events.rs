// SPDX-License-Identifier: GPL-3.0-only

use super_voice_app_core::events::Event;
use super_voice_app_core::pages::speech;
use super_voice_shared::registry::events::RegistryEvent;

use crate::ui::messages::{
    DaemonMessage, Message, ModelsPageMessage, RecordingMessage, SpeechMessage,
};

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
        Event::Recording(_) | Event::MicrophoneLevel { .. } => {
            Message::Recording(RecordingMessage::Microphone(event))
        }
        Event::Speaking {
            is_speaking,
            utterance_id,
        } => Message::Speech(SpeechMessage::Page(speech::Input::Speaking {
            is_speaking,
            utterance_id,
        })),
        Event::PlaybackLevel { level, is_speech } => {
            Message::Speech(SpeechMessage::Page(speech::Input::Level {
                level,
                is_speech,
            }))
        }
        Event::Status(notification) => {
            Message::Daemon(DaemonMessage::DaemonEventsReceived(vec![notification]))
        }
        Event::Install(
            event @ (RegistryEvent::Progress { .. }
            | RegistryEvent::Completed { .. }
            | RegistryEvent::Failed { .. }),
        ) => Message::ModelsPage(ModelsPageMessage::Library(
            super_voice_app_core::pages::library::Input::Event(event),
        )),
        Event::Install(
            RegistryEvent::RefreshCompleted { .. } | RegistryEvent::RefreshFailed { .. },
        ) => return None,
    })
}
