// SPDX-License-Identifier: GPL-3.0-only
//! The Models page's pipelines: one row of steps per feature.
//!
//! Speech to text runs Microphone › Transcription › Post-processing › Typed,
//! and text to speech runs Text › Synthesis › Speakers. Each stage is a step
//! that says which backend fills it and how its model stands; pressing one
//! opens that stage's card under the rows. The ends of each row are not
//! settings, only where the feature starts and stops, so they are drawn as
//! plain labels rather than as steps. Too narrow for a row, each feature's
//! items stack instead, in the same order.
//!
//! The rows are what tell the two features apart: a stage sits in the row of
//! the feature it belongs to, so a speech-to-text model never reads as a peer
//! of a text-to-speech one.

use cosmic::Element;
use cosmic::iced::widget::{column, row};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, text};
use super_voice_app_core::rules::pipeline::{self, Step, StepModel};

use super::status::{StatusLook, stage_status};
use super::surface::{accent_border_color, muted_text_color};
use crate::core::app::AppModel;
use crate::state::model_operations::ModelOperationState;
use crate::state::{BlockStage, Stage};
use crate::ui::icons;
use crate::ui::messages::{Message, ModelsPageMessage};
use crate::ui::views::common::status_dot;

/// The name a stage goes by on its step and above its card.
pub(crate) fn stage_name(stage: Stage) -> String {
    match stage {
        Stage::Transcription => crate::fl!("transcription"),
        Stage::PostProcessing => crate::fl!("post-processing"),
        Stage::Synthesis => crate::fl!("synthesis"),
    }
}

/// What a step says under its stage's name.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct StepSummary {
    /// The backend that fills the stage, or what the stage is without one.
    pub title: String,
    /// The model and the device it runs on, or why nothing runs.
    pub detail: String,
}

/// A stage with no backend selected.
fn empty(stage: Stage) -> StepSummary {
    match stage {
        // Optional: transcripts are typed as they are without one.
        Stage::PostProcessing => StepSummary {
            title: crate::fl!("off"),
            detail: crate::fl!("optional"),
        },
        Stage::Transcription | Stage::Synthesis => StepSummary {
            title: crate::fl!("none"),
            detail: crate::fl!("select-a-backend"),
        },
    }
}

/// `model · accelerator`, or `model` alone when the accelerator is unknown.
fn running(model: &str, device: Option<&str>) -> String {
    match device {
        Some(device) if !device.is_empty() && device != "none" => {
            format!("{model} · {}", super::fmt::accel_name(device))
        }
        _ => model.to_string(),
    }
}

/// The transcription step: the active backend, and the model the daemon has
/// loaded for it.
///
/// A loaded model only counts when it belongs to the active backend, the same
/// rule the header's status pill applies: the daemon can still report the
/// previous backend's model for a moment after a switch.
pub(super) fn transcription_summary(
    backend: Option<&str>,
    loaded_model: &str,
    loaded_for_backend: bool,
    device: &str,
) -> StepSummary {
    let Some(name) = backend else {
        return empty(Stage::Transcription);
    };
    let detail = if loaded_for_backend && !loaded_model.is_empty() {
        running(loaded_model, Some(device))
    } else {
        crate::fl!("not-loaded")
    };
    StepSummary {
        title: name.to_string(),
        detail,
    }
}

/// A step for a stage the app reads back from the daemon.
pub(super) fn block_summary(
    stage: BlockStage,
    block: &crate::daemon::client::v1::pipeline::StageState,
) -> StepSummary {
    match pipeline::step(stage.stage(), block) {
        Step::Empty => empty(stage.stage()),
        Step::Filled { backend, model } => StepSummary {
            title: backend,
            detail: match model {
                StepModel::Running { name, device } => running(&name, device.as_deref()),
                StepModel::Idle { name } => name,
                StepModel::NotLoaded => crate::fl!("not-loaded"),
            },
        },
    }
}

/// What a download or load in flight says in place of the model, and how far
/// through it is when that is known. `None` when nothing is in flight.
fn operation(op: &ModelOperationState) -> Option<(String, Option<f32>)> {
    match op {
        ModelOperationState::Ready => None,
        ModelOperationState::Provisioning { progress, .. } => {
            let fraction = (progress.percentage / 100.0).clamp(0.0, 1.0);
            let verb = super::active::provisioning_verb(&progress.status);
            Some((format!("{verb} {:.0}%", fraction * 100.0), Some(fraction)))
        }
        ModelOperationState::Loading { load, .. } => {
            let step = super::active::load_step(load.as_ref().and_then(|l| l.step.as_deref()));
            match load.as_ref().and_then(|l| l.progress) {
                Some(fraction) => {
                    let fraction = fraction.clamp(0.0, 1.0);
                    Some((format!("{step} · {:.0}%", fraction * 100.0), Some(fraction)))
                }
                None => Some((step, None)),
            }
        }
        ModelOperationState::Error { .. } => Some((crate::fl!("load-failed"), None)),
    }
}

fn summary(app: &AppModel, stage: Stage) -> StepSummary {
    let view = app.models.view(stage);
    let mut summary = if let Some(block) = stage.block() {
        block_summary(block, view)
    } else {
        let name = view
            .source
            .as_deref()
            .and_then(|source| app.backends.iter().find(|b| b.source == source))
            .map(|b| b.name.as_str());
        let running = super_voice_app_core::pages::models::running_model(&app.models, stage);
        transcription_summary(
            name,
            running.unwrap_or_default(),
            running.is_some(),
            view.running_device().unwrap_or_default(),
        )
    };
    // A model out of memory until its next use runs on nothing yet, so the
    // line is its name alone. The step marks it idle in its corner.
    if let Some(model) = super_voice_app_core::pages::models::idle_model(&app.models, stage) {
        summary.detail = model.to_string();
    }
    summary
}

/// The content width the rows need to stay rows: both ends, three carets and
/// two steps wide enough to read. Narrower, each feature's steps stack.
const ROW_WIDTH: f32 = 600.0;

/// One item of a feature's pipeline.
enum Item {
    /// Where the feature starts or stops: a name and a line about it.
    End(String, String),
    /// A stage the user sets.
    Step(Stage),
}

/// Both features' pipelines: a row each where the page is wide enough, and a
/// column each, joined by down carets, where it is not.
///
/// Measured where the rows are drawn rather than from the window, so the side
/// panel opening on a wide window stacks them too.
pub(super) fn pipelines(app: &AppModel) -> Element<'_, Message> {
    widget::responsive(move |size| {
        let stacked = size.width < ROW_WIDTH;
        let spacing = cosmic::theme::spacing();
        column![
            feature(
                app,
                stacked,
                icons::MICROPHONE,
                crate::fl!("speech-to-text"),
                &[
                    Item::End(crate::fl!("microphone"), crate::fl!("your-voice")),
                    Item::Step(Stage::Transcription),
                    Item::Step(Stage::PostProcessing),
                    Item::End(crate::fl!("typed"), crate::fl!("into-your-app")),
                ],
            ),
            feature(
                app,
                stacked,
                icons::SPEAKER_HIGH,
                crate::fl!("text-to-speech"),
                &[
                    Item::End(crate::fl!("text"), crate::fl!("from-your-app")),
                    Item::Step(Stage::Synthesis),
                    Item::End(crate::fl!("speakers"), crate::fl!("read-aloud")),
                ],
            ),
        ]
        .spacing(spacing.space_m)
        .into()
    })
    .height(Length::Shrink)
    .into()
}

/// One feature: its name over its items, each joined to the next by a caret
/// pointing the way the feature runs.
fn feature<'a>(
    app: &'a AppModel,
    stacked: bool,
    glyph: &'static [u8],
    name: String,
    items: &[Item],
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let selected = app.models_page.pipeline_step;
    let heading = row![
        icons::phosphor(glyph).size(18),
        text::title4(name).line_height(1.0),
    ]
    .spacing(spacing.space_xs)
    .align_y(Alignment::Center);

    let built = items.iter().map(|item| match item {
        Item::End(name, caption) => end(name.clone(), caption.clone(), stacked),
        Item::Step(stage) => step(app, *stage, selected),
    });
    let count = items.len();
    let body: Element<'a, Message> = if stacked {
        let mut steps = column![].spacing(spacing.space_xxs);
        for (index, item) in built.enumerate() {
            steps = steps.push(item);
            if index + 1 < count {
                steps = steps.push(widget::container(caret(true)).padding([0, 0, 0, 14]));
            }
        }
        steps.into()
    } else {
        let mut steps = row![].spacing(spacing.space_xxs).align_y(Alignment::Center);
        for (index, item) in built.enumerate() {
            steps = steps.push(item);
            if index + 1 < count {
                steps = steps.push(caret(false));
            }
        }
        steps.into()
    };

    column![heading, body].spacing(spacing.space_xs).into()
}

/// The caret between two items: right in a row, down in a column.
fn caret<'a>(down: bool) -> Element<'a, Message> {
    let glyph = icons::phosphor_tinted(icons::CARET_RIGHT, 14.0, muted_text_color());
    if down {
        glyph
            .rotation(cosmic::iced::Radians(std::f32::consts::FRAC_PI_2))
            .into()
    } else {
        glyph.into()
    }
}

/// Where a feature starts or stops, with no surface of its own, since there is
/// nothing to set there: in a row, a name over a line about it, at a fixed
/// width so both rows' steps start at the same place; in a column, the two
/// side by side on one line.
fn end<'a>(name: String, caption: String, stacked: bool) -> Element<'a, Message> {
    let muted = muted_text_color();
    let name = text::body(name).font(cosmic::font::semibold());
    let caption = text::caption(caption).class(cosmic::theme::Text::Color(muted));
    if stacked {
        widget::container(
            row![name, caption]
                .spacing(cosmic::theme::spacing().space_xs)
                .align_y(Alignment::Center),
        )
        .padding([2, 4])
        .into()
    } else {
        widget::container(column![name, caption].spacing(2))
            .padding([10, 12])
            .width(Length::Fixed(112.0))
            .into()
    }
}

/// A stage: its name and status dot, the backend in it and how its model
/// stands, with a bar while a download or load reports how far it is, or
/// "Idle" in its bottom-right corner while its model is out of memory until
/// its next use, as the stage's card has. Pressing it opens the stage's card
/// under the rows; the open one wears the accent border.
fn step(app: &AppModel, stage: Stage, selected: Stage) -> Element<'_, Message> {
    let open = stage == selected;
    let mut summary = summary(app, stage);
    let in_flight = operation(app.models.operations.get(stage));
    let idle = in_flight.is_none()
        && super_voice_app_core::pages::models::idle_model(&app.models, stage).is_some();
    let mut progress = None;
    if let Some((detail, fraction)) = in_flight {
        summary.detail = detail;
        progress = fraction;
    }
    let muted = muted_text_color();
    let label = text::caption(stage_name(stage))
        .class(if open {
            cosmic::theme::Text::Accent
        } else {
            cosmic::theme::Text::Color(muted)
        })
        .width(Length::Fill);
    let detail = text::caption(summary.detail).class(cosmic::theme::Text::Color(muted));
    let detail: Element<'_, Message> = if idle {
        row![detail, super::surface::idle_label()]
            .align_y(Alignment::Center)
            .into()
    } else {
        detail.into()
    };
    let mut content = column![
        row![label, status_dot(stage_status(app, stage).color(), 8.0)].align_y(Alignment::Center),
        text::body(summary.title).font(cosmic::font::semibold()),
        detail,
    ]
    .spacing(2)
    .width(Length::Fill);
    if let Some(fraction) = progress {
        content = content.push(
            widget::container(widget::determinate_linear(fraction.max(0.02)).width(Length::Fill))
                .padding([4, 0, 0, 0]),
        );
    }

    button::custom(content)
        .padding([10, 12])
        .width(Length::Fill)
        .on_press(Message::ModelsPage(ModelsPageMessage::SelectPipelineStep(
            stage,
        )))
        .class(step_class(open))
        .into()
}

/// The step's surface: the card fill and hairline, the accent border on the
/// open step, and the standard hover and press fills.
fn step_class(open: bool) -> cosmic::theme::Button {
    fn style(theme: &cosmic::Theme, open: bool, hover: bool) -> cosmic::widget::button::Style {
        let cosmic = theme.cosmic();
        let component = &theme.current_container().component;
        let fill: cosmic::iced::Color = if hover {
            cosmic.button.hover.into()
        } else {
            component.base.into()
        };
        cosmic::widget::button::Style {
            background: Some(cosmic::iced::Background::Color(fill)),
            border_radius: cosmic.corner_radii.radius_s.into(),
            border_width: 1.0,
            border_color: accent_border_color(theme, open),
            icon_color: Some(component.on.into()),
            text_color: Some(component.on.into()),
            ..cosmic::widget::button::Style::new()
        }
    }
    cosmic::theme::Button::Custom {
        active: Box::new(move |_, theme| style(theme, open, false)),
        disabled: Box::new(move |theme| style(theme, open, false)),
        hovered: Box::new(move |_, theme| style(theme, open, true)),
        pressed: Box::new(move |_, theme| style(theme, open, true)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::client::v1::pipeline::StageState;

    fn block(source: Option<&str>, model: Option<&str>, enabled: bool, loaded: bool) -> StageState {
        StageState {
            source: source.map(ToString::to_string),
            name: source.map(|_| "Tidy".to_string()),
            enabled,
            model: model.map(ToString::to_string),
            loaded,
            state: if loaded {
                crate::daemon::client::ModelState::Loaded
            } else {
                crate::daemon::client::ModelState::NotLoaded
            },
            device: None,
        }
    }

    #[test]
    fn an_empty_stage_asks_for_a_backend() {
        assert_eq!(
            transcription_summary(None, "", false, ""),
            StepSummary {
                title: "None".into(),
                detail: "Select a backend".into()
            }
        );
        assert_eq!(
            block_summary(BlockStage::Synthesis, &block(None, None, false, false)).detail,
            "Select a backend"
        );
    }

    /// Post-processing is optional, so an empty one is off rather than missing.
    #[test]
    fn an_empty_post_processor_is_off() {
        assert_eq!(
            block_summary(BlockStage::PostProcessing, &block(None, None, false, false)),
            StepSummary {
                title: "Off".into(),
                detail: "Optional".into()
            }
        );
    }

    #[test]
    fn a_loaded_model_shows_with_its_device() {
        assert_eq!(
            transcription_summary(Some("Whisper"), "whisper-base", true, "cuda"),
            StepSummary {
                title: "Whisper".into(),
                detail: "whisper-base · CUDA".into()
            }
        );
    }

    /// The daemon can still report the previous backend's model just after a
    /// switch; that model is not the new backend's.
    #[test]
    fn another_backends_model_is_not_loaded() {
        assert_eq!(
            transcription_summary(Some("Voxtral"), "whisper-base", false, "cuda").detail,
            "Not loaded"
        );
    }

    /// A post-processor switched on whose model failed to come up says so,
    /// rather than reading as running.
    #[test]
    fn an_enabled_block_whose_model_is_down_is_not_loaded() {
        let summary = block_summary(
            BlockStage::PostProcessing,
            &block(Some("github.com/x/tidy"), Some("rules-v1"), true, false),
        );
        assert_eq!(summary.title, "Tidy");
        assert_eq!(summary.detail, "Not loaded");
    }

    #[test]
    fn a_running_block_shows_its_model() {
        let summary = block_summary(
            BlockStage::PostProcessing,
            &block(Some("github.com/x/tidy"), Some("rules-v1"), true, true),
        );
        assert_eq!(summary.detail, "rules-v1");
    }
}
