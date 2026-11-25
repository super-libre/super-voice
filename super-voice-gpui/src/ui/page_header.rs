// SPDX-License-Identifier: GPL-3.0-only
//! A page's header: where it is (its feature, then its name), its title, a
//! line about it, and its actions on the right.
//!
//! The description sits beside the title in a wide window and under it in a
//! compact one. In a narrow window the title bar names the page, so the
//! header keeps only its actions.
//!
//! ```ignore
//! PageHeader::new(Page::SttModels).description(t("models-description")).action(button)
//! ```

use gpui_kit::component::breadcrumb::Breadcrumb;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div,
};

use super::width::{self, WindowSize};
use crate::pages::Page;
use crate::t;

/// A page's header.
#[derive(IntoElement)]
pub struct PageHeader {
    page: Page,
    description: Option<SharedString>,
    actions: Vec<AnyElement>,
}

impl PageHeader {
    #[must_use]
    pub fn new(page: Page) -> Self {
        Self {
            page,
            description: None,
            actions: Vec::new(),
        }
    }

    /// One line about the page.
    #[must_use]
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// A control at the end of the header, after any before it.
    #[must_use]
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl RenderOnce for PageHeader {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let size = WindowSize::of(window);
        let padding = width::page_padding(window);
        let actions = (!self.actions.is_empty()).then(|| {
            h_flex()
                .flex_shrink_0()
                .gap_2()
                .items_center()
                .children(self.actions)
        });
        if size == WindowSize::Narrow {
            return div()
                .when_some(actions, |header, actions| {
                    header
                        .flex()
                        .justify_end()
                        .px(padding)
                        .pt_3()
                        .child(actions)
                })
                .into_any_element();
        }
        let title = v_flex()
            .flex_shrink_0()
            .gap_0p5()
            .child(
                Breadcrumb::new()
                    .child(t(self.page.section().key()))
                    .child(t(self.page.key())),
            )
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t(self.page.key())),
            );
        let description = |text: SharedString| {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text)
        };
        let beside = size == WindowSize::Wide;
        let (inline, under) = if beside {
            (self.description, None)
        } else {
            (None, self.description)
        };
        v_flex()
            .px(padding)
            .pt_4()
            .pb_3()
            .gap_1()
            .child(
                h_flex()
                    .gap_3()
                    .items_end()
                    .child(title)
                    .map(|row| match inline {
                        Some(text) => row.child(description(text).flex_1().min_w_0().pb_0p5()),
                        None => row.child(div().flex_1()),
                    })
                    .children(actions),
            )
            .when_some(under, |header, text| header.child(description(text)))
            .into_any_element()
    }
}
