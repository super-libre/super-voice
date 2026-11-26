// SPDX-License-Identifier: GPL-3.0-only
//! Finish setup: a checklist of what macOS has to allow, first in the
//! sidebar while typing into other apps isn't allowed, which is how someone
//! opening the app for the first time finds it. The app opens on it then.
//!
//! Typing is required: its item says why and has the two steps to allow it,
//! and folds to one line once allowed. Notifications are optional: their
//! item says whether macOS shows them and opens their settings. The page
//! leaves the sidebar when typing is allowed and another page is shown; an
//! optional item alone never keeps it there.
//!
//! The logic is the core's (`pages::setup`), held by the window's
//! [`crate::setup::Setup`]; whether the daemon may type is the readiness's.

use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use super_voice_app_core::pages::setup::{self, Input, TypedCard};

use crate::icons::Phosphor;
use crate::pages::{Page, PageView, SidebarMark};
use crate::shared::Shared;
use crate::t;
use crate::ui::page_header::PageHeader;
use crate::ui::status::{Dot, StatusPill};
use crate::ui::typing;
use crate::ui::width::{self, WindowSize};

/// The widest the checklist grows.
const LIST_MAX: f32 = 820.;

pub struct FinishSetupPage {
    shared: Shared,
}

impl PageView for FinishSetupPage {
    fn new(_: Page, shared: &Shared, _: &mut Window, cx: &mut Context<Self>) -> Self {
        // Drawn from the setup and the readiness, so it follows both.
        cx.observe(&shared.setup, |_, _, cx| cx.notify()).detach();
        cx.observe(&shared.readiness, |_, _, cx| cx.notify())
            .detach();
        Self {
            shared: shared.clone(),
        }
    }

    fn sidebar_mark(&self, cx: &App) -> Option<SidebarMark> {
        self.shared
            .readiness
            .read(cx)
            .typing_blocked()
            .then_some(SidebarMark::NeedsSetup)
    }
}

impl FinishSetupPage {
    /// Typing into other apps: the steps while it isn't allowed, one line
    /// once it is.
    fn typing(&self, blocked: bool, compact: bool, cx: &App) -> AnyElement {
        let card = setup::typed_card(&self.shared.setup.read(cx).state);
        if !blocked {
            return item(
                Phosphor::Keyboard,
                t("type-into-other-apps"),
                None,
                StatusPill::new(Dot::Green, t("status-on")),
                vec![muted(t("typing-allowed"), cx)],
                cx,
            );
        }
        // Checking, as the card would say, once Open was pressed.
        let footer = if card == TypedCard::Checking {
            TypedCard::Checking
        } else {
            TypedCard::NeedsSetup
        };
        item(
            Phosphor::Keyboard,
            t("type-into-other-apps"),
            None,
            StatusPill::new(Dot::Yellow, t("status-needs-setup")),
            vec![
                muted(t("typing-not-allowed"), cx),
                div()
                    .pt_1p5()
                    .child(typing::steps(&self.shared.setup, compact, cx))
                    .into_any_element(),
                div()
                    .pt_1p5()
                    .child(typing::footer(footer, cx))
                    .into_any_element(),
            ],
            cx,
        )
    }

    /// Notifications, optional: whether macOS shows them, and a way to
    /// switch them on. Not drawn where the app can't tell.
    fn notifications(&self, cx: &App) -> Option<AnyElement> {
        let on = self.shared.setup.read(cx).state.notifications?;
        // Off is a choice, not a fault: a grey dot.
        let (dot, state, words) = if on {
            (Dot::Green, t("status-on"), t("macos-notifications-on"))
        } else {
            (Dot::Grey, t("status-off"), t("macos-notifications-off"))
        };
        let setup = self.shared.setup.clone();
        let mut body = vec![muted(words, cx)];
        if !on {
            body.push(
                h_flex()
                    .pt_1()
                    .child(
                        Button::new("open-notifications")
                            .small()
                            .icon(Phosphor::ArrowSquareOut)
                            .label(t("open-notifications"))
                            .on_click(move |_, _, cx| {
                                setup.update(cx, |setup, cx| {
                                    setup.send(Input::OpenNotifications, cx);
                                });
                            }),
                    )
                    .into_any_element(),
            );
        }
        Some(item(
            if on {
                Phosphor::Bell
            } else {
                Phosphor::BellSlash
            },
            t("notifications"),
            Some(t("optional")),
            StatusPill::new(dot, state),
            body,
            cx,
        ))
    }
}

/// One item of the checklist: its icon on a tile, its name, a muted word
/// beside it (Optional), its status on the right, and what follows under.
fn item(
    icon: Phosphor,
    title: gpui_kit::SharedString,
    aside: Option<gpui_kit::SharedString>,
    status: StatusPill,
    body: Vec<AnyElement>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let icon_tile = div()
        .flex_shrink_0()
        .size_8()
        .rounded(theme.radius_lg)
        .bg(theme.muted)
        .flex()
        .items_center()
        .justify_center()
        .child(Icon::new(icon).size_4());
    let head = h_flex()
        .gap_2()
        .items_center()
        .min_w_0()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_base()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .when_some(aside, |row, aside| {
            row.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(aside),
            )
        })
        .child(status);
    div()
        .w_full()
        .p_4()
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(theme.border)
        .flex()
        .flex_row()
        .gap_3()
        .child(icon_tile)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(head)
                .children(body),
        )
        .into_any_element()
}

/// A muted line of an item.
fn muted(words: gpui_kit::SharedString, cx: &App) -> AnyElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(words)
        .into_any_element()
}

impl Render for FinishSetupPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let padding = width::page_padding(window);
        let compact = WindowSize::of(window) == WindowSize::Narrow;
        let blocked = self.shared.readiness.read(cx).typing_blocked();
        let description = if blocked {
            t("finish-setup-description")
        } else {
            t("finish-setup-done-description")
        };
        v_flex()
            .size_full()
            .child(PageHeader::new(Page::FinishSetup).description(description))
            .child(
                div()
                    .id("finish-setup")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(LIST_MAX))
                            .px(padding)
                            .pt_1()
                            .pb_6()
                            .gap_3p5()
                            .child(self.typing(blocked, compact, cx))
                            .children(self.notifications(cx)),
                    ),
            )
    }
}
