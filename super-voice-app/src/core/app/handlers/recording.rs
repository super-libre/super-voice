// SPDX-License-Identifier: GPL-3.0-only
//! Speech to text's recording test, wired to its logic in the core
//! (`super_voice_app_core::pages::recording_test`), and the microphone every
//! page that records reads, followed from the daemon's events.

use cosmic::prelude::*;
use futures_util::StreamExt;
use super_voice_app_core::pages::recording_test::{self as page, Effect, Input};

use crate::core::app::AppModel;
use crate::ui::messages::{Message, RecordingMessage};

fn test_action(input: Input) -> cosmic::Action<Message> {
    cosmic::Action::App(Message::Recording(RecordingMessage::Test(input)))
}

impl AppModel {
    pub(in crate::core::app) fn handle_recording_messages(
        &mut self,
        message: RecordingMessage,
    ) -> Task<cosmic::Action<Message>> {
        match message {
            RecordingMessage::Test(input) => self.recording_test_input(input),
            RecordingMessage::Microphone(event) => {
                self.last_udp_data = std::time::Instant::now();
                self.microphone.follow(&event);
                Task::none()
            }
        }
    }

    /// Hand `input` to the test's logic, and do what it asks.
    pub(in crate::core::app) fn recording_test_input(
        &mut self,
        input: Input,
    ) -> Task<cosmic::Action<Message>> {
        let effects = page::update(&mut self.recording_test, &mut self.microphone, input);
        Task::batch(effects.into_iter().map(run))
    }
}

/// What the test asks for, as an iced task.
fn run(effect: Effect) -> Task<cosmic::Action<Message>> {
    match effect {
        Effect::Take => cosmic::task::stream(page::take().map(test_action)),
        Effect::Stop => Task::perform(page::run_stop(), |()| cosmic::Action::None),
    }
}
