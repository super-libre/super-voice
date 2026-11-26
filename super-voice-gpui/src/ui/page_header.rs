// SPDX-License-Identifier: GPL-3.0-only
//! A page's header: where it is (its feature, then its name), its title, a
//! line about it, and its actions on the right.
//!
//! The description sits beside the title in a wide window when it fits there
//! on one line, and under the title otherwise, as in a compact window. In a
//! narrow window the title bar names the page, so the header keeps only its
//! actions.
//!
//! ```ignore
//! PageHeader::new(Page::SttModels).description(t("models-description")).action(button)
//! ```

use gpui_kit::component::breadcrumb::Breadcrumb;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{
    AnyElement, App, FontWeight, IntoElement, ParentElement as _, Pixels, RenderOnce, SharedString,
    Styled as _, Window, canvas, div, px, rems,
};

use super::width::{self, WindowSize};
use crate::pages::Page;
use crate::t;

/// What a header action is taken to need before it is first drawn.
const ACTION_GUESS: f32 = 200.;

/// The gaps between the title, the description and the actions, with some
/// room to spare.
const GAPS: Pixels = px(40.);

/// The breadcrumb's words, with room for its chevron.
fn crumbs(page: Page) -> SharedString {
    format!("{} > {}", t(page.section().key()), t(page.key())).into()
}

/// How wide `text` is on one line, at `size` rems and `weight`, in the
/// window's font.
fn text_width(text: &str, size: f32, weight: FontWeight, window: &Window) -> Pixels {
    let style = window.text_style();
    let mut run = style.to_run(text.len());
    run.font.weight = weight;
    let font_size = rems(size).to_pixels(window.rem_size());
    window
        .text_system()
        .shape_line(
            SharedString::from(text.to_string()),
            font_size,
            &[run],
            None,
        )
        .width
}

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
        // How wide the actions were drawn last: what the description
        // beside the title leaves them. Before the first frame, a button's
        // width each.
        let drawn = window
            .use_keyed_state(("page-header-actions", self.page as usize), cx, |_, _| {
                Rc::new(Cell::new(None::<Pixels>))
            })
            .read(cx)
            .clone();
        let actions_width = if self.actions.is_empty() {
            px(0.)
        } else {
            #[allow(clippy::cast_precision_loss)]
            let guess = px(ACTION_GUESS * self.actions.len() as f32);
            drawn.get().unwrap_or(guess)
        };
        let actions = (!self.actions.is_empty()).then(|| {
            let drawn = drawn.clone();
            h_flex()
                .relative()
                .flex_shrink_0()
                .gap_2()
                .items_center()
                .children(self.actions)
                .child(
                    canvas(
                        move |bounds, _, _| drawn.set(Some(bounds.size.width)),
                        |_, (), _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
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
        // Beside the title only when it fits there on one line: wrapped
        // beside it, it would start on the breadcrumb's line.
        let beside = size == WindowSize::Wide
            && self.description.as_ref().is_some_and(|text| {
                let title =
                    text_width(&t(self.page.key()), 1.25, FontWeight::SEMIBOLD, window).max(
                        text_width(&crumbs(self.page), 0.875, FontWeight::NORMAL, window),
                    );
                let room = width::content(window) - padding * 2. - title - actions_width - GAPS;
                text_width(text, 0.875, FontWeight::NORMAL, window) <= room
            });
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
