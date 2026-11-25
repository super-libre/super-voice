// SPDX-License-Identifier: GPL-3.0-only
//! Text to speech's speech test, wired to its logic in the core
//! (`super_voice_app_core::pages::speech`). The app runs the work it asks for
//! as iced tasks, and says its notice in its own words.

use cosmic::prelude::*;
use super_voice_app_core::pages::speech::{self as page, Effect, Input, Notice, Outcome};

use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::ui::messages::{Message, SpeechMessage};

fn page_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Speech(SpeechMessage::Page(input)))
}

impl AppModel {
    pub(in crate::core::app) fn handle_speech_messages(
        &mut self,
        message: SpeechMessage,
    ) -> Task<cosmic::Action<Message>> {
        let SpeechMessage::Page(input) = message;
        match &input {
            Input::Level { .. } => self.last_udp_data = std::time::Instant::now(),
            Input::Speaking { is_speaking, .. } => {
                self.last_udp_data = std::time::Instant::now();
                // Whatever the Voices tab was waiting for is done: a voice is
                // registered before its utterance is taken, so speech starting
                // means its preparation finished. The event is the device's,
                // so another app's utterance clears the label too; matching
                // on an utterance id the preview doesn't keep would fix a case
                // that costs a label cleared early.
                if *is_speaking {
                    self.voices.previewing = None;
                    self.voices.preparing = None;
                }
            }
            _ => {}
        }
        let Outcome {
            effects,
            clear_notice,
            notice,
        } = page::update(&mut self.speech, input);
        if clear_notice {
            self.clear_action_error(ErrorScope::Speech);
        }
        if let Some(notice) = notice {
            self.set_action_error(ErrorScope::Speech, notice_text(notice));
        }
        Task::batch(effects.into_iter().map(run))
    }
}

/// What the test asks for, as an iced task.
fn run(effect: Effect) -> Task<cosmic::Action<Message>> {
    match effect {
        Effect::Speak(text) => Task::perform(page::run_speak(text), page_action),
        Effect::Stop => Task::perform(page::run_stop(), |()| cosmic::Action::None),
    }
}

/// What the test tells the user, in this app's words: the daemon's own.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::CouldNotSpeak(e) => e,
    }
}
