// SPDX-License-Identifier: GPL-3.0-only
//! Typing, on speech to text's Behavior tab: how transcribed text is typed
//! into the focused window, and the tests that type.
use cosmic::Element;
use cosmic::iced::widget::row;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, Id, button, settings, text};
use super_voice_shared::models::write_method::WriteMethod;

use super_voice_app_core::pages::behavior::Input;

use crate::ui::messages::{BehaviorMessage, Message};

/// Id of the test field, so the Test button can focus it before the daemon
/// types: `POST /write_method/test` types into whatever window holds keyboard
/// focus, and it needs to be this field rather than the button.
#[must_use]
pub fn test_field_id() -> Id {
    Id::new("write_method_test_field")
}

/// Typing section: how transcribed text is typed into the focused window,
/// and, once a test has run, what it actually typed through.
pub(super) fn typing_section(
    write_method: WriteMethod,
    resolved: Option<WriteMethod>,
) -> Element<'static, Message> {
    // Not a fixed list: `xdg_desktop_portal` and `ydotool` are Linux session
    // technologies, and offering a method the daemon has no backend for would
    // be offering a setting that cannot work.
    let methods = WriteMethod::supported_on_this_platform();
    let method_names: Vec<String> = methods
        .iter()
        .map(|m| crate::i18n::write_method_name(*m))
        .collect();
    let selected_index = methods.iter().position(|m| *m == write_method);

    let mut method_section = settings::section().title(crate::fl!("typing")).add(
        settings::item::builder(crate::fl!("write-method"))
            .description(crate::fl!("controls-how-transcribed-text-is-typed-into"))
            .control(widget::dropdown(
                method_names,
                selected_index,
                move |index| {
                    Message::Behavior(BehaviorMessage::Page(Input::WriteMethodChanged(
                        methods[index],
                    )))
                },
            )),
    );

    // "Auto" names a chain, not a backend, so the configured value alone never
    // says what is really typing. A test is the only thing that resolves it.
    if let Some(resolved) = resolved {
        method_section = method_section.add(
            settings::item::builder(crate::fl!("active-backend"))
                .description(crate::fl!("what-the-last-test-actually-typed-through"))
                .control(text::body(crate::i18n::write_method_name(resolved))),
        );
    }
    method_section.into()
}

/// The typing test's rows, for the Behavior tab's one Test section: the
/// in-app typing test, plus the delayed one that targets whatever window the
/// user switches to.
pub(super) fn test_rows(test_text: &str, countdown: Option<u8>) -> Vec<Element<'_, Message>> {
    let test_row = row![
        widget::text_input(crate::fl!("typed-text-lands-here-progress"), test_text)
            .id(test_field_id())
            .on_input(|text| Message::Behavior(BehaviorMessage::Page(
                Input::TypingTestTextChanged(text)
            )))
            .width(Length::Fill),
        button::suggested(crate::fl!("type-test-text"))
            .on_press(Message::Behavior(BehaviorMessage::Page(Input::TestTyping))),
    ]
    .align_y(Alignment::Center)
    .spacing(10);

    // Mid-countdown the only useful action is calling it off: starting a second
    // test would race the first into whichever window won the focus.
    let delayed_control: Element<'_, Message> = if let Some(secs) = countdown {
        button::destructive(crate::fl!(
            "cancel-typing-countdown",
            secs = (secs).to_string()
        ))
        .on_press(Message::Behavior(BehaviorMessage::Page(
            Input::CancelTypingTest,
        )))
        .into()
    } else {
        button::standard(crate::fl!("start-countdown"))
            .on_press(Message::Behavior(BehaviorMessage::Page(
                Input::TestTypingLater,
            )))
            .into()
    };

    vec![
        settings::item::builder(crate::fl!("typing-test"))
            .description(crate::fl!("types-a-short-string-into-the-field"))
            .flex_control(test_row)
            .into(),
        settings::item::builder(crate::fl!("test-another-window"))
            .description(crate::fl!("counts-down-before-typing-so-you-can"))
            .control(delayed_control)
            .into(),
    ]
}
