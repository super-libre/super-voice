// SPDX-License-Identifier: GPL-3.0-only
//! Speech to text's recording test, on its Behavior tab: Test recording
//! records a take through the daemon, shows its words as they come, and then
//! its transcript.
//!
//! The test shares the microphone's state with every page that records: it
//! marks the microphone open the moment it starts, before the daemon says so,
//! so a second press, or another page's Record, waits for it.

use futures_util::{Stream, StreamExt};
use super_voice_shared::models::protocol::PreviewSource;

use crate::daemon::client::{RecordEvent, record_command_stream, stop_record_command};
use crate::state::Microphone;
use crate::state::recording_test::{Heard, RecordingTest};

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Test recording was pressed.
    Start,
    /// Stop recording was pressed.
    Stop,
    /// The take heard this much so far.
    Preview {
        text: String,
        source: Option<PreviewSource>,
    },
    /// The take is over: its transcript, or why it failed.
    Transcribed(Result<String, String>),
    /// The daemon is back after the app lost it: what the test showed
    /// belonged to a session that is gone.
    Reconnected,
}

/// What the app should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Record a take: run [`take`] and hand each of its inputs back.
    Take,
    /// Ask the daemon to stop it: [`run_stop`].
    Stop,
}

/// Take in `input`, and say what to do.
pub fn update(test: &mut RecordingTest, microphone: &mut Microphone, input: Input) -> Vec<Effect> {
    match input {
        Input::Start => {
            // The daemon records one take at a time.
            if microphone.recording {
                return Vec::new();
            }
            microphone.recording = true;
            *test = RecordingTest::default();
            vec![Effect::Take]
        }
        Input::Stop => vec![Effect::Stop],
        // Previews have a line of their own; the transcript is left alone
        // until it arrives.
        Input::Preview { text, source } => {
            test.preview = text;
            test.preview_source = source;
            Vec::new()
        }
        Input::Transcribed(result) => {
            test.heard = Some(match result {
                // A blank transcript is a recording of nothing, not a failure.
                Ok(words) if words.trim().is_empty() => Heard::Nothing,
                Ok(words) => Heard::Words(words),
                Err(e) => Heard::Failed(e),
            });
            // The live line gives way to the transcript.
            test.preview.clear();
            test.preview_source = None;
            microphone.recording = false;
            microphone.level = 0.0;
            Vec::new()
        }
        Input::Reconnected => {
            *test = RecordingTest::default();
            Vec::new()
        }
    }
}

/// Record a take: what it hears so far, then its transcript. Dropping the
/// stream stops the recording.
pub fn take() -> impl Stream<Item = Input> + Send + 'static {
    record_command_stream().map(|event| match event {
        RecordEvent::Preview { text, source } => Input::Preview { text, source },
        RecordEvent::Final(result) => Input::Transcribed(result),
    })
}

/// Ask the daemon to stop the take.
pub async fn run_stop() {
    if let Err(e) = stop_record_command().await {
        log::warn!("Stopping the recording test failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::{Effect, Input, update};
    use crate::state::Microphone;
    use crate::state::recording_test::{Heard, RecordingTest};

    #[test]
    fn a_take_marks_the_microphone_open_at_once_and_ends_with_its_words() {
        let mut test = RecordingTest {
            heard: Some(Heard::Words("last time".into())),
            ..RecordingTest::default()
        };
        let mut microphone = Microphone::default();

        assert_eq!(
            update(&mut test, &mut microphone, Input::Start),
            [Effect::Take]
        );
        assert!(microphone.recording, "before the daemon says so");
        assert_eq!(test.heard, None, "the last take's words go");

        update(
            &mut test,
            &mut microphone,
            Input::Preview {
                text: "hello".into(),
                source: None,
            },
        );
        assert_eq!(test.preview, "hello");

        microphone.level = 0.7;
        update(
            &mut test,
            &mut microphone,
            Input::Transcribed(Ok("Hello there.".into())),
        );
        assert_eq!(test.heard, Some(Heard::Words("Hello there.".into())));
        assert_eq!(test.preview, "");
        assert!(!microphone.recording);
        assert!(
            microphone.level.abs() < f32::EPSILON,
            "the meter goes quiet"
        );
    }

    #[test]
    fn a_second_press_while_recording_does_nothing() {
        let mut test = RecordingTest::default();
        let mut microphone = Microphone {
            recording: true,
            ..Microphone::default()
        };
        assert!(update(&mut test, &mut microphone, Input::Start).is_empty());
    }

    #[test]
    fn a_blank_transcript_is_nothing_heard_and_an_error_a_failure() {
        let mut test = RecordingTest::default();
        let mut microphone = Microphone::default();
        update(
            &mut test,
            &mut microphone,
            Input::Transcribed(Ok("  ".into())),
        );
        assert_eq!(test.heard, Some(Heard::Nothing));
        update(
            &mut test,
            &mut microphone,
            Input::Transcribed(Err("no model".into())),
        );
        assert_eq!(test.heard, Some(Heard::Failed("no model".into())));
    }

    #[test]
    fn a_reconnect_clears_the_test() {
        let mut test = RecordingTest {
            preview: "half".into(),
            heard: Some(Heard::Nothing),
            ..RecordingTest::default()
        };
        update(&mut test, &mut Microphone::default(), Input::Reconnected);
        assert_eq!(test, RecordingTest::default());
    }
}
