// SPDX-License-Identifier: GPL-3.0-only
//! Common components and utilities shared across views.

use cosmic::iced::widget::row;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, text};
use cosmic::{Apply, Element};

use crate::ui::icons;
use crate::ui::messages::Message;

/// How wide a test's level meter is, and the Volume slider: one width, so the
/// bars on speech to text's Behavior tab line up.
///
/// Fixed, not filling: each meter sits in a settings row's control, which
/// shrinks to its content, and a bar that fills that has nothing to fill. It
/// drew at no width at all.
pub const LEVEL_METER_WIDTH: f32 = 250.0;

/// A scope-tagged action-error banner: a destructive warning glyph plus the
/// message. Shared by the pages that render `AppModel::action_error` (the
/// Customization page and the Configure sheet) so a failed save surfaces
/// inline instead of hijacking the UI. Mirrors the Models card's `card_error`.
pub fn error_banner(message: &str) -> Element<'static, Message> {
    row![
        icons::phosphor_destructive(icons::WARNING, 18.0),
        text::body(message.to_string()),
    ]
    .spacing(cosmic::theme::spacing().space_xs)
    .align_y(Alignment::Center)
    .into()
}

/// A round dot of `size` in the theme color `color` picks: the indicator a
/// state is drawn with, beside the words or glyph that name it, on a neutral
/// surface. A tinted surface reads as a button.
pub fn status_dot<'a>(
    color: fn(&cosmic::Theme) -> cosmic::iced::Color,
    size: f32,
) -> Element<'a, Message> {
    widget::container(widget::text(""))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .class(cosmic::theme::Container::custom(move |theme| {
            cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Background::Color(color(theme))),
                border: cosmic::iced::Border {
                    radius: (size / 2.0).into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }))
        .into()
}

/// Dim a secondary line so it reads as a caption rather than as body text.
pub fn muted<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    widget::container(content.into())
        .class(cosmic::theme::Container::custom(|theme| {
            let mut color: cosmic::iced::Color = theme.current_container().component.on.into();
            color.a *= 0.7;
            cosmic::iced::widget::container::Style {
                text_color: Some(color),
                ..Default::default()
            }
        }))
        .into()
}

/// Create a page container following cosmic-settings patterns
pub fn page_container<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let theme = cosmic::theme::active();
    let padding = theme.cosmic().space_l();
    let bottom_spacer = theme.cosmic().space_m();

    widget::container(content.into())
        .max_width(800)
        .width(Length::Fill)
        .apply(widget::container)
        .center_x(Length::Fill)
        .padding([0, padding, bottom_spacer, padding])
        .into()
}

/// Create page title header
pub fn page_header<'a>(title: impl Into<std::borrow::Cow<'a, str>> + 'a) -> Element<'a, Message> {
    page_container(text::title3(title))
}

/// Create scrollable page content
pub fn page_content<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    page_container(content.into())
        .apply(widget::scrollable)
        .height(Length::Fill)
        .into()
}

/// Create a standard two-part page layout (header + scrollable content)
pub fn page_layout<'a>(
    title: impl Into<std::borrow::Cow<'a, str>> + 'a,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    widget::column::with_capacity(2)
        .push(page_header(title))
        .push(page_content(content))
        .height(Length::Fill)
        .into()
}

/// A feature section's layout: its title, its tabs, then the open tab's
/// scrollable content. The tabs stay put while the content scrolls.
pub fn tabbed_layout<'a>(
    title: impl Into<std::borrow::Cow<'a, str>> + 'a,
    tabs: Element<'a, Message>,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    widget::column::with_capacity(3)
        .push(page_header(title))
        .push(page_container(tabs))
        .push(page_content(content))
        .height(Length::Fill)
        .into()
}
