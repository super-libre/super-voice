// SPDX-License-Identifier: GPL-3.0-only
//! A list and the detail of what is picked in it: side by side while the
//! content is at least 640 px wide; below, one at a time, the detail with a
//! back button (a ghost `Button` named after the list) over it.
//!
//! The page owns what is picked. Back, and Alt+Left (the shell calls the
//! page's `PageView::back`), drop the pick, which shows the list again; the
//! list keeps its scroll position because the page keeps its list's state.
//!
//! ```ignore
//! ListDetail::new("notes", list).detail(note).back(t("notes"), cx.listener(|this, _, _, cx| this.unpick(cx)))
//! ```

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, IconName, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, RenderOnce, SharedString, Styled as _, Window, div, px,
};

use super::width;
use crate::t_with;

type OnBack = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A list and its detail.
#[derive(IntoElement)]
pub struct ListDetail {
    id: ElementId,
    list: AnyElement,
    detail: Option<AnyElement>,
    placeholder: Option<AnyElement>,
    back: Option<(SharedString, OnBack)>,
    list_width: Pixels,
}

impl ListDetail {
    /// `list` alone, until a [`Self::detail`] is given.
    #[must_use]
    pub fn new(id: impl Into<ElementId>, list: impl IntoElement) -> Self {
        Self {
            id: id.into(),
            list: list.into_any_element(),
            detail: None,
            placeholder: None,
            back: None,
            list_width: px(280.),
        }
    }

    /// What is picked in the list, when something is.
    #[must_use]
    pub fn detail(mut self, detail: Option<impl IntoElement>) -> Self {
        self.detail = detail.map(IntoElement::into_any_element);
        self
    }

    /// What the detail pane shows beside the list while nothing is picked:
    /// an [`EmptyState`](super::empty_state::EmptyState).
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl IntoElement) -> Self {
        self.placeholder = Some(placeholder.into_any_element());
        self
    }

    /// The back button over a detail shown alone: `list_name` is what it goes
    /// back to ("Notes"), `on_back` drops the pick.
    #[must_use]
    pub fn back(
        mut self,
        list_name: impl Into<SharedString>,
        on_back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.back = Some((list_name.into(), Rc::new(on_back)));
        self
    }

    /// The list's width beside the detail; 280 px unless set.
    #[must_use]
    pub fn list_width(mut self, width: Pixels) -> Self {
        self.list_width = width;
        self
    }
}

impl RenderOnce for ListDetail {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let two_panes = width::content(window) >= width::TWO_PANES;
        if two_panes {
            return h_flex()
                .id(self.id)
                .size_full()
                .min_h_0()
                .child(
                    div()
                        .h_full()
                        .w(self.list_width)
                        .flex_shrink_0()
                        .border_r_1()
                        .border_color(cx.theme().border)
                        .child(self.list),
                )
                .child(
                    div()
                        .h_full()
                        .flex_1()
                        .min_w_0()
                        .children(self.detail.or(self.placeholder)),
                )
                .into_any_element();
        }
        let Some(detail) = self.detail else {
            return div()
                .id(self.id)
                .size_full()
                .child(self.list)
                .into_any_element();
        };
        v_flex()
            .id(self.id)
            .size_full()
            .when_some(self.back, |pane, (name, on_back)| {
                let label = t_with("back-to", &[("page", &name)]);
                pane.child(
                    h_flex()
                        .px_2()
                        .py_2()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            Button::new("back")
                                .ghost()
                                .icon(IconName::ArrowLeft)
                                .label(name)
                                .accessibility_label(label)
                                .on_click(move |event, window, cx| on_back(event, window, cx)),
                        ),
                )
            })
            .child(div().flex_1().min_h_0().child(detail))
            .into_any_element()
    }
}
