// SPDX-License-Identifier: GPL-3.0-only
//! The title bar: the kit's `TitleBar`, which draws the window's own buttons
//! on Linux (client decorations) and leaves room for the traffic lights on
//! macOS.
//!
//! - Wide: the app's icon and name.
//! - Compact: the same, and "Go to or run…" on the right, which the rail has
//!   no room for.
//! - Narrow: the drawer's button, the page's name (the page drops its own
//!   header), and a search button for the palette.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, TitleBar, h_flex,
};
use gpui_kit::{Context, FontWeight, IntoElement, ParentElement as _, Styled as _, div, img, px};

use super::Shell;
use super::sidebar::palette_field;
use crate::assets::app_icon;
use crate::t;
use crate::ui::width::WindowSize;

pub fn render(shell: &Shell, size: WindowSize, cx: &mut Context<Shell>) -> impl IntoElement {
    let left = if size == WindowSize::Narrow {
        narrow_title(shell, cx).into_any_element()
    } else {
        h_flex()
            .gap_2()
            .items_center()
            .child(img(app_icon(px(16.))).size_4())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(t("app-title")),
            )
            .into_any_element()
    };
    let weak = cx.weak_entity();
    let right = match size {
        WindowSize::Wide => None,
        WindowSize::Compact => Some(
            div()
                .w(px(240.))
                .child(palette_field("title-palette", weak, cx).small())
                .into_any_element(),
        ),
        WindowSize::Narrow => Some(
            Button::new("title-search")
                .ghost()
                .small()
                .icon(IconName::Search)
                .tooltip(t("go-to-or-run"))
                .on_click(move |_, window, cx| {
                    _ = weak.update(cx, |shell, cx| shell.open_palette(window, cx));
                })
                .into_any_element(),
        ),
    };
    // The close button asks the pages first: one with work in flight holds
    // the window until it is done (`Shell::close`).
    let shell = cx.weak_entity();
    TitleBar::new()
        .on_close_window(move |_, window, cx| {
            let close = shell
                .update(cx, |shell, cx| shell.close(window, cx))
                .unwrap_or(true);
            if close {
                window.remove_window();
            }
        })
        .child(left)
        .child(h_flex().pr_1().gap_1().items_center().children(right))
}

/// The drawer's button and the page's name: "Speech to text › Notes", "General › Settings", or
/// "Super Voice" while the Connection page shows.
fn narrow_title(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    let open = shell.drawer_open;
    let weak = cx.weak_entity();
    let muted = cx.theme().muted_foreground;
    let toggle = Button::new("drawer")
        .ghost()
        .small()
        .selected(open)
        .icon(if open {
            IconName::PanelLeftClose
        } else {
            IconName::PanelLeftOpen
        })
        .tooltip(t(if open { "hide-pages" } else { "show-pages" }))
        .on_click(move |_, window, cx| {
            _ = weak.update(cx, |shell, cx| shell.toggle_drawer(window, cx));
        });
    let name = match shell.page_shown(cx) {
        None => h_flex().child(t("app-title")),
        Some(page) => h_flex()
            .gap_1()
            .items_center()
            .min_w_0()
            .child(
                div()
                    .text_color(muted)
                    .whitespace_nowrap()
                    .child(t(page.section().key())),
            )
            .child(Icon::new(IconName::ChevronRight).size_3().text_color(muted))
            .child(div().truncate().child(t(page.key()))),
    };
    h_flex()
        .gap_1()
        .items_center()
        .min_w_0()
        .child(toggle)
        .child(name.text_sm().font_weight(FontWeight::MEDIUM).min_w_0())
}
