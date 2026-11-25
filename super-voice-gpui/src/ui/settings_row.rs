// SPDX-License-Identifier: GPL-3.0-only
//! A settings row: a title, a muted description under it, and the control on
//! the right; or the control under the text when the row is narrower than
//! 480 px. Rows go in a [`Card`](super::card::Card) or a kit `GroupBox`.
//!
//! ```ignore
//! SettingsRow::new(t("sleep-when-idle")).description(t("sleep-when-idle-description")).control(switch)
//! ```

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, FontWeight, IntoElement, ParentElement as _, Pixels, RenderOnce, SharedString,
    Styled as _, Window, div,
};

use super::width;

/// One row of a settings group.
#[derive(IntoElement)]
pub struct SettingsRow {
    title: SharedString,
    heading: Option<AnyElement>,
    description: Option<SharedString>,
    control: Option<AnyElement>,
    width: Option<Pixels>,
    inline: bool,
}

impl SettingsRow {
    #[must_use]
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            heading: None,
            description: None,
            control: None,
            width: None,
            inline: false,
        }
    }

    /// Draw this in the title's place: the title with a status pill beside
    /// it.
    #[must_use]
    pub fn heading(mut self, heading: impl IntoElement) -> Self {
        self.heading = Some(heading.into_any_element());
        self
    }

    /// The muted line under the title.
    #[must_use]
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// The control: a Switch, a Select, a Button.
    #[must_use]
    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }

    /// How wide the row is, when it isn't the page's content less its
    /// padding: in a sheet, or one of two columns.
    #[must_use]
    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    /// Keep the control beside the text however narrow the row: for a Switch
    /// or a short Select, which fit.
    #[must_use]
    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }
}

impl RenderOnce for SettingsRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let row = self
            .width
            .unwrap_or_else(|| width::content(window) - width::page_padding(window) * 2.);
        let stacked = !self.inline && width::row_stacks(row);
        let text = v_flex()
            .flex_1()
            .min_w_0()
            .gap_0p5()
            .child(div().text_sm().font_weight(FontWeight::MEDIUM).map(
                |title| match self.heading {
                    Some(heading) => title.child(heading),
                    None => title.child(self.title),
                },
            ))
            .when_some(self.description, |text, description| {
                text.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                )
            });
        let row = if stacked {
            v_flex()
                .gap_2()
                .child(text)
                .when_some(self.control, |row, control| {
                    row.child(div().w_full().child(control))
                })
                .into_any_element()
        } else {
            h_flex()
                .gap_4()
                .items_center()
                .child(text)
                .when_some(self.control, |row, control| {
                    row.child(div().flex_shrink_0().child(control))
                })
                .into_any_element()
        };
        div().w_full().py_3().child(row)
    }
}
