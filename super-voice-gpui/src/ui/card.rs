// SPDX-License-Identifier: GPL-3.0-only
//! A card: a bordered surface with a header (title, a status pill, actions on
//! the right) over its body. A Models step, a group of settings rows, a saved
//! voice. `dashed` draws it as a place nothing fills yet.
//!
//! ```ignore
//! Card::new().title(t("transcription")).status(pill).action(configure).child(fields)
//! ```

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled as _,
    Window, div,
};

/// A card.
#[derive(IntoElement, Default)]
pub struct Card {
    title: Option<SharedString>,
    status: Option<AnyElement>,
    actions: Vec<AnyElement>,
    children: Vec<AnyElement>,
    dashed: bool,
}

impl Card {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The card's name, at the start of its header.
    #[must_use]
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// A status beside the title: a [`StatusPill`](super::status::StatusPill).
    #[must_use]
    pub fn status(mut self, status: impl IntoElement) -> Self {
        self.status = Some(status.into_any_element());
        self
    }

    /// A control at the end of the header, after any before it.
    #[must_use]
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    /// Draw the border dashed: a place nothing fills yet.
    #[must_use]
    pub fn dashed(mut self) -> Self {
        self.dashed = true;
        self
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Card {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let header = self.title.is_some() || self.status.is_some() || !self.actions.is_empty();
        v_flex()
            .w_full()
            .gap_3()
            .p_4()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .when(self.dashed, gpui_kit::Styled::border_dashed)
            .bg(theme.background)
            .when(header, |card| {
                card.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .min_w_0()
                        .when_some(self.title, |row, title| {
                            row.child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .whitespace_nowrap()
                                    .child(title),
                            )
                        })
                        .children(self.status)
                        .child(div().flex_1())
                        .children(self.actions),
                )
            })
            .children(self.children)
    }
}
