// SPDX-License-Identifier: GPL-3.0-only
//! An empty state: an icon, a title, a line of what to do, and its actions.
//! No notes yet, nothing matches, the daemon isn't running. The kit's `Empty`,
//! filled in the one way the app uses it.
//!
//! ```ignore
//! EmptyState::new(Phosphor::Tray, t("no-notes-yet")).description(t("no-notes-help")).action(record)
//! ```

use gpui_kit::component::empty::{
    Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window,
};

/// An empty state.
#[derive(IntoElement)]
pub struct EmptyState {
    icon: Icon,
    title: SharedString,
    description: Option<SharedString>,
    actions: Vec<AnyElement>,
}

impl EmptyState {
    #[must_use]
    pub fn new(icon: impl Into<Icon>, title: impl Into<SharedString>) -> Self {
        Self {
            icon: icon.into(),
            title: title.into(),
            description: None,
            actions: Vec::new(),
        }
    }

    /// What it means, or what to do: a sentence or two.
    #[must_use]
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// A button under the words, after any before it.
    #[must_use]
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let header = EmptyHeader::new()
            .media(
                EmptyMedia::new()
                    .with_variant(EmptyMediaVariant::Icon)
                    .child(self.icon),
            )
            .title(EmptyTitle::new().child(self.title))
            .when_some(self.description, |header, description| {
                header.description(EmptyDescription::new().child(description))
            });
        Empty::new()
            .header(header)
            .when(!self.actions.is_empty(), |empty| {
                empty.content(
                    EmptyContent::new()
                        .child(h_flex().gap_2().justify_center().children(self.actions)),
                )
            })
    }
}
