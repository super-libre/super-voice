// SPDX-License-Identifier: GPL-3.0-only
//! Status, as the app always shows it: a coloured dot, on a neutral pill when
//! there are words beside it. Never a tinted pill, which reads as a button.
//!
//! The colours are the core's rule ([`Dot`]): green ready or idle, yellow no
//! model loaded, red failed, grey no backend.
//!
//! ```ignore
//! StatusPill::new(Dot::Green, t("ready")).icon(IconName::Mic)
//! ```

use gpui_kit::component::{ActiveTheme as _, Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Hsla, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window, div,
    px,
};
pub use super_voice_app_core::rules::status::Dot;

/// The theme's colour for `dot`.
#[must_use]
pub fn dot_color(dot: Dot, cx: &App) -> Hsla {
    let theme = cx.theme();
    match dot {
        Dot::Green => theme.success,
        Dot::Yellow => theme.warning,
        Dot::Red => theme.danger,
        Dot::Grey => theme.muted_foreground.opacity(0.6),
    }
}

/// A bare dot: where the words are elsewhere on screen, such as on the rail,
/// whose status bar says them.
///
/// `StatusDot::new(Dot::Yellow)`
#[derive(IntoElement)]
pub struct StatusDot {
    dot: Dot,
}

impl StatusDot {
    #[must_use]
    pub fn new(dot: Dot) -> Self {
        Self { dot }
    }
}

impl RenderOnce for StatusDot {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .flex_shrink_0()
            .size(px(7.))
            .rounded_full()
            .bg(dot_color(self.dot, cx))
    }
}

/// A dot and its words on a neutral pill, with an optional icon before them.
///
/// `StatusPill::new(Dot::Yellow, t("loading"))`
#[derive(IntoElement)]
pub struct StatusPill {
    dot: Dot,
    label: SharedString,
    icon: Option<Icon>,
}

impl StatusPill {
    #[must_use]
    pub fn new(dot: Dot, label: impl Into<SharedString>) -> Self {
        Self {
            dot,
            label: label.into(),
            icon: None,
        }
    }

    /// An icon before the dot, naming what the status is of.
    #[must_use]
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }
}

impl RenderOnce for StatusPill {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .flex_shrink_0()
            .gap_1p5()
            .items_center()
            .h(px(20.))
            .px_2()
            .rounded_full()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .text_xs()
            .text_color(theme.foreground)
            .whitespace_nowrap()
            .when_some(self.icon, |pill, icon| {
                pill.child(icon.size_3().text_color(theme.muted_foreground))
            })
            .child(StatusDot::new(self.dot))
            .child(self.label)
    }
}
