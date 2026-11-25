// SPDX-License-Identifier: GPL-3.0-only
//! The Shortcuts page: each of Super Voice's keyboard shortcuts, where it
//! stands on this desktop, and a button to add or remove it. The daemon owns
//! the shortcuts; the page shows what it answers. The installer adds none: it
//! says to come here.

use cosmic::Element;
use cosmic::iced::Alignment;
use cosmic::iced::widget::{Space, row};
use cosmic::widget::{button, settings, text};
use super_voice_app_core::pages::shortcuts::{self as page, ChangedIn, Input, Row};
use super_voice_shared::models::shortcuts::{Shortcut, ShortcutId, ShortcutList};

use super::common::{error_banner, page_layout};
use crate::state::shortcuts::ShortcutsState;
use crate::ui::messages::{Message, ShortcutsMessage};

/// The shortcut's name on the page, and what it does.
fn name(id: ShortcutId) -> (String, String) {
    match id {
        ShortcutId::Dictate => (
            crate::fl!("shortcut-dictate"),
            crate::fl!("shortcut-dictate-description"),
        ),
        ShortcutId::ReadAloud => (
            crate::fl!("shortcut-read-aloud"),
            crate::fl!("shortcut-read-aloud-description"),
        ),
    }
}

/// One shortcut's row: what it does and where it stands, with its keys once
/// it is set up and a Remove button where the daemon can remove it, or the
/// button that adds it.
fn row_for(shortcut: &Shortcut, list: &ShortcutList) -> Element<'static, Message> {
    let (title, does) = name(shortcut.id);
    let item = settings::item::builder(title);
    let id = shortcut.id;
    match page::row(shortcut, list) {
        // The daemon removes it from COSMIC's file, has KDE Plasma's
        // shortcut service forget it, and takes it out of GNOME's record.
        Row::Removable { keys, changed_in } => {
            let set_up = match changed_in {
                ChangedIn::Cosmic => crate::fl!("shortcut-set-up"),
                ChangedIn::Gnome => crate::fl!("shortcut-set-up-gnome"),
                ChangedIn::Desktop => crate::fl!("shortcut-set-up-desktop"),
            };
            item.description(format!("{does} {set_up}"))
                .control(
                    row![
                        text::body(keys),
                        button::standard(crate::fl!("remove"))
                            .on_press(page_input(Input::Remove(id))),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .into()
        }
        // Through the portal elsewhere the daemon cannot: the portal has no
        // unbind, and the desktop keeps it in its own settings.
        Row::Added { keys } => item
            .description(format!(
                "{does} {}",
                crate::fl!("shortcut-change-in-desktop")
            ))
            .control(
                row![
                    text::body(keys),
                    // No `on_press`: shown, grayed out, as where it stands.
                    button::standard(crate::fl!("added")),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .into(),
        Row::Missing { keys } => {
            let state = crate::fl!("shortcut-missing", keys = keys);
            item.description(format!("{does} {state}"))
                .control(
                    button::standard(crate::fl!("add-shortcut"))
                        .on_press(page_input(Input::Add(id))),
                )
                .into()
        }
        Row::Taken { keys, command } => {
            let state = crate::fl!("shortcut-taken", keys = keys, command = command);
            item.description(format!("{does} {state}"))
                .control(Space::new())
                .into()
        }
        Row::Elsewhere { command } => {
            let state = crate::fl!("shortcut-elsewhere", command = command);
            item.description(format!("{does} {state}"))
                .control(Space::new())
                .into()
        }
    }
}

fn page_input(input: Input) -> Message {
    Message::Shortcuts(ShortcutsMessage::Page(input))
}

pub fn page<'a>(state: &ShortcutsState, action_error: Option<&'a str>) -> Element<'a, Message> {
    let mut blocks = Vec::new();
    if let Some(message) = action_error {
        blocks.push(error_banner(message));
    }
    if let Some(list) = &state.list {
        let mut section = settings::section().title(crate::fl!("keyboard-shortcuts"));
        for shortcut in &list.shortcuts {
            section = section.add(row_for(shortcut, list));
        }
        blocks.push(section.into());
    }
    page_layout(crate::fl!("shortcuts"), settings::view_column(blocks))
}
