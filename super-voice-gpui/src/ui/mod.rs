// SPDX-License-Identifier: GPL-3.0-only
//! The building blocks every page draws with, on top of the kit's
//! components: one small module each, with a one-line example at its top.
//!
//! - [`width`]: the window's and the content's width, and the rules that
//!   follow from them (two panes, the inspector, a card's fields, a row).
//! - [`list_detail::ListDetail`]: a list and its detail, side by side or one
//!   at a time with a back button.
//! - [`status::StatusPill`] and [`status::StatusDot`]: status, as a dot on a
//!   neutral pill.
//! - [`settings_row::SettingsRow`]: title, description and control.
//! - [`card::Card`]: a bordered surface with a header.
//! - [`empty_state::EmptyState`]: nothing here yet, and what to do.
//! - [`page_header::PageHeader`]: where the page is, its title and actions.
//! - [`toast`]: the core's notices, as toasts.
//! - [`code_line`]: a command to copy, in the monospace font.
//! - [`waveform::Waveform`]: the daemon's microphone, live.

// The blocks are for the pages, and a page built later is the first to use
// some of them.
#![allow(dead_code)]

pub mod card;
pub mod empty_state;
pub mod list_detail;
pub mod page_header;
pub mod settings_row;
pub mod status;
pub mod toast;
pub mod waveform;
pub mod width;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, IntoElement, ParentElement as _, SharedString, Styled as _, div};

/// A line of code to read or copy, such as a command: monospace, on a muted
/// surface.
///
/// `ui::code_line("systemctl --user start super-voice".into(), cx)`
pub fn code_line(code: SharedString, cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    div()
        .flex_1()
        .min_w_0()
        .px_3()
        .py_1p5()
        .rounded(theme.radius)
        .bg(theme.muted)
        .font_family(theme.mono_font_family.clone())
        .text_sm()
        .overflow_hidden()
        .child(code)
}
