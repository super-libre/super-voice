// SPDX-License-Identifier: GPL-3.0-only
//! The microphone, as the daemon's events report it. Every page that records
//! reads it: whether something else holds the microphone, how loud it is, and
//! whether that sounds like speech.

use crate::events::Event;

/// What the daemon last said about its microphone.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Microphone {
    /// Whether the microphone is open: a take of any app, or dictation.
    pub recording: bool,
    /// How loud it is, 0 to 1.
    pub level: f32,
    /// Whether that sounds like speech.
    pub speech: bool,
}

impl Microphone {
    /// Take in `event`. Whether anything changed: most events are about
    /// something else, and a meter's reading often repeats.
    pub fn follow(&mut self, event: &Event) -> bool {
        let next = match *event {
            Event::Recording(true) => Self {
                recording: true,
                ..*self
            },
            // A closed microphone is a quiet one, whatever its last reading.
            // A stream live again missed what happened while it was down, so
            // nothing is taken to be recording until the daemon says so.
            Event::Recording(false) | Event::Connected => Self::default(),
            Event::MicrophoneLevel { level, is_speech } => Self {
                level,
                speech: is_speech,
                ..*self
            },
            _ => return false,
        };
        let changed = next != *self;
        *self = next;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::Microphone;
    use crate::events::Event;

    #[test]
    fn opens_reads_and_closes() {
        let mut microphone = Microphone::default();
        assert!(microphone.follow(&Event::Recording(true)));
        assert!(microphone.follow(&Event::MicrophoneLevel {
            level: 0.6,
            is_speech: true,
        }));
        assert_eq!(
            microphone,
            Microphone {
                recording: true,
                level: 0.6,
                speech: true,
            }
        );
        assert!(microphone.follow(&Event::Recording(false)));
        assert_eq!(microphone, Microphone::default());
    }

    #[test]
    fn a_repeated_reading_or_another_event_changes_nothing() {
        let mut microphone = Microphone::default();
        let reading = Event::MicrophoneLevel {
            level: 0.2,
            is_speech: false,
        };
        assert!(microphone.follow(&reading));
        assert!(!microphone.follow(&reading));
        assert!(!microphone.follow(&Event::Speaking {
            is_speaking: true,
            utterance_id: None,
        }));
    }

    /// The daemon may have closed the microphone while the stream was down.
    #[test]
    fn a_reconnect_forgets_the_microphone() {
        let mut microphone = Microphone {
            recording: true,
            level: 0.4,
            speech: true,
        };
        assert!(microphone.follow(&Event::Connected));
        assert_eq!(microphone, Microphone::default());
    }
}
