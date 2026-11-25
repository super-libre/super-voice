// SPDX-License-Identifier: GPL-3.0-only
//! Text to speech › Clips: text kept as spoken audio, to play again or
//! export. The clips are a list; the one picked opens beside it with its
//! player (a Button and a Slider over the core's player: the kit has no audio
//! player), its words, Export… and Delete…. The composer for the next clip
//! sits under it, or alone while nothing is picked.
//!
//! The logic is the core's Clips page (`pages::clips`). The voice a clip is
//! made in is the synthesis model's, read with the core's `pages::model_voice`
//! for the model the window's readiness names.

use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::AlertDialog;
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants as _};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Anchor, AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, px,
};
use super_voice_app_core::clips::Clip;
use super_voice_app_core::events::{self, Event};
use super_voice_app_core::pages::clips::{self as page, Notice};
use super_voice_app_core::pages::model_voice;
use super_voice_app_core::pages::readiness::{Feature, Readiness};
use super_voice_app_core::state::VoiceState;
use super_voice_app_core::state::clips::{ClipsState, Playing};

use crate::effects::{self, CorePage, Runner, runner};
use crate::pages::{Page, PageView, PaletteCommand, SidebarMark};
use crate::shared::Shared;
use crate::ui::empty_state::EmptyState;
use crate::ui::key_list::{self, KeyList, KeyListState};
use crate::ui::list_detail::ListDetail;
use crate::ui::page_header::PageHeader;
use crate::ui::toast::{self, Toast};
use crate::ui::width;
use crate::{t, t_with};

/// How often the player's position and the making clock move.
const TICK: Duration = Duration::from_millis(100);

/// The slider's steps: fine enough to land within a few milliseconds of any
/// point of a clip.
const SLIDER_STEPS: f32 = 1000.;

/// How much of a clip's first line its row shows; the rest is cut at a word.
const ROW_CHARS: usize = 60;

/// How much of it a question names.
const NAME_CHARS: usize = 40;

/// What happened on the page: the clips', or the synthesis model's voice.
#[derive(Debug, Clone)]
pub enum In {
    Clips(page::Input),
    Voice(model_voice::Input),
    /// An answer with nothing in it: a clip made after Cancel let it go.
    Nothing,
}

/// The work either core page asks for.
pub enum Out {
    Clips(page::Effect),
    Voice(model_voice::Effect),
}

/// What the detail pane shows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pane {
    /// Nothing picked: beside the list, the composer; alone, the list.
    Nothing,
    /// A clip, by its id.
    Clip(String),
    /// The composer, picked with New clip.
    New,
}

pub struct ClipsPage {
    state: ClipsState<String>,
    voice: VoiceState,
    shared: Shared,
    pane: Pane,
    draft: Entity<TextareaState>,
    slider: Entity<SliderState>,
    /// The slider is held: its value is the user's, not the player's.
    dragging: bool,
    /// Moves the player's position and the making clock while either runs.
    ticker: Option<Task<()>>,
    /// The list, as the keyboard reaches it.
    keys: KeyListState,
    _subscriptions: Vec<Subscription>,
}

impl PageView for ClipsPage {
    fn new(_: Page, shared: &Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let draft = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(t("what-the-clip-should-say"))
                .auto_grow(3, 8)
        });
        let slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(SLIDER_STEPS)
                .step(1.)
                .default_value(0.)
        });
        let subscriptions = vec![
            cx.subscribe_in(&draft, window, |this, state, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.state.draft = state.read(cx).value().to_string();
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &slider,
                window,
                |this, _, event: &SliderEvent, window, cx| match event {
                    SliderEvent::Change(_) => this.dragging = true,
                    SliderEvent::Release(value) => {
                        this.dragging = false;
                        let fraction = value.end() / SLIDER_STEPS;
                        this.send(In::Clips(page::Input::Seek(fraction)), window, cx);
                    }
                },
            ),
            // The synthesis model changed: read the voice it speaks in.
            cx.observe_in(&shared.readiness, window, |this, _, window, cx| {
                this.follow_synthesis(window, cx);
            }),
        ];
        Self {
            state: ClipsState::default(),
            voice: VoiceState::default(),
            shared: shared.clone(),
            pane: Pane::Nothing,
            draft,
            slider,
            dragging: false,
            ticker: None,
            keys: KeyListState::new(cx),
            _subscriptions: subscriptions,
        }
    }

    fn reached(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send(In::Clips(page::Input::Reload), window, cx);
        self.follow_synthesis(window, cx);
    }

    fn shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The folder is the truth, and another copy of the app may have
        // added to it.
        if self.state.loaded {
            self.send(In::Clips(page::Input::Reload), window, cx);
        }
    }

    fn heard(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
        // A model's voice was set, or the library it draws on changed.
        if matches!(events::setting_changed(event), Some("voice" | "voices")) {
            self.send(In::Voice(model_voice::Input::Reload), window, cx);
        }
    }

    fn back(&mut self, _: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.pane == Pane::Nothing {
            return false;
        }
        self.pane = Pane::Nothing;
        cx.notify();
        true
    }

    fn commands(&self, _: &App) -> Vec<PaletteCommand> {
        let feature = t("text-to-speech");
        let mut commands = vec![
            PaletteCommand::new("new-clip", t("new-clip"), IconName::Plus).detail(feature.clone()),
        ];
        if self.state.playing.is_some() {
            commands.push(
                PaletteCommand::new("stop-clip", t("stop-the-clip"), IconName::Square)
                    .detail(feature),
            );
        }
        let group = t("clips");
        commands.extend(self.state.items.iter().map(|clip| {
            PaletteCommand::new(
                format!("clip:{}", clip.id),
                page::first_line(&clip.text, ROW_CHARS),
                Lucide::FileVolume,
            )
            .group(group.clone())
            .keywords([SharedString::from(clip.text.clone())])
        }));
        commands
    }

    fn run(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            "new-clip" => self.compose(window, cx),
            "stop-clip" => self.send(In::Clips(page::Input::Stop), window, cx),
            _ => {
                if let Some(clip) = id.strip_prefix("clip:") {
                    self.pick(clip.to_string(), window, cx);
                }
            }
        }
    }

    fn sidebar_mark(&self, _: &App) -> Option<SidebarMark> {
        self.state
            .loaded
            .then_some(SidebarMark::Count(self.state.items.len()))
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, scenes: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        pretend::fill(self, scenes, window, cx);
    }
}

impl KeyList for ClipsPage {
    fn key_list(&self) -> &KeyListState {
        &self.keys
    }

    fn key_list_mut(&mut self) -> &mut KeyListState {
        &mut self.keys
    }

    fn rows(&self, _: &App) -> Vec<String> {
        self.state
            .items
            .iter()
            .map(|clip| clip.id.clone())
            .collect()
    }

    fn picked(&self) -> Option<String> {
        match &self.pane {
            Pane::Clip(id) => Some(id.clone()),
            _ => None,
        }
    }

    fn open_row(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.pick(id.to_string(), window, cx);
    }

    /// The count sits above the clips.
    fn first_row(&self, _: &App) -> usize {
        1
    }
}

impl CorePage for ClipsPage {
    type Input = In;
    type Effect = Out;

    fn send(&mut self, input: In, window: &mut Window, cx: &mut Context<Self>) {
        match input {
            In::Clips(input) => {
                let asked = self.state.confirming_delete.clone();
                let outcome = page::update(&mut self.state, input);
                effects::run_effects(
                    self,
                    outcome.effects.into_iter().map(Out::Clips),
                    window,
                    cx,
                );
                toast::notice::<Self>(window, cx, outcome.clear_notice, outcome.notice.map(notice));
                // A clip made empties the draft; so does its field.
                if self.state.draft.is_empty() && !self.draft.read(cx).value().is_empty() {
                    self.draft
                        .update(cx, |draft, cx| draft.set_value("", window, cx));
                }
                if let Some(id) = self.state.confirming_delete.clone()
                    && asked.as_ref() != Some(&id)
                {
                    self.ask_to_delete(id, window, cx);
                }
                // A deleted clip can't stay open.
                if let Pane::Clip(id) = &self.pane
                    && self.state.loaded
                    && self.state.clip(id).is_none()
                {
                    self.pane = Pane::Nothing;
                }
                self.keep_ticking(window, cx);
                self.show_position(window, cx);
            }
            In::Voice(input) => {
                let outcome = model_voice::update(&mut self.voice, input);
                effects::run_effects(
                    self,
                    outcome.effects.into_iter().map(Out::Voice),
                    window,
                    cx,
                );
            }
            In::Nothing => {}
        }
        cx.notify();
    }

    fn effect(
        &mut self,
        effect: Out,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Runner<In>> {
        Some(match effect {
            Out::Clips(page::Effect::List) => runner(async { In::Clips(page::run_list().await) }),
            Out::Clips(page::Effect::Make { text, voice, abort }) => runner(async move {
                page::run_make(text, voice, abort)
                    .await
                    .map_or(In::Nothing, In::Clips)
            }),
            Out::Clips(page::Effect::ReadSamples(id)) => {
                runner(async move { In::Clips(page::run_samples(id).await) })
            }
            Out::Clips(page::Effect::Export { id, name }) => {
                // The dialog is the platform's, opened here, in Documents as
                // a note's is; the copy runs once it answers. GPUI's dialog
                // takes no file types, so it can't be limited to WAV; the
                // name it suggests ends in .wav, and the core adds it to a
                // name typed without.
                let chosen = cx
                    .prompt_for_new_path(&super_voice_app_core::pages::export_start(), Some(&name));
                runner(async move {
                    let result = match chosen.await {
                        Ok(Ok(Some(path))) => page::export_to(&id, &path).await,
                        Ok(Ok(None)) | Err(_) => Ok(None),
                        Ok(Err(e)) => Err(e.to_string()),
                    };
                    In::Clips(page::Input::Exported { id, result })
                })
            }
            Out::Clips(page::Effect::Delete(id)) => {
                runner(async move { In::Clips(page::run_delete(id).await) })
            }
            Out::Voice(model_voice::Effect::Read { source, model }) => {
                runner(async move { In::Voice(model_voice::run_read(source, model).await) })
            }
            Out::Voice(model_voice::Effect::List { source, model }) => {
                runner(async move { In::Voice(model_voice::run_list(source, model).await) })
            }
            // The Clips page never sets a voice: the Models page does.
            Out::Voice(model_voice::Effect::Save { .. }) => return None,
        })
    }
}

/// What the page tells the user, in the app's words.
fn notice(notice: Notice) -> Toast {
    let (key, e) = match notice {
        Notice::CouldNotRead(e) => ("clips-could-not-be-read", e),
        Notice::CouldNotMake(e) => ("the-clip-could-not-be-made", e),
        Notice::CouldNotPlay(e) => ("the-clip-could-not-play", e),
        Notice::CouldNotExport(e) => ("the-clip-could-not-be-exported", e),
        Notice::CouldNotDelete(e) => ("the-clip-could-not-be-deleted", e),
    };
    Toast::error(t_with(key, &[("error", &e)]))
}

impl ClipsPage {
    /// Read the voice the synthesis model speaks in, when the model changed.
    fn follow_synthesis(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (source, model) = self.synthesis(cx);
        let target =
            (!source.is_empty() && !model.is_empty()).then(|| (source.clone(), model.clone()));
        if self.voice.target != target {
            self.send(
                In::Voice(model_voice::Input::Focus { source, model }),
                window,
                cx,
            );
        }
    }

    /// The synthesis stage's backend and model, empty when it has none.
    fn synthesis(&self, cx: &App) -> (String, String) {
        self.shared
            .readiness
            .read(cx)
            .stage(Feature::TextToSpeech)
            .map_or_else(
                || (String::new(), String::new()),
                |stage| {
                    (
                        stage.source.clone().unwrap_or_default(),
                        stage.model.clone().unwrap_or_default(),
                    )
                },
            )
    }

    /// The voice a clip made now is spoken in.
    fn voice_label(&self, cx: &App) -> Option<String> {
        let (source, model) = self.synthesis(cx);
        page::voice_label(&self.voice, &source, &model)
    }

    /// Open the composer, and put the cursor in it.
    fn compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pane = Pane::New;
        self.draft.update(cx, |draft, cx| draft.focus(window, cx));
        cx.notify();
    }

    /// Open the clip `id`.
    fn pick(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.pane = Pane::Clip(id);
        self.show_position(window, cx);
        cx.notify();
    }

    fn make(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let voice = self.voice_label(cx);
        self.send(In::Clips(page::Input::Make { voice }), window, cx);
    }

    /// Run the ticker while a clip plays or is made, and only then.
    fn keep_ticking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let busy =
            self.state.playing.as_ref().is_some_and(|p| !p.paused) || self.state.making.is_some();
        if !busy {
            self.ticker = None;
            return;
        }
        if self.ticker.is_some() {
            return;
        }
        self.ticker = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let going = this.update_in(cx, |page, window, cx| {
                    page.send(In::Clips(page::Input::Tick), window, cx);
                    page.ticker.is_some()
                });
                if !matches!(going, Ok(true)) {
                    return;
                }
            }
        }));
    }

    /// Put the slider where the open clip has played to, unless it is held.
    fn show_position(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dragging {
            return;
        }
        let fraction = if let (Pane::Clip(id), Some(playing)) = (&self.pane, &self.state.playing)
            && playing.id == *id
        {
            playing.fraction()
        } else {
            0.
        };
        let value = fraction * SLIDER_STEPS;
        if (self.slider.read(cx).value().end() - value).abs() >= 1. {
            self.slider
                .update(cx, |slider, cx| slider.set_value(value, window, cx));
        }
    }

    /// Ask before deleting the clip `id`, naming it by its first words.
    fn ask_to_delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(clip) = self.state.clip(&id) else {
            return;
        };
        let name = page::first_line(&clip.text, NAME_CHARS);
        let title = t_with("delete-clip-named", &[("name", &name)]);
        let this = cx.weak_entity();
        let cancel = this.clone();
        window.open_alert_dialog(cx, move |alert: AlertDialog, _, _| {
            let this = this.clone();
            let cancel = cancel.clone();
            let id = id.clone();
            alert
                .title(title.clone())
                .description(t("delete-clip-description"))
                .confirm()
                .ok_text(t("delete"))
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(t("cancel"))
                .on_ok(move |_, window, cx| {
                    let id = id.clone();
                    _ = this.update(cx, |page, cx| {
                        page.send(In::Clips(page::Input::DeleteConfirmed(id)), window, cx);
                    });
                    true
                })
                .on_cancel(move |_, window, cx| {
                    _ = cancel.update(cx, |page, cx| {
                        page.send(In::Clips(page::Input::DeleteCancelled), window, cx);
                    });
                    true
                })
        });
    }

    /// The facts under a clip's first words: when it was made, its voice and
    /// length; or how far it has played.
    fn about(&self, clip: &Clip, today: chrono::NaiveDate) -> SharedString {
        let length = page::clock(Duration::from_secs_f32(clip.seconds.max(0.)));
        let Some(playing) = self.state.playing.as_ref().filter(|p| p.id == clip.id) else {
            let mut parts = vec![page::when(clip.created, today)];
            parts.extend(clip.voice.clone());
            parts.push(length);
            return parts.join(" · ").into();
        };
        let key = if playing.paused {
            "paused-position"
        } else {
            "playing-position"
        };
        match (playing.position(), playing.length()) {
            (Some(at), Some(of)) => t_with(
                key,
                &[("position", &page::clock(at)), ("length", &page::clock(of))],
            ),
            _ => t("starting"),
        }
    }

    /// The clips, newest first.
    fn list(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        if !self.state.loaded {
            return div()
                .p_4()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(t("loading-progress"))
                .into_any_element();
        }
        if self.state.items.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .child(
                    EmptyState::new(Lucide::FileVolume, t("no-clips-yet"))
                        .description(t("each-clip-you-make-shows-here")),
                )
                .into_any_element();
        }
        let today = chrono::Local::now().date_naive();
        let picked = match &self.pane {
            Pane::Clip(id) => Some(id.as_str()),
            _ => None,
        };
        let ringed = key_list::ringed(self, window, cx);
        let rows = self.state.items.iter().map(|clip| {
            let row = self.clip_row(clip, picked == Some(clip.id.as_str()), today, cx);
            key_list::row(row, ringed.as_deref() == Some(clip.id.as_str()), window, cx)
        });
        let rows: Vec<_> = rows.collect();
        // The rows are the list's own children, after the count, for the
        // keyboard to scroll to.
        key_list::wire(div().id("clip-list"), self, cx)
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .p_2()
            .gap_0p5()
            .child(
                div()
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t_with(
                        "clips-count",
                        &[("count", &self.state.items.len().to_string())],
                    )),
            )
            .children(rows)
            .into_any_element()
    }

    /// A clip in the list: how it starts, and when it was made, its voice
    /// and length, or how far it has played.
    fn clip_row(
        &self,
        clip: &Clip,
        picked: bool,
        today: chrono::NaiveDate,
        cx: &mut Context<Self>,
    ) -> ListItem {
        let playing = self.state.is_playing_on(&clip.id);
        let icon = if playing {
            Lucide::Volume2
        } else {
            Lucide::FileVolume
        };
        let id = clip.id.clone();
        ListItem::new(SharedString::from(format!("clip-{}", clip.id)))
            .selected(picked)
            .accessibility_label(page::first_line(&clip.text, ROW_CHARS))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.pick(id.clone(), window, cx);
            }))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_start()
                    .child(
                        div()
                            .pt_0p5()
                            .text_color(if playing {
                                cx.theme().primary
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(gpui_kit::component::Icon::new(icon).small()),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(page::first_line(&clip.text, ROW_CHARS)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(self.about(clip, today)),
                            ),
                    ),
            )
    }

    fn new_clip_button(cx: &mut Context<Self>) -> Button {
        Button::new("new-clip")
            .icon(IconName::Plus)
            .label(t("new-clip"))
            .on_click(cx.listener(|this, _, window, cx| this.compose(window, cx)))
    }

    /// The clip `clip` opened: when it was made, Export… and Delete…, its
    /// player and its words.
    fn clip(&self, clip: &Clip, cx: &mut Context<Self>) -> impl IntoElement {
        let exported = self
            .state
            .exported
            .as_ref()
            .filter(|(exported, _)| *exported == clip.id)
            .map(|(_, path)| t_with("exported-to", &[("path", path)]));
        let muted = cx.theme().muted_foreground;
        v_flex()
            .size_full()
            .child(Self::clip_header(clip, cx))
            .child(
                div()
                    .id("clip")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .p_4()
                            .gap_4()
                            .child(self.player(clip, cx))
                            .child(
                                div()
                                    .text_base()
                                    .line_height(px(26.))
                                    .child(clip.text.clone()),
                            )
                            .when_some(exported, |body, exported| {
                                body.child(div().text_xs().text_color(muted).child(exported))
                            }),
                    ),
            )
    }

    /// Over an open clip: when it was made, its length and voice, Export…,
    /// and a menu with Delete….
    fn clip_header(clip: &Clip, cx: &mut Context<Self>) -> impl IntoElement {
        let today = chrono::Local::now().date_naive();
        let mut facts = vec![
            t_with(
                "seconds-short",
                &[("seconds", &format!("{:.0}", clip.seconds.max(0.)))],
            )
            .to_string(),
        ];
        facts.extend(clip.voice.clone());
        let id = clip.id.clone();
        let menu_id = clip.id.clone();
        let weak = cx.weak_entity();
        h_flex()
            .px_4()
            .py_2p5()
            .gap_2()
            .min_h(px(52.))
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(page::when(clip.created, today)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(facts.join(" · ")),
                    ),
            )
            .child(
                Button::new("export-clip")
                    .outline()
                    .label(t("export"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.send(In::Clips(page::Input::Export(id.clone())), window, cx);
                    })),
            )
            .child(
                Button::new("clip-more")
                    .ghost()
                    .icon(IconName::Ellipsis)
                    .accessibility_label(t("clip-actions"))
                    .tooltip(t("clip-actions"))
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                        let weak = weak.clone();
                        let id = menu_id.clone();
                        menu.item(
                            PopupMenuItem::new(t("delete-clip-ellipsis"))
                                .icon(IconName::Delete)
                                .on_click(move |_, window, cx| {
                                    let id = id.clone();
                                    _ = weak.update(cx, |this, cx| {
                                        this.send(
                                            In::Clips(page::Input::DeleteRequested(id)),
                                            window,
                                            cx,
                                        );
                                    });
                                }),
                        )
                    }),
            )
    }

    /// The player: Play or Pause, Stop while it plays or is paused, where
    /// it is, a slider to move it (paused too), and how long the clip is.
    /// Built from Buttons and a Slider over the core's player: the kit has no
    /// audio player.
    fn player(&self, clip: &Clip, cx: &mut Context<Self>) -> impl IntoElement {
        let length = Duration::from_secs_f32(clip.seconds.max(0.));
        let playing = self.state.playing.as_ref().filter(|p| p.id == clip.id);
        let position = playing.and_then(Playing::position).unwrap_or_default();
        let play = if playing.is_some_and(|p| p.paused) {
            Button::new("resume-clip")
                .primary()
                .icon(IconName::Play)
                .accessibility_label(t("play"))
                .tooltip(t("play"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.send(In::Clips(page::Input::Resume), window, cx);
                }))
        } else if playing.is_some() {
            Button::new("pause-clip")
                .primary()
                .icon(IconName::Pause)
                .accessibility_label(t("pause"))
                .tooltip(t("pause"))
                // Nothing to pause until its audio is read.
                .disabled(playing.is_some_and(|p| p.audio.is_none()))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.send(In::Clips(page::Input::Pause), window, cx);
                }))
        } else {
            let id = clip.id.clone();
            Button::new("play-clip")
                .primary()
                .icon(IconName::Play)
                .accessibility_label(t("play"))
                .tooltip(t("play"))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.send(In::Clips(page::Input::Play(id.clone())), window, cx);
                }))
        };
        // Back to the start, the speakers let go.
        let stop = playing.is_some().then(|| {
            Button::new("stop-clip")
                .ghost()
                .icon(IconName::Square)
                .accessibility_label(t("stop"))
                .tooltip(t("stop"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.send(In::Clips(page::Input::Stop), window, cx);
                }))
        });
        let muted = cx.theme().muted_foreground;
        GroupBox::new().fill().child(
            h_flex()
                .gap_3()
                .items_center()
                .child(h_flex().gap_1().child(play).children(stop))
                .child(time(position, muted))
                .child(
                    div().flex_1().min_w_0().child(
                        Slider::new(&self.slider)
                            .disabled(playing.is_none_or(|p| p.audio.is_none()))
                            .accessibility_label(t("position-in-the-clip")),
                    ),
                )
                .child(time(length, muted)),
        )
    }

    /// The composer: the next clip's words and Make clip; or, while one is
    /// made, how long it has taken and Cancel.
    fn composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        if let Some(making) = &self.state.making {
            return self.making(making.started, cx);
        }
        let readiness = self.shared.readiness.read(cx).of(Feature::TextToSpeech);
        let ready = matches!(readiness, Readiness::Ready | Readiness::Idle);
        let words = super_voice_app_core::notes::words(&self.state.draft);
        let estimate: SharedString = if !ready {
            t("text-to-speech-is-not-ready")
        } else if words == 0 {
            t("type-what-the-clip-should-say")
        } else {
            let count = if words == 1 {
                t("count-1-word")
            } else {
                t_with("words-count", &[("n", &words.to_string())])
            };
            let seconds = page::estimated_seconds(words).to_string();
            format!(
                "{count} · {}",
                t_with("about-seconds", &[("seconds", &seconds)])
            )
            .into()
        };
        let voice = self
            .voice_label(cx)
            .map(|voice| t_with("spoken-in", &[("voice", &voice)]));
        let nav = self.shared.nav.clone();
        GroupBox::new()
            .outline()
            .title(t("new-clip"))
            .child(Textarea::new(&self.draft).aria_label(t("new-clip")))
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(160.))
                            .gap_0p5()
                            .child(div().text_sm().text_color(muted).child(estimate))
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .text_xs()
                                    .text_color(muted)
                                    .children(voice)
                                    .child(
                                        Button::new("change-voice")
                                            .link()
                                            .xsmall()
                                            .label(t("change-the-voice"))
                                            .on_click(move |_, window, cx| {
                                                nav.go(Page::TtsModels, window, cx);
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        Button::new("make-clip")
                            .primary()
                            .label(t("make-clip"))
                            .disabled(!ready || words == 0)
                            .on_click(cx.listener(|this, _, window, cx| this.make(window, cx))),
                    ),
            )
            .into_any_element()
    }

    /// The composer while a clip is made: how long it has taken, Cancel, and
    /// a bar that moves, since the daemon can't say how far it has got.
    fn making(&self, started: std::time::Instant, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let making = started;

        GroupBox::new()
            .outline()
            .title(t("new-clip"))
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t("making-the-clip")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(page::clock(making.elapsed())),
                            ),
                    )
                    .child(
                        Button::new("cancel-clip")
                            .label(t("cancel"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.send(In::Clips(page::Input::Cancel), window, cx);
                            })),
                    ),
            )
            .child(Progress::new("making").small().loading(true))
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .truncate()
                    .child(page::first_line(&self.state.draft, 90)),
            )
            .into_any_element()
    }
}

/// A time beside the player: "0:02".
fn time(at: Duration, color: gpui_kit::Hsla) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .text_sm()
        .text_color(color)
        .child(page::clock(at))
}

impl Render for ClipsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let two_panes = width::content(window) >= width::TWO_PANES;
        let composer = div()
            .id("composer")
            .size_full()
            .overflow_y_scroll()
            .child(v_flex().p_4().child(self.composer(cx)));
        let detail: Option<AnyElement> = match &self.pane {
            Pane::Clip(id) => self.state.clip(id).cloned().map(|clip| {
                let opened = self.clip(&clip, cx).into_any_element();
                if two_panes {
                    // Beside the list, the composer under the clip.
                    v_flex()
                        .size_full()
                        .child(div().flex_1().min_h_0().child(opened))
                        .child(
                            div()
                                .flex_shrink_0()
                                .p_4()
                                .border_t_1()
                                .border_color(cx.theme().border)
                                .child(self.composer(cx)),
                        )
                        .into_any_element()
                } else {
                    opened
                }
            }),
            Pane::New => Some(composer.into_any_element()),
            Pane::Nothing => None,
        };
        let placeholder = div()
            .id("composer-beside")
            .size_full()
            .overflow_y_scroll()
            .child(v_flex().p_4().child(self.composer(cx)));
        let mut header =
            PageHeader::new(Page::Clips).description(t("type-something-and-keep-it-as-speech"));
        // New clip, unless the composer is the page shown alone.
        if two_panes || self.pane != Pane::New {
            header = header.action(Self::new_clip_button(cx));
        }
        v_flex().size_full().child(header).child(
            div()
                .flex_1()
                .min_h_0()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    ListDetail::new("clips", self.list(window, cx))
                        .detail(detail)
                        .placeholder(placeholder)
                        .back(
                            t("clips"),
                            cx.listener(|this, _, window, cx| {
                                this.back(window, cx);
                            }),
                        ),
                ),
        )
    }
}

/// Made-up clips for `just app-shot`: three, in Kokoro's and Qwen TTS's
/// voices. Scenes: `clips-empty` (none yet), `clip-playing` (the first
/// playing, two seconds in), `clip-paused` (the first paused, nearly four
/// seconds in), `clip-new` (the composer, with words in it), `clips-keys`
/// (the list has the keyboard's focus).
#[cfg(feature = "screenshot")]
mod pretend {
    use super::*;
    use super_voice_app_core::audio::player::Player;
    use super_voice_app_core::state::{VoiceChoice, VoiceResolution};

    pub fn fill(
        page: &mut ClipsPage,
        scenes: &[&str],
        window: &mut Window,
        cx: &mut Context<ClipsPage>,
    ) {
        page.state.loaded = true;
        let (source, model) = page.synthesis(cx);
        page.voice = VoiceState {
            resolution: Some(VoiceResolution {
                effective: Some("af_heart".into()),
                ..VoiceResolution::default()
            }),
            choices: vec![VoiceChoice {
                id: "af_heart".into(),
                label: "af_heart".into(),
                kind: "preset".into(),
            }],
            target: Some((source, model)),
        };
        if scenes.contains(&"clips-empty") {
            return;
        }
        let today = chrono::Local::now().date_naive();
        let at = |days: u64, time: &str| {
            let day = today - chrono::Days::new(days);
            chrono::NaiveDateTime::new(
                day,
                chrono::NaiveTime::parse_from_str(time, "%H:%M").expect("a time"),
            )
        };
        page.state.items = vec![
            Clip {
                id: "c3".into(),
                created: at(0, "09:40"),
                text: "Welcome to the demo. Everything you hear was spoken on this computer, \
                       by a model running on its own graphics card."
                    .into(),
                voice: Some("af_heart".into()),
                seconds: 6.4,
            },
            Clip {
                id: "c2".into(),
                created: at(1, "17:12"),
                text: "Stand-up moves to 10:30 tomorrow, in the small room.".into(),
                voice: Some("Ryan".into()),
                seconds: 3.1,
            },
            Clip {
                id: "c1".into(),
                created: at(5, "08:05"),
                text: "Chapter one. The lighthouse keeper counted the ships each night, \
                       and wrote their names in a book no one read."
                    .into(),
                voice: Some("Grace".into()),
                seconds: 7.8,
            },
        ];
        let playing = scenes.contains(&"clip-playing");
        let paused = scenes.contains(&"clip-paused");
        if playing || paused || width::content(window) >= width::TWO_PANES {
            page.pane = Pane::Clip("c3".into());
        }
        if playing || paused {
            let mut clip = Playing::new("c3".into());
            // Silence, as long as the clip: what the slider measures.
            clip.audio = Some((vec![0.0; 6_400].into(), 1_000));
            if paused {
                clip.from = Duration::from_millis(3_900);
                clip.paused = true;
            } else {
                clip.player = Some(Player::held_at(
                    Duration::from_secs_f32(6.4),
                    Duration::from_secs(2),
                ));
            }
            page.state.playing = Some(clip);
        }
        if scenes.contains(&"clip-new") {
            page.pane = Pane::New;
            let words = "The library closes early on Friday for the holiday.";
            page.draft
                .update(cx, |draft, cx| draft.set_value(words, window, cx));
            page.state.draft = words.to_string();
        }
        if scenes.contains(&"clips-keys") {
            page.keys.focus(window, cx);
        }
        page.show_position(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    /// Every word the page shows is in the English catalog: a missing key
    /// would show as the key itself.
    #[test]
    fn every_word_is_in_the_catalog() {
        for key in [
            "about-seconds",
            "cancel",
            "change-the-voice",
            "clips",
            "clips-could-not-be-read",
            "clips-count",
            "count-1-word",
            "delete",
            "delete-clip-description",
            "delete-clip-named",
            "delete-clip-ellipsis",
            "each-clip-you-make-shows-here",
            "export",
            "exported-to",
            "loading-progress",
            "make-clip",
            "making-the-clip",
            "clip-actions",
            "new-clip",
            "no-clips-yet",
            "pause",
            "paused-position",
            "play",
            "playing-position",
            "position-in-the-clip",
            "seconds-short",
            "spoken-in",
            "starting",
            "stop",
            "stop-the-clip",
            "text-to-speech",
            "text-to-speech-is-not-ready",
            "the-clip-could-not-be-deleted",
            "the-clip-could-not-be-exported",
            "the-clip-could-not-be-made",
            "the-clip-could-not-play",
            "type-something-and-keep-it-as-speech",
            "type-what-the-clip-should-say",
            "what-the-clip-should-say",
            "words-count",
        ] {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }
}
