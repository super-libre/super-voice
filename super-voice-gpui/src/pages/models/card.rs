// SPDX-License-Identifier: GPL-3.0-only
//! A Models page's steps: the rail of marks down their left, the two ends
//! (where the feature's input comes from, where its output goes), and a
//! card for each step a model fills.
//!
//! A filled step's card: its name and status, Configure… and a menu (Reload
//! this model for synthesis, Show in Library, Deselect this backend); the
//! backend and what it runs on; the download or load under way, or why the
//! last one failed; its fields (model, where it runs, language, voice); and a
//! footer with the model's memory and Load or Unload. An empty step's card
//! is dashed, says what the step is for, and offers the installed backends
//! that fill it, or the Library.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::select::Select;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Anchor, AnyElement, App, Context, FontWeight, IntoElement, ParentElement as _, Pixels,
    SharedString, Styled as _, Window, div, px,
};
use super_voice_app_core::pages::model_language::{
    self as language, Button as LanguageButton, Focus,
};
use super_voice_app_core::pages::model_voice::{self as voice, DefaultVoice, VoiceWarning};
use super_voice_app_core::pages::models::{self as page, Card as StageCard, Memory, Shown};
use super_voice_app_core::rules::backends::{backend_is_online, stage_device_support};
use super_voice_app_core::rules::gpu::accel_name;
use super_voice_app_core::state::Stage;
use super_voice_app_core::state::language::ModelLanguages;
use super_voice_app_core::state::voice::VoiceState;

use super::pickers::{Choice, PickerKey, Pickers};
use super::words::{self, Progress as Shows};
use super::{In, ModelsPage};
use crate::effects::CorePage as _;
use crate::icons::Phosphor;
use crate::pages::Page;
use crate::ui::card::Card;
use crate::ui::status::StatusPill;
use crate::ui::width;
use crate::{t, t_with};

/// A card narrower than this draws its compact layout: the status under the
/// step's name, no backend description or tags.
const COMPACT_CARD: Pixels = px(400.);

/// The icon of a step's mark.
pub fn stage_icon(stage: Stage) -> Icon {
    match stage {
        Stage::Transcription => Phosphor::Waveform.into(),
        Stage::PostProcessing => Phosphor::MagicWand.into(),
        Stage::Synthesis => Phosphor::SpeakerHigh.into(),
    }
}

/// The rail down the steps' left: a round mark per step, joined by a line.
#[derive(Clone, Copy)]
pub struct Rail {
    pub mark: Pixels,
    pub gap: Pixels,
    icon: Pixels,
}

impl Rail {
    /// The rail of a narrow window has smaller marks.
    pub fn new(narrow: bool) -> Self {
        if narrow {
            Self {
                mark: px(24.),
                gap: px(12.),
                icon: px(13.),
            }
        } else {
            Self {
                mark: px(32.),
                gap: px(16.),
                icon: px(15.),
            }
        }
    }

    /// A mark: drawn in the text's colour while its step is filled.
    fn mark(self, icon: Icon, on: bool, cx: &App) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let color = if on {
            theme.foreground
        } else {
            theme.muted_foreground
        };
        div()
            .flex_shrink_0()
            .size(self.mark)
            .rounded_full()
            .border_1()
            .border_color(if on { theme.foreground } else { theme.border })
            .bg(theme.background)
            .flex()
            .items_center()
            .justify_center()
            .child(icon.size(self.icon).text_color(color))
    }

    /// One row of the rail: its mark, the line down to the next, and `body`.
    fn row(
        self,
        mark: impl IntoElement,
        last: bool,
        body: impl IntoElement,
        cx: &App,
    ) -> AnyElement {
        div()
            .flex()
            .flex_row()
            .w_full()
            .gap(self.gap)
            .child(
                v_flex()
                    .flex_shrink_0()
                    .w(self.mark)
                    .items_center()
                    .child(mark)
                    .when(!last, |rail| {
                        rail.child(
                            div()
                                .w(px(1.))
                                .flex_1()
                                .min_h(px(12.))
                                .bg(cx.theme().border),
                        )
                    }),
            )
            .child(div().flex_1().min_w_0().pb(px(18.)).child(body))
            .into_any_element()
    }

    /// Where the feature's input comes from, or its output goes.
    pub fn edge(
        self,
        icon: Icon,
        title: SharedString,
        description: SharedString,
        last: bool,
        cx: &App,
    ) -> AnyElement {
        let body = v_flex()
            .pt(px(if self.mark > px(24.) { 5. } else { 2. }))
            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            );
        self.row(self.mark(icon, false, cx), last, body, cx)
    }

    /// A step a model fills, with its card.
    pub fn step(self, icon: Icon, filled: bool, card: AnyElement, cx: &App) -> AnyElement {
        self.row(self.mark(icon, filled, cx), false, card, cx)
    }
}

/// A field of a card: its label over its control.
fn field(label: SharedString, control: impl IntoElement, cx: &App) -> AnyElement {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_1p5()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(control)
        .into_any_element()
}

/// `fields`, `per_row` to a row, each row's fields the same width.
fn grid(fields: Vec<AnyElement>, per_row: usize) -> AnyElement {
    let per_row = per_row.max(1);
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut fields = fields.into_iter().peekable();
    while fields.peek().is_some() {
        let row: Vec<AnyElement> = fields.by_ref().take(per_row).collect();
        let pad = per_row - row.len();
        rows.push(
            h_flex()
                .w_full()
                .items_start()
                .gap(width::FIELD_GAP)
                .children(row)
                .children((0..pad).map(|_| div().flex_1()))
                .into_any_element(),
        );
    }
    v_flex().w_full().gap_3().children(rows).into_any_element()
}

/// A part of a card set apart from the one above by a line.
fn section(cx: &App) -> gpui_kit::Div {
    v_flex()
        .w_full()
        .gap_2()
        .pt_3()
        .border_t_1()
        .border_color(cx.theme().border)
}

/// A warning line: an icon in the warning colour, and its words.
fn warning(text: SharedString, cx: &App) -> AnyElement {
    h_flex()
        .gap_2()
        .items_start()
        .text_sm()
        .child(
            Icon::new(Phosphor::Warning)
                .size_4()
                .flex_shrink_0()
                .mt_0p5()
                .text_color(cx.theme().warning),
        )
        .child(div().flex_1().min_w_0().child(text))
        .into_any_element()
}

impl ModelsPage {
    /// `stage`'s card, `width` wide.
    pub(super) fn step_card(
        &mut self,
        stage: Stage,
        width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let compact = width < COMPACT_CARD;
        let card = page::card(&self.models, stage);
        let Some(source) = card.source.clone() else {
            return self.empty_card(stage, compact, cx);
        };
        let mut out = self.card_head(stage, &card, &source, compact, cx);

        // The download, load or build under way, or why the last one failed.
        let shows = card
            .build
            .map(|build| words::build(&build.framework, self.registry.installs.get(&build.source)))
            .or_else(|| words::progress(card.operation));
        let pending = card.operation.is_pending() || card.build.is_some();
        let shown_progress = shows.is_some();
        if let Some(shows) = shows {
            out = out.child(progress(stage, &shows, compact, cx));
        }

        // A backend not set up yet offers no model: Configure is the way on.
        if !card.unmet.is_empty() {
            out = out.child(
                section(cx).children(
                    card.unmet
                        .iter()
                        .map(|label| warning(t_with("required-setting", &[("label", label)]), cx)),
                ),
            );
            return out.into_any_element();
        }

        let fields = fields(
            &mut self.pickers,
            Details {
                languages: &self.languages,
                focus: &self.focus,
                voice: &self.voice,
            },
            stage,
            &card,
            window,
            cx,
        );
        let inner = width - px(34.);
        let per_row = width::fields_per_row(inner, fields.len());
        if !fields.is_empty() {
            // Set apart by a line only from a progress above it.
            let grid = grid(fields, per_row);
            out = out.child(if shown_progress {
                section(cx).child(grid).into_any_element()
            } else {
                grid
            });
        }
        let notes = self.notes(stage, &card, cx);
        if !notes.is_empty() {
            out = out.child(v_flex().gap_2().children(notes));
        }
        if !pending {
            out = out.child(self.footer(stage, &card, compact, cx));
        }
        out.into_any_element()
    }

    /// A filled card's name, status and actions, and its backend.
    fn card_head(
        &self,
        stage: Stage,
        card: &StageCard<'_>,
        source: &str,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> Card {
        let status = page::stage_status(&self.models, stage, &self.voice);
        let pill = StatusPill::new(status.dot(), words::status(status, card.operation));
        let view = self.models.view(stage);
        let name = card
            .backend
            .map(|b| b.name.clone())
            .or_else(|| view.name.clone())
            .unwrap_or_else(|| source.to_string());
        let configure = Button::new(SharedString::from(format!("configure-{stage}")))
            .small()
            .label(t("configure-backend"))
            .on_click(cx.listener({
                let source = source.to_string();
                move |page, _, window, cx| page.open_configure(source.clone(), window, cx)
            }));
        let menu = self.step_menu(stage, card, cx);

        let mut out = Card::new().title(words::stage_name(stage));
        if compact {
            out = out.action(configure).action(menu).child(
                h_flex().gap_2().min_w_0().child(pill).child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(backend_name(
                            &name,
                            card.backend.map(|b| b.version.as_str()),
                        )),
                ),
            );
        } else {
            out = out
                .status(pill)
                .action(configure)
                .action(menu)
                .child(self.backend_line(stage, card, &name, cx));
        }

        out
    }

    /// Why the card's model can't load here, or can't speak.
    fn notes(&self, stage: Stage, card: &StageCard<'_>, cx: &App) -> Vec<AnyElement> {
        let mut notes: Vec<AnyElement> = Vec::new();
        if card.no_device {
            let model = match &card.shown {
                Shown::Staging { model: Some(model) } => model.clone(),
                _ => String::new(),
            };
            notes.push(warning(
                t_with("model-device-unavailable", &[("model", &model)]),
                cx,
            ));
        }
        if stage == Stage::Synthesis
            && let Some(said) = voice::warning(&self.voice)
        {
            notes.push(warning(
                t(match said {
                    VoiceWarning::PickACloned => "no-built-in-voices-pick-a-cloned-voice",
                    VoiceWarning::RecordOne => "no-built-in-voices-record-one",
                    VoiceWarning::NoneChosen => "no-cloned-voice-chosen",
                }),
                cx,
            ));
        }
        notes
    }

    /// "Whisper 0.2.1 · Local Whisper speech to text.", then what it runs
    /// on.
    fn backend_line(&self, stage: Stage, card: &StageCard<'_>, name: &str, cx: &App) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let mut line = backend_name(name, card.backend.map(|b| b.version.as_str())).to_string();
        let mut tags: Vec<AnyElement> = Vec::new();
        if let Some(backend) = card.backend {
            if !backend.description.is_empty() {
                line = format!("{line} · {}", backend.description);
            }
            let source = card.source.as_deref().unwrap_or_default();
            let compute = stage_device_support(self.models.offers.backend(stage, source), backend);
            let tag = |icon: Icon, words: SharedString| {
                Tag::secondary()
                    .small()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(icon.size_3().text_color(muted))
                            .child(words),
                    )
                    .into_any_element()
            };
            if backend_is_online(backend) {
                tags.push(tag(Phosphor::Cloud.into(), t("cloud")));
            } else {
                if compute.gpu {
                    tags.push(tag(Phosphor::GraphicsCard.into(), t("chip-gpu")));
                }
                if compute.cpu {
                    tags.push(tag(Phosphor::Cpu.into(), t("chip-cpu")));
                }
            }
        }
        h_flex()
            .gap_2()
            .min_w_0()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(muted)
                    .child(line),
            )
            .children(tags)
            .into_any_element()
    }

    /// The step's "…" menu: Reload this model (synthesis, while its model is
    /// up), Switch to (the other installed backends that fit the step), Show
    /// in Library, Deselect this backend.
    fn step_menu(&self, stage: Stage, card: &StageCard<'_>, cx: &mut Context<Self>) -> AnyElement {
        let page = cx.weak_entity();
        let nav = self.shared.nav.clone();
        let reloadable = stage == Stage::Synthesis
            && matches!(card.shown, Shown::Running { idle: false, .. })
            && !card.busy
            && !card.operation.is_pending();
        let free = !card.busy && !card.operation.is_pending();
        let others: Vec<(String, String, bool)> = page::switch_targets(&self.models, stage)
            .into_iter()
            .map(|b| (b.source.clone(), b.name.clone(), backend_is_online(b)))
            .collect();
        Button::new(SharedString::from(format!("more-{stage}")))
            .small()
            .icon(Phosphor::DotsThreeVertical)
            .accessibility_label(t_with(
                "step-more-actions",
                &[("step", &words::stage_name(stage))],
            ))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                let mut menu = menu;
                if reloadable {
                    let page = page.clone();
                    menu = menu.item(
                        PopupMenuItem::new(t("reload-this-model"))
                            .icon(Phosphor::ArrowClockwise)
                            .on_click(move |_, window, cx| {
                                _ = page.update(cx, |page, cx| {
                                    page.send(
                                        In::Models(page::Input::ReloadModel(stage)),
                                        window,
                                        cx,
                                    );
                                });
                            }),
                    );
                }
                // Straight from one backend to another, without deselecting
                // first.
                if !others.is_empty() {
                    let others = others.clone();
                    let page = page.clone();
                    menu = menu.submenu_with_icon(
                        Some(Phosphor::ArrowRight.into()),
                        t("switch-to-backend"),
                        window,
                        cx,
                        move |menu, _, _| {
                            others.iter().fold(menu, |menu, (source, name, online)| {
                                let page = page.clone();
                                let source = source.clone();
                                let item = PopupMenuItem::new(name.clone())
                                    .disabled(!free)
                                    .on_click(move |_, window, cx| {
                                        _ = page.update(cx, |page, cx| {
                                            let pick = page::Input::PickBackend(
                                                stage,
                                                Some(source.clone()),
                                            );
                                            page.send(In::Models(pick), window, cx);
                                        });
                                    });
                                menu.item(if *online {
                                    item.icon(Phosphor::Cloud)
                                } else {
                                    item
                                })
                            })
                        },
                    );
                }
                let nav = nav.clone();
                menu = menu.item(
                    PopupMenuItem::new(t("show-in-library"))
                        .icon(Phosphor::Books)
                        .on_click(move |_, window, cx| nav.go(Page::Library, window, cx)),
                );
                let page = page.clone();
                menu.separator().item(
                    PopupMenuItem::new(t("deselect-this-backend"))
                        .icon(Phosphor::X)
                        .disabled(!free)
                        .on_click(move |_, window, cx| {
                            _ = page.update(cx, |page, cx| {
                                page.send(
                                    In::Models(page::Input::PickBackend(stage, None)),
                                    window,
                                    cx,
                                );
                            });
                        }),
                )
            })
            .into_any_element()
    }

    /// The card's last line: what its model needs or uses, and Load or
    /// Unload.
    fn footer(
        &self,
        stage: Stage,
        card: &StageCard<'_>,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let mut lines: Vec<AnyElement> = Vec::new();
        if let Some(memory) = page::memory(&self.models, stage) {
            let words = words::memory(memory);
            lines.push(match memory {
                Memory::MayNotFit { .. } => warning(words, cx),
                _ => div().text_color(muted).child(words).into_any_element(),
            });
        }
        if let Some(framework) = &card.fallback {
            lines.push(
                div()
                    .text_color(muted)
                    .child(t_with(
                        "running-on-framework",
                        &[("framework", &accel_name(framework))],
                    ))
                    .into_any_element(),
            );
        }
        if matches!(card.shown, Shown::Running { idle: true, .. }) {
            lines.push(
                div()
                    .text_color(muted)
                    .child(t("model-idle-note"))
                    .into_any_element(),
            );
        }
        let button = match &card.shown {
            Shown::Running { .. } => Button::new(SharedString::from(format!("unload-{stage}")))
                .icon(Phosphor::Stop)
                .label(t("unload"))
                .disabled(card.busy)
                .on_click(cx.listener(move |page, _, window, cx| {
                    page.send(In::Models(page::Input::Unload(stage)), window, cx);
                })),
            _ => Button::new(SharedString::from(format!("load-{stage}")))
                .primary()
                .icon(Phosphor::Play)
                .label(t("load-model"))
                .disabled(!card.can_load)
                .on_click(cx.listener(move |page, _, window, cx| {
                    page.send(In::Models(page::Input::Load(stage)), window, cx);
                })),
        };
        section(cx)
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .map(|lines_box| {
                                if compact {
                                    lines_box.text_xs()
                                } else {
                                    lines_box.text_sm()
                                }
                            })
                            .children(lines),
                    )
                    .child(if compact { button.small() } else { button }),
            )
            .into_any_element()
    }

    /// A step no backend fills: what it is for, and the backends that can
    /// fill it, or the Library when none is installed.
    fn empty_card(&self, stage: Stage, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let backends: Vec<(String, String, bool)> = self
            .models
            .catalog
            .backends(stage)
            .unwrap_or_default()
            .iter()
            .map(|b| (b.source.clone(), b.name.clone(), backend_is_online(b)))
            .collect();
        let installed = !backends.is_empty() || self.models.catalog.backends(stage).is_none();
        let pill = StatusPill::new(
            super_voice_app_core::rules::status::Dot::Grey,
            t("readiness-no-backend"),
        );
        let nav = self.shared.nav.clone();
        let library = Button::new(SharedString::from(format!("library-{stage}")))
            .small()
            .icon(Phosphor::Books)
            .label(t("open-library"))
            .on_click(move |_, window, cx| nav.go(Page::Library, window, cx));
        let library = if installed { library.ghost() } else { library };
        let select = (!backends.is_empty()).then(|| {
            let page = cx.weak_entity();
            Button::new(SharedString::from(format!("select-{stage}")))
                .small()
                .label(t("select-a-backend-menu"))
                .dropdown_caret(true)
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                    backends.iter().fold(menu, |menu, (source, name, online)| {
                        let page = page.clone();
                        let source = source.clone();
                        let item =
                            PopupMenuItem::new(name.clone()).on_click(move |_, window, cx| {
                                _ = page.update(cx, |page, cx| {
                                    let pick =
                                        page::Input::PickBackend(stage, Some(source.clone()));
                                    page.send(In::Models(pick), window, cx);
                                });
                            });
                        menu.item(if *online {
                            item.icon(Phosphor::Cloud)
                        } else {
                            item
                        })
                    })
                })
        });
        let description = div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(words::empty_step(stage, installed));
        let mut out = Card::new()
            .dashed()
            .title(words::stage_name(stage))
            .status(pill);
        if compact {
            out = out
                .child(description)
                .child(h_flex().gap_2().flex_wrap().children(select).child(library));
        } else {
            out = out.action(library);
            if let Some(select) = select {
                out = out.action(select);
            }
            out = out.child(description);
        }
        out.into_any_element()
    }
}

/// What a card's fields read beside the cards: each model's language and
/// the synthesis model's voice.
#[derive(Clone, Copy)]
struct Details<'a> {
    languages: &'a ModelLanguages,
    focus: &'a Focus,
    voice: &'a VoiceState,
}

/// The card's fields: its model, where it runs, its language, and for
/// synthesis its voice.
fn fields(
    pickers: &mut Pickers<ModelsPage>,
    details: Details<'_>,
    stage: Stage,
    card: &StageCard<'_>,
    window: &mut Window,
    cx: &mut Context<ModelsPage>,
) -> Vec<AnyElement> {
    let waits = card.busy || card.operation.is_pending() || card.build.is_some();
    let (shown, running) = match &card.shown {
        Shown::Running { model, .. } => (Some(model.clone()), true),
        Shown::Staging { model } => (model.clone(), false),
        Shown::Empty => (None, false),
    };
    let mut fields = Vec::new();

    let models: Vec<Choice> = card
        .models
        .iter()
        .map(|m| Choice::new(m.clone(), m.clone()))
        .collect();
    let picker = pickers.get(
        &PickerKey::Model(stage),
        models,
        shown.as_deref(),
        false,
        window,
        cx,
    );
    fields.push(field(
        t("model"),
        Select::new(&picker)
            .w_full()
            .placeholder(t("select-model"))
            .accessibility_label(t("model"))
            // A running model is unloaded before another is picked.
            .disabled(running || waits),
        cx,
    ));

    if let Some(devices) = card.devices.filter(|d| !d.is_empty()) {
        let items: Vec<Choice> = devices
            .iter()
            .map(|d| Choice::new(d.clone(), accel_name(d)))
            .collect();
        let picker = pickers.get(
            &PickerKey::Device(stage),
            items,
            card.device,
            false,
            window,
            cx,
        );
        fields.push(field(
            t("runs-on"),
            Select::new(&picker)
                .w_full()
                .placeholder(t("select"))
                .accessibility_label(t("runs-on"))
                .disabled(waits),
            cx,
        ));
    }

    if stage == Stage::Synthesis
        && let Some((items, chosen)) = voice_choices(details, card, shown.as_deref())
    {
        let picker = pickers.get(&PickerKey::Voice, items, Some(&chosen), true, window, cx);
        fields.push(field(
            t("voice"),
            Select::new(&picker)
                .w_full()
                .accessibility_label(t("voice")),
            cx,
        ));
    }

    if let Some((items, selected, fixed)) = language_choices(details, stage, card, shown.as_deref())
    {
        let picker = pickers.get(
            &PickerKey::Language(stage),
            items,
            selected.as_deref(),
            true,
            window,
            cx,
        );
        fields.push(field(
            t("language"),
            Select::new(&picker)
                .w_full()
                .placeholder(t("language"))
                .accessibility_label(t("language"))
                .disabled(fixed),
            cx,
        ));
    }
    fields
}

/// The language picker of the model the card shows: its rows, the row
/// picked, and whether the model has one language only, which it shows with
/// nothing to pick.
fn language_choices(
    details: Details<'_>,
    stage: Stage,
    card: &StageCard<'_>,
    shown: Option<&str>,
) -> Option<(Vec<Choice>, Option<String>, bool)> {
    let model = shown?;
    let key = details
        .focus
        .get(stage)
        .filter(|key| key.model == model && Some(&key.source) == card.source.as_ref())?;
    let button = language::button(details.languages, key)?;
    let offers = details
        .languages
        .offered(stage, &key.source, &key.model)
        .unwrap_or_default();
    let chosen = language::chosen(details.languages, key).unwrap_or_default();
    let items: Vec<Choice> = if let LanguageButton::Fixed(tag) = &button {
        vec![Choice::new(tag.clone(), words::language(&button))]
    } else {
        language::picker_rows(offers, "")
            .into_iter()
            .map(|row| match row {
                // Following, the row says what that comes to.
                language::Row::FollowGlobal if chosen.is_empty() => {
                    Choice::new("", words::language(&button))
                }
                language::Row::FollowGlobal => Choice::new("", t("follow-global")),
                language::Row::Language(tag, name) => Choice::new(tag, name),
            })
            .collect()
    };
    Some(match button {
        LanguageButton::Fixed(tag) => (items, Some(tag), true),
        LanguageButton::Unread => (items, None, false),
        LanguageButton::Pickable(_) => (items, Some(chosen), false),
    })
}

/// The synthesis model's voice picker: the model's own default first, then
/// each voice, a cloned one marked; and the row picked.
fn voice_choices(
    details: Details<'_>,
    card: &StageCard<'_>,
    shown: Option<&str>,
) -> Option<(Vec<Choice>, String)> {
    let (source, model) = (card.source.as_deref()?, shown?);
    if !voice::shows(details.voice, source, model) {
        return None;
    }
    let (default, voices) = voice::picker(details.voice)?;
    let default = Choice::new(
        "",
        match default {
            DefaultVoice::NoneChosen => t("no-cloned-voice-chosen"),
            DefaultVoice::Named(label) => t_with("voice-default", &[("label", &label)]),
        },
    );
    let items = std::iter::once(default)
        .chain(voices.into_iter().map(|(v, cloned)| {
            Choice::new(
                v.id.clone(),
                if cloned {
                    t_with("voice-cloned", &[("label", &v.label)])
                } else {
                    v.label.clone().into()
                },
            )
        }))
        .collect();
    Some((
        items,
        voice::chosen(details.voice).unwrap_or_default().to_string(),
    ))
}

/// "Whisper 0.2.1".
fn backend_name(name: &str, version: Option<&str>) -> SharedString {
    match version.filter(|v| !v.is_empty()) {
        Some(version) => format!("{name} {version}").into(),
        None => name.to_string().into(),
    }
}

/// A step's download, load, build or failure, set apart from the card above.
fn progress(
    stage: Stage,
    shows: &Shows,
    compact: bool,
    cx: &mut Context<ModelsPage>,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let bar = |id: &str, percent: Option<f32>| {
        Progress::new(SharedString::from(format!("{id}-{stage}")))
            .small()
            .map(|bar| match percent {
                Some(percent) => bar.value(percent),
                None => bar.loading(true),
            })
    };
    let body: AnyElement = match shows {
        Shows::Download {
            line,
            bytes,
            percent,
        } => download(stage, line, bytes, *percent, compact, cx),
        Shows::Load {
            title,
            step,
            percent,
        } => v_flex()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(title.clone()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(muted)
                    .child(div().flex_1().child(step.clone()))
                    .when_some(*percent, |row, percent| row.child(words::percent(percent))),
            )
            .child(bar("load", *percent))
            .into_any_element(),
        Shows::Build {
            line,
            bytes,
            percent,
        } => v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .text_sm()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::MEDIUM)
                            .child(line.clone()),
                    )
                    .child(div().text_color(muted).child(bytes.clone()))
                    .when_some(*percent, |row, percent| row.child(words::percent(percent))),
            )
            .child(bar("build", *percent))
            .into_any_element(),
        Shows::Failed(message) => h_flex()
            .gap_2()
            .items_start()
            .text_sm()
            .child(
                Icon::new(Phosphor::XCircle)
                    .size_4()
                    .flex_shrink_0()
                    .mt_0p5()
                    .text_color(cx.theme().danger),
            )
            .child(div().flex_1().min_w_0().child(message.clone()))
            .into_any_element(),
    };
    section(cx).child(body).into_any_element()
}

/// A step's download: what, how far, and Cancel.
fn download(
    stage: Stage,
    line: &SharedString,
    bytes: &SharedString,
    percent: f32,
    compact: bool,
    cx: &mut Context<ModelsPage>,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let bar = |percent: f32| {
        Progress::new(SharedString::from(format!("download-{stage}")))
            .small()
            .value(percent)
    };
    let cancel = Button::new(SharedString::from(format!("cancel-{stage}")))
        .small()
        .label(t("cancel"))
        .on_click(cx.listener(move |page, _, window, cx| {
            page.send(In::Models(page::Input::CancelDownload(stage)), window, cx);
        }));
    let after = div()
        .text_xs()
        .text_color(muted)
        .child(t("model-loads-after-download"));
    if compact {
        let size = if bytes.is_empty() {
            words::percent(percent)
        } else {
            format!("{bytes} · {}", words::percent(percent)).into()
        };
        v_flex()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(line.clone()),
            )
            .child(bar(percent))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().text_xs().text_color(muted).child(size))
                    .child(cancel),
            )
            .child(after)
            .into_any_element()
    } else {
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .text_sm()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_weight(FontWeight::MEDIUM)
                            .child(line.clone()),
                    )
                    .child(div().text_color(muted).child(bytes.clone()))
                    .child(
                        div()
                            .w(px(40.))
                            .flex()
                            .justify_end()
                            .child(words::percent(percent)),
                    ),
            )
            .child(bar(percent))
            .child(h_flex().gap_2().child(after.flex_1()).child(cancel))
            .into_any_element()
    }
}
