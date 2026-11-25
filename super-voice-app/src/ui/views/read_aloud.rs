// SPDX-License-Identifier: GPL-3.0-only
//! Read aloud, on text to speech's Behavior tab: where the read-aloud shortcut
//! reads from, and a test that reads it now. Text to speech's counterpart to
//! Typing, which holds the write method and its typing test.

use cosmic::Element;
use cosmic::widget::{self, button, settings};
use super_voice_shared::models::read_method::ReadMethod;

use crate::state::SpeakingStatus;
use crate::ui::messages::{Message, ReadMethodMessage};
use super_voice_app_core::pages::read_aloud as page;

/// Read aloud section: where the shortcut reads from.
pub(super) fn settings_section(read_method: ReadMethod) -> Element<'static, Message> {
    let methods = ReadMethod::ALL;
    let names: Vec<String> = methods
        .iter()
        .map(|m| crate::i18n::read_method_name(*m))
        .collect();
    let selected = methods.iter().position(|m| *m == read_method);
    settings::section()
        .title(crate::fl!("read-aloud-section"))
        .add(
            settings::item::builder(crate::fl!("read-from"))
                .description(crate::fl!("read-from-description"))
                .control(widget::dropdown(names, selected, move |index| {
                    Message::ReadMethod(ReadMethodMessage::Page(page::Input::Pick(methods[index])))
                })),
        )
        .into()
}

/// The read test's row, for the Behavior tab's one Test section: reads what
/// the setting names now, the way the shortcut does. While anything is
/// speaking the button waits: the speech test's row above has the Stop.
pub(super) fn test_row(
    read_method: ReadMethod,
    speaking_status: &SpeakingStatus,
) -> Element<'static, Message> {
    let idle = matches!(speaking_status, SpeakingStatus::Idle);
    let action = button::suggested(crate::fl!("read")).on_press_maybe(idle.then_some(
        Message::ReadMethod(ReadMethodMessage::Page(page::Input::Test)),
    ));
    let hint = match read_method {
        ReadMethod::Selection => crate::fl!("read-test-selection"),
        ReadMethod::Clipboard => crate::fl!("read-test-clipboard"),
    };
    settings::item::builder(crate::fl!("read-test"))
        .description(hint)
        .control(action)
        .into()
}
