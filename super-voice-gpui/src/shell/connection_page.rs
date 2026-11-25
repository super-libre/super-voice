// SPDX-License-Identifier: GPL-3.0-only
//! The Connection page: what the content area shows in place of a page that
//! needs the daemon while the daemon can't be reached, or refused the app.
//! It says what happened, what fixes it (the core's `connection::fix`), and
//! the status and socket, for a report.
//!
//! The pages that work without the daemon (Shortcuts, Settings, Updates,
//! About) stay open in the sidebar.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    px,
};
use super_voice_app_core::pages::connection::{Fix, Input, socket_path};
use super_voice_app_core::state::connection::DaemonStatus;

use super::Shell;
use crate::connection::Connection;
use crate::t;
use crate::ui::code_line;
use crate::ui::empty_state::EmptyState;
use crate::ui::width::{self, WindowSize};

/// The column's width, when the content leaves room for it.
const COLUMN: f32 = 520.;

pub fn render(
    connection: &Entity<Connection>,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let narrow = WindowSize::of(window) == WindowSize::Narrow;
    let column = (width::content(window) - px(32.)).min(px(COLUMN));
    let model = connection.read(cx);
    let fix = model.fix();
    let failure = model.failure().cloned();
    let mut body = fix.map_or_else(Vec::new, |fix| steps(fix, connection, narrow, cx));
    let status: SharedString = match &failure {
        Some(DaemonStatus::Blocked(reason) | DaemonStatus::Error(reason)) => reason.clone().into(),
        _ => t("daemon-connecting"),
    };
    let socket = div()
        .font_family(cx.theme().mono_font_family.clone())
        .text_xs()
        .child(socket_path())
        .into_any_element();
    body.push(
        div()
            .pt_2()
            .text_sm()
            .child(
                DescriptionList::new()
                    .small()
                    .bordered(true)
                    .columns(1)
                    .label_width(px(if narrow { 72. } else { 120. }))
                    .item(t("status"), status, 1)
                    .item(t("socket"), socket, 1),
            )
            .into_any_element(),
    );

    div()
        .id("connection")
        .size_full()
        .overflow_y_scroll()
        .child(
            v_flex()
                .min_h_full()
                .w_full()
                .items_center()
                .justify_center()
                .py_6()
                .child(v_flex().w(column).gap_4().children(body)),
        )
}

/// What fixes `fix`: what happened, then each step, with its button.
fn steps(fix: Fix, connection: &Entity<Connection>, narrow: bool, cx: &App) -> Vec<AnyElement> {
    let send = |input: Input| {
        let connection = connection.clone();
        move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| {
            let input = input.clone();
            connection.update(cx, |connection, cx| connection.send(input, cx));
        }
    };
    let mut body: Vec<AnyElement> = Vec::new();
    match fix {
        Fix::Reauthorize { restart } => {
            body.push(
                EmptyState::new(Lucide::ShieldX, t("connection-refused-title"))
                    .description(t("connection-refused-body"))
                    .into_any_element(),
            );
            body.push(step(
                1,
                "connection-restart-the-daemon",
                command(restart, narrow, cx),
                cx,
            ));
            body.push(step(
                2,
                "connection-ask-again",
                Button::new("retry-authorization")
                    .primary()
                    .icon(IconName::RefreshCw)
                    .label(t("retry-authorization"))
                    .on_click(send(Input::RetryAuthorization))
                    .into_any_element(),
                cx,
            ));
        }
        Fix::Start { command: start } => {
            body.push(
                EmptyState::new(Lucide::ServerOff, t("connection-not-running-title"))
                    .description(t("connection-not-running-body"))
                    .into_any_element(),
            );
            body.push(step(
                1,
                "connection-start-the-daemon",
                command(start, narrow, cx),
                cx,
            ));
            body.push(step(
                2,
                "connection-try-now",
                Button::new("retry")
                    .icon(IconName::RefreshCw)
                    .label(t("retry"))
                    .on_click(send(Input::Retry))
                    .into_any_element(),
                cx,
            ));
        }
        Fix::TurnOn => {
            body.push(
                EmptyState::new(Lucide::ServerOff, t("connection-switched-off-title"))
                    .description(t("super-voice-s-background-service-is-switched"))
                    .action(
                        Button::new("open-login-items")
                            .primary()
                            .label(t("open-login-items"))
                            .on_click(send(Input::OpenLoginItems)),
                    )
                    .into_any_element(),
            );
        }
        Fix::Register => {
            body.push(
                EmptyState::new(Lucide::ServerOff, t("connection-not-registered-title"))
                    .description(t("super-voice-s-background-service-is-not"))
                    .action(
                        Button::new("register")
                            .primary()
                            .label(t("register"))
                            .on_click(send(Input::Register)),
                    )
                    .into_any_element(),
            );
        }
    }
    body
}

/// A numbered step: what to do, over what does it.
fn step(number: u8, key: &str, control: AnyElement, cx: &App) -> AnyElement {
    v_flex()
        .gap_1p5()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{number}. {}", t(key))),
        )
        .child(h_flex().child(control))
        .into_any_element()
}

/// A command to copy: the line, and Copy (an icon in a narrow window).
fn command(command: String, narrow: bool, cx: &App) -> AnyElement {
    let copy = if narrow {
        Clipboard::new("copy-command")
            .value(command.clone())
            .tooltip(t("copy-command"))
            .into_any_element()
    } else {
        let text = command.clone();
        Button::new("copy-command")
            .icon(Icon::new(IconName::Copy))
            .label(t("copy"))
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
            })
            .into_any_element()
    };
    h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .child(code_line(command.into(), cx))
        .child(copy)
        .into_any_element()
}
