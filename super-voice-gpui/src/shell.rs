// SPDX-License-Identifier: GPL-3.0-only
//! The window: a title bar, the sidebar of pages grouped by feature, the
//! selected page, and a status bar that says whether the daemon answers.
//!
//! Only the frame for now. Each page is a placeholder until it is built on
//! its core page (`super_voice_app_core::pages`).

use gpui_kit::component::breadcrumb::Breadcrumb;
use gpui_kit::component::empty::{Empty, EmptyHeader, EmptyTitle};
use gpui_kit::component::sidebar::{
    Sidebar, SidebarCollapsible, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem,
};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{ActiveTheme as _, TitleBar, h_flex, v_flex};
use gpui_kit::{
    Context, Entity, FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, SharedString, Styled as _, Subscription, Window, div, img, px,
};
use super_voice_app_core::state::connection::DaemonStatus;

use crate::assets::APP_ICON;
use crate::connection::Connection;
use crate::pages::{Page, Section};
use crate::{t, t_with};

/// The message keys the shell shows besides the pages' and headings' names.
pub const KEYS: &[&str] = &[
    "app-title",
    "daemon-connected",
    "daemon-connecting",
    "daemon-not-answering",
    "daemon-access-refused",
    "page-not-built",
];

/// The sidebar's width.
const SIDEBAR_WIDTH: f32 = 232.;

/// The window's content.
pub struct Shell {
    page: Page,
    connection: Entity<Connection>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    /// The window, showing `page`, with the daemon's answers from
    /// `connection`. With `follow_system`, light or dark follows the system's
    /// appearance as it changes.
    pub fn new(
        page: Page,
        connection: Entity<Connection>,
        follow_system: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = vec![cx.observe(&connection, |_, _, cx| cx.notify())];
        if follow_system {
            crate::theme::follow_system(window, cx);
            subscriptions.push(cx.observe_window_appearance(window, |_, window, cx| {
                crate::theme::follow_system(window, cx);
            }));
        }
        Self {
            page,
            connection,
            _subscriptions: subscriptions,
        }
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let groups = Section::ALL.map(|section| {
            SidebarGroup::new(t(section.key())).child(SidebarMenu::new().children(
                section.pages().map(|page| {
                    SidebarMenuItem::new(t(page.key()))
                        .icon(page.icon())
                        .active(self.page == page)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.page = page;
                            cx.notify();
                        }))
                }),
            ))
        });
        Sidebar::new("pages")
            .collapsible(SidebarCollapsible::None)
            .w(px(SIDEBAR_WIDTH))
            .header(
                SidebarHeader::new().child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .min_w_0()
                        .child(img(APP_ICON).size_8().flex_shrink_0())
                        .child(
                            v_flex()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(t("app-title")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(version()),
                                ),
                        ),
                ),
            )
            .children(groups)
    }

    fn content(&self, cx: &Context<Self>) -> impl IntoElement {
        let title = t(self.page.key());
        v_flex()
            .id("page")
            .flex_1()
            .min_w_0()
            .h_full()
            .px_6()
            .py_4()
            .gap_1()
            .bg(cx.theme().background)
            .child(
                Breadcrumb::new()
                    .child(t(self.page.section().key()))
                    .child(title.clone()),
            )
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Empty::new().header(
                        EmptyHeader::new().title(EmptyTitle::new().child(t("page-not-built"))),
                    )),
            )
    }

    fn status_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let (dot, words) = daemon_status(self.connection.read(cx).status(), cx);
        StatusBar::new()
            .left(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(div().size_2().rounded_full().bg(dot))
                    .child(words),
            )
            .right(version())
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(t("app-title")),
                ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.sidebar(cx))
                    .child(self.content(cx)),
            )
            .child(self.status_bar(cx))
    }
}

/// The status bar's dot and words for the daemon: green when it answers,
/// grey while asking, red when it doesn't answer or refused the app.
fn daemon_status(status: &DaemonStatus, cx: &Context<Shell>) -> (Hsla, SharedString) {
    let theme = cx.theme();
    match status {
        DaemonStatus::Connected => (theme.success, t("daemon-connected")),
        DaemonStatus::Disconnected | DaemonStatus::Connecting => {
            (theme.muted_foreground, t("daemon-connecting"))
        }
        DaemonStatus::Error(reason) => (
            theme.danger,
            t_with("daemon-not-answering", &[("reason", reason)]),
        ),
        DaemonStatus::Blocked(reason) => (
            theme.danger,
            t_with("daemon-access-refused", &[("reason", reason)]),
        ),
    }
}

/// The app's version, as the sidebar and the status bar show it.
fn version() -> SharedString {
    concat!("v", env!("CARGO_PKG_VERSION")).into()
}
