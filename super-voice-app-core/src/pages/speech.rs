// SPDX-License-Identifier: GPL-3.0-only
//! The speech test, on text to speech's Behavior tab: text to speak, and what
//! the daemon says it is speaking.
//!
//! The speaking status follows only the daemon's events, never a press here:
//! the daemon speaks for whoever asked, so another app's utterance turns it on
//! too, and flipping it on a press would claim an utterance this page didn't
//! start.

use crate::daemon::client::v1::speak::{speak_command, stop_speaking_command};
use crate::state::{SpeakingStatus, SpeechState};

/// What happened on the page: something the user did, something the daemon
/// said, or the answer to work an [`Effect`] started.
#[derive(Debug, Clone)]
pub enum Input {
    /// The test field now holds this.
    TextChanged(String),
    /// Speak what the field holds.
    Speak,
    /// The daemon took the utterance, and named it or didn't; or why not.
    Spoke(Result<Option<String>, String>),
    /// Stop what the daemon is saying.
    Stop,
    /// The daemon started or stopped speaking (its `speaking_state` event).
    Speaking {
        is_speaking: bool,
        utterance_id: Option<String>,
    },
    /// How loud what it says is, and whether sound reaches the speaker (its
    /// `playback_frequency_bands` event).
    Level { level: f32, is_speech: bool },
}

/// Work [`update`] asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Speak this ([`run_speak`]); answers [`Input::Spoke`].
    Speak(String),
    /// Stop speaking ([`run_stop`]); answers nothing: the daemon's event says
    /// it stopped.
    Stop,
}

/// Something to tell the user. Each app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The daemon refused the utterance.
    CouldNotSpeak(String),
}

/// What an [`update`] asks of the app.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The work to start.
    pub effects: Vec<Effect>,
    /// Whether the page's notice is out of date and goes. Applied before
    /// [`notice`](Self::notice).
    pub clear_notice: bool,
    /// What to tell the user.
    pub notice: Option<Notice>,
}

/// What `input` does to the page.
pub fn update(state: &mut SpeechState, input: Input) -> Outcome {
    let mut out = Outcome::default();
    match input {
        Input::TextChanged(text) => state.test_text = text,
        // Nothing to say is not sent: the daemon refuses an empty utterance,
        // and Speak is off while the field is empty.
        Input::Speak => {
            let text = state.test_text.trim();
            if !text.is_empty() {
                out.clear_notice = true;
                out.effects.push(Effect::Speak(text.to_string()));
            }
        }
        Input::Spoke(Ok(Some(id))) => log::info!("speaking utterance {id}"),
        // Taken, with no name: nothing to match later, and nothing wrong.
        Input::Spoke(Ok(None)) => {}
        Input::Spoke(Err(e)) => out.notice = Some(Notice::CouldNotSpeak(e)),
        Input::Stop => out.effects.push(Effect::Stop),
        Input::Speaking {
            is_speaking,
            utterance_id,
        } => {
            state.status = if is_speaking {
                SpeakingStatus::Speaking
            } else {
                SpeakingStatus::Idle
            };
            state.utterance = utterance_id;
            // No more sound is coming, and the meter would otherwise hold its
            // last level.
            if !is_speaking {
                state.level = 0.0;
                state.is_playing = false;
            }
        }
        Input::Level { level, is_speech } => {
            state.level = level;
            state.is_playing = is_speech;
        }
    }
    out
}

/// Whether Speak can be pressed: nothing is speaking, and there is something
/// to say. While something speaks, the page offers Stop instead.
#[must_use]
pub fn can_speak(state: &SpeechState) -> bool {
    state.status == SpeakingStatus::Idle && !state.test_text.trim().is_empty()
}

/// What the daemon is doing, as the test says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saying {
    /// Nothing.
    Nothing,
    /// It took an utterance, and no sound has reached the speakers yet.
    Starting,
    /// Sound is reaching the speakers.
    Speaking,
}

/// What the daemon is doing: an utterance it took is speaking before its
/// first sound plays.
#[must_use]
pub fn saying(state: &SpeechState) -> Saying {
    match (&state.status, state.is_playing) {
        (SpeakingStatus::Idle, _) => Saying::Nothing,
        (SpeakingStatus::Speaking, false) => Saying::Starting,
        (SpeakingStatus::Speaking, true) => Saying::Speaking,
    }
}

/// Speak `text`.
pub async fn run_speak(text: String) -> Input {
    Input::Spoke(speak_command(text).await.map_err(|e| e.to_string()))
}

/// Stop what the daemon is saying. A failure is only logged: the daemon's
/// event, or its absence, is what the page shows.
pub async fn run_stop() {
    if let Err(e) = stop_speaking_command().await {
        log::warn!("stop speaking failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speak_sends_the_trimmed_text_and_nothing_when_empty() {
        let mut state = SpeechState::default();
        update(&mut state, Input::TextChanged("   ".into()));
        assert!(update(&mut state, Input::Speak).effects.is_empty());
        update(&mut state, Input::TextChanged("  Hello there ".into()));
        let out = update(&mut state, Input::Speak);
        assert_eq!(out.effects, [Effect::Speak("Hello there".into())]);
        assert!(out.clear_notice);
    }

    #[test]
    fn a_press_doesnt_turn_speaking_on_but_the_daemon_does() {
        let mut state = SpeechState::default();
        update(&mut state, Input::TextChanged("Hi".into()));
        update(&mut state, Input::Speak);
        assert_eq!(state.status, SpeakingStatus::Idle);
        update(
            &mut state,
            Input::Speaking {
                is_speaking: true,
                utterance_id: Some("u1".into()),
            },
        );
        assert_eq!(state.status, SpeakingStatus::Speaking);
        assert!(!can_speak(&state));
    }

    #[test]
    fn stopping_empties_the_meter() {
        let mut state = SpeechState::default();
        update(
            &mut state,
            Input::Level {
                level: 0.7,
                is_speech: true,
            },
        );
        assert!(state.is_playing);
        update(
            &mut state,
            Input::Speaking {
                is_speaking: false,
                utterance_id: None,
            },
        );
        assert!(!state.is_playing);
        assert!(state.level.abs() < f32::EPSILON);
    }

    #[test]
    fn an_utterance_starts_before_its_sound() {
        let mut state = SpeechState::default();
        assert_eq!(saying(&state), Saying::Nothing);
        update(
            &mut state,
            Input::Speaking {
                is_speaking: true,
                utterance_id: None,
            },
        );
        assert_eq!(saying(&state), Saying::Starting);
        update(
            &mut state,
            Input::Level {
                level: 0.4,
                is_speech: true,
            },
        );
        assert_eq!(saying(&state), Saying::Speaking);
    }

    #[test]
    fn a_refusal_is_said() {
        let mut state = SpeechState::default();
        let out = update(&mut state, Input::Spoke(Err("no model".into())));
        assert_eq!(out.notice, Some(Notice::CouldNotSpeak("no model".into())));
    }
}
