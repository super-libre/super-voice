// SPDX-License-Identifier: GPL-3.0-only
//! A page that isn't built yet: its header, and a line that says so.
//!
//! The palette's commands of the pages it stands in for (Load a model,
//! Unload all models, New note) go to the page, which is all they can do
//! until the page is built.

use gpui_kit::component::{IconName, v_flex};
use gpui_kit::{App, Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div};

use super_voice_app_core::pages::readiness::Feature;

use crate::pages::{Page, PageView, PaletteCommand};
use crate::shared::Shared;
use crate::ui::empty_state::EmptyState;
use crate::ui::page_header::PageHeader;
use crate::{t, t_with};

pub struct Placeholder {
    page: Page,
    shared: Shared,
}

impl PageView for Placeholder {
    fn new(page: Page, shared: &Shared, _: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            page,
            shared: shared.clone(),
        }
    }

    fn commands(&self, cx: &App) -> Vec<PaletteCommand> {
        let mut commands = Vec::new();
        if let Some(feature) = self.page.feature() {
            let readiness = self.shared.readiness.read(cx);
            if let Some(model) = readiness.model_to_load(feature) {
                let stage = match feature {
                    Feature::SpeechToText => "transcription",
                    Feature::TextToSpeech => "synthesis",
                };
                commands.push(
                    PaletteCommand::new(
                        "load-model",
                        t_with("palette-load-model", &[("model", model)]),
                        IconName::Play,
                    )
                    .detail(format!("{} · {}", t(feature.key()), t(stage))),
                );
            }
        }
        match self.page {
            Page::SttModels => commands.push(PaletteCommand::new(
                "unload-all-models",
                t("unload-all-models"),
                IconName::Square,
            )),
            Page::Notes => {
                commands.push(PaletteCommand::new(
                    "new-note",
                    t("new-note"),
                    IconName::Plus,
                ));
            }
            _ => {}
        }
        commands
    }
}

impl Render for Placeholder {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(PageHeader::new(self.page))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(EmptyState::new(self.page.icon(), t("page-not-built"))),
            )
    }
}
