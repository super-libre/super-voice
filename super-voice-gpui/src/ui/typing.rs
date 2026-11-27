// SPDX-License-Identifier: GPL-3.0-only
//! How to let the daemon type into other apps on macOS: why it can't, the
//! two steps (open the pane; switch the daemon on, or add it by its path),
//! and the note under them. The Typed card on Speech to text › Models and
//! the Finish setup page both draw it, from the window's [`Setup`].
//!
//! ```ignore
//! v_flex().child(typing::why(cx)).child(typing::steps(&shared.setup, compact, cx)).child(typing::footer(card, cx))
//! ```

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Entity, FontWeight, IntoElement, ParentElement as _, SharedString,
    Styled as _, div, px,
};
use super_voice_app_core::pages::setup::{Input, TypedCard};

use crate::setup::Setup;
use crate::{t, t_with};

/// Why the daemon can't type, and where the words go meanwhile.
pub fn why(cx: &App) -> AnyElement {
    div()
        .text_sm()
        .text_color(cx.theme().foreground)
        .child(t("typing-not-allowed"))
        .into_any_element()
}

/// The two steps. `compact` (a narrow window) uses small buttons and lets the
/// path wrap.
pub fn steps(setup: &Entity<Setup>, compact: bool, cx: &App) -> AnyElement {
    let state = &setup.read(cx).state;
    let pane = t(state.pane.key());
    let open = {
        let setup = setup.clone();
        Button::new("open-typing-pane")
            .primary()
            .icon(IconName::ExternalLink)
            .label(t_with("typing-open-pane-button", &[("pane", &pane)]))
            .when(compact, Sizable::small)
            .on_click(move |_, _, cx| {
                setup.update(cx, |setup, cx| setup.send(Input::OpenPane, cx));
            })
    };
    let copy = {
        let setup = setup.clone();
        let (icon, label) = if state.copied {
            (IconName::Check, t("copied"))
        } else {
            (IconName::Copy, t("copy"))
        };
        Button::new("copy-daemon-path")
            .icon(icon)
            .label(label)
            .when(compact, Sizable::small)
            .on_click(move |_, _, cx| {
                setup.update(cx, |setup, cx| setup.send(Input::CopyPath, cx));
            })
    };
    let theme = cx.theme();
    let path = div()
        .flex_1()
        .min_w_0()
        .px_2p5()
        .py_1p5()
        .rounded(theme.radius)
        .bg(theme.muted)
        .font_family(theme.mono_font_family.clone())
        .map(|path| {
            if compact {
                path.text_xs()
            } else {
                path.text_sm()
            }
        })
        .child(SharedString::from(state.daemon_path.clone()));
    v_flex()
        .gap_3p5()
        .child(step(
            1,
            v_flex()
                .gap_2()
                .child(t_with("typing-open-pane", &[("pane", &pane)]))
                .child(h_flex().child(open)),
            cx,
        ))
        .child(step(
            2,
            v_flex()
                .gap_2()
                .child(t("typing-switch-on"))
                .child(h_flex().gap_1p5().items_start().child(path).child(copy)),
            cx,
        ))
        .into_any_element()
}

/// One numbered step: its number in a muted circle, then `body`.
fn step(number: u8, body: impl IntoElement, cx: &App) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .items_start()
        .gap_2p5()
        .text_sm()
        .child(
            div()
                .flex_shrink_0()
                .size(px(22.))
                .rounded_full()
                .bg(theme.muted)
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .child(number.to_string()),
        )
        .child(div().flex_1().min_w_0().child(body))
        .into_any_element()
}

/// The line under the steps, behind a rule: what to try if the switch is on
/// and nothing types, or, after Open Device Control, that the app is
/// checking.
pub fn footer(card: TypedCard, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (sign, words) = if card == TypedCard::Checking {
        (
            Spinner::new()
                .small()
                .color(theme.muted_foreground)
                .into_any_element(),
            t("typing-checking"),
        )
    } else {
        (
            Icon::new(IconName::Info)
                .size_3p5()
                .flex_shrink_0()
                .into_any_element(),
            t("typing-readd"),
        )
    };
    h_flex()
        .pt_2p5()
        .gap_2()
        .items_center()
        .border_t_1()
        .border_color(theme.border)
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(sign)
        .child(div().flex_1().min_w_0().child(words))
        .into_any_element()
}

/// What the Typed card says once typing is allowed: a green check, and that
/// it can type now.
pub fn allowed(cx: &App) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .gap_2()
        .items_start()
        .text_sm()
        .child(
            Icon::new(IconName::CircleCheck)
                .size_4()
                .flex_shrink_0()
                .text_color(theme.success),
        )
        .child(t("typing-allowed"))
        .into_any_element()
}
