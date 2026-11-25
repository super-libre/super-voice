// SPDX-License-Identifier: GPL-3.0-only

use crate::core::app::AppModel;
use crate::daemon::client::{RecordEvent, record_command_stream, stop_record_command};
use crate::state::RecordingStatus;
use crate::ui::messages::{Message, RecordingMessage};
use cosmic::prelude::*;
use futures_util::StreamExt;

impl AppModel {
    /// Route recording/audio messages to the appropriate helper.
    pub(in crate::core::app) fn handle_recording_messages(
        &mut self,
        message: RecordingMessage,
    ) -> Task<cosmic::Action<Message>> {
        match &message {
            RecordingMessage::StartRecording
            | RecordingMessage::StopRecording
            | RecordingMessage::PreviewReceived { .. }
            | RecordingMessage::TranscriptionReceived(_) => self.handle_recording_control(message),

            RecordingMessage::WidgetAudioLevel { .. }
            | RecordingMessage::WidgetRecordingState(_) => self.handle_audio_messages(&message),
        }
    }

    /// Handle recording control messages: start/stop recording and transcription results.
    fn handle_recording_control(
        &mut self,
        message: RecordingMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            RecordingMessage::StartRecording => {
                if matches!(self.recording_status, RecordingStatus::Recording) {
                    return Task::none();
                }

                self.recording_status = RecordingStatus::Recording;
                self.transcription_text.clear();
                self.preview_text.clear();
                self.preview_source = None;

                let stream = record_command_stream();
                cosmic::task::stream(stream.map(|event| match event {
                    RecordEvent::Preview { text, source } => {
                        cosmic::Action::App(Message::Recording(RecordingMessage::PreviewReceived {
                            text,
                            source,
                        }))
                    }
                    // A blank result is a recording of nothing, not a
                    // failure: the test says so in words rather than leaving
                    // an empty box.
                    RecordEvent::Final(Ok(text)) if text.trim().is_empty() => cosmic::Action::App(
                        Message::Recording(RecordingMessage::TranscriptionReceived(crate::fl!(
                            "no-speech-detected"
                        ))),
                    ),
                    RecordEvent::Final(Ok(text)) => cosmic::Action::App(Message::Recording(
                        RecordingMessage::TranscriptionReceived(text),
                    )),
                    RecordEvent::Final(Err(e)) => cosmic::Action::App(Message::Recording(
                        RecordingMessage::TranscriptionReceived(crate::fl!(
                            "the-recording-failed",
                            error = e
                        )),
                    )),
                }))
            }

            RecordingMessage::StopRecording => Task::perform(stop_record_command(), |result| {
                if let Err(e) = result {
                    log::warn!("Stop recording failed: {e}");
                }
                cosmic::Action::None
            }),

            RecordingMessage::PreviewReceived { text, source } => {
                // Previews land on their own line; the final result below is
                // left alone until it actually arrives.
                self.preview_text = text;
                self.preview_source = source;
                Task::none()
            }

            RecordingMessage::TranscriptionReceived(text) => {
                log::info!(
                    "TranscriptionReceived: '{}'",
                    text.chars().take(50).collect::<String>()
                );
                self.transcription_text = text;
                // The live line has been superseded by the final transcript.
                self.preview_text.clear();
                self.preview_source = None;
                self.recording_status = RecordingStatus::Idle;
                self.audio_level = 0.0;
                Task::none()
            }

            _ => Task::none(),
        }
    }

    /// The microphone's level and state, from the daemon's events.
    fn handle_audio_messages(
        &mut self,
        message: &RecordingMessage,
    ) -> Task<cosmic::Action<Message>> {
        match *message {
            RecordingMessage::WidgetAudioLevel { level, is_speech } => {
                self.last_udp_data = std::time::Instant::now();
                self.audio_level = level;
                self.is_speech_detected = is_speech;
                Task::none()
            }

            RecordingMessage::WidgetRecordingState(is_recording) => {
                self.last_udp_data = std::time::Instant::now();
                self.recording_status = if is_recording {
                    RecordingStatus::Recording
                } else {
                    RecordingStatus::Idle
                };
                Task::none()
            }

            _ => Task::none(),
        }
    }
}
