// SPDX-License-Identifier: GPL-3.0-only
//! Speech to text › Behavior: what stops a recording and whether its words
//! are typed while heard, how they are typed, the sounds around a recording
//! and their volume, and the tests: a recording through the daemon (its
//! microphone live, its words as they come, then its transcript), typing into
//! a field here, and typing into another window after a countdown.
//!
//! What a change does is the core's (`pages::behavior`, and
//! `pages::recording_test` for the test); a setting shows once the daemon has
//! stored it, except the sounds, which show at once.

use std::future::Future;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants as _};
use gpui_kit::component::input::{Input as TextInput, InputEvent, InputState};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, IndexPath, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, StyleRefinement, Styled as _, Subscription, Window, div, px,
};
use super_voice_app_core::events::{Event, setting_changed};
use super_voice_app_core::pages::behavior::{self as page, Effect, Input};
use super_voice_app_core::pages::recording_test as test;
use super_voice_app_core::state::behavior::BehaviorState;
use super_voice_app_core::state::recording_test::RecordingTest;
use super_voice_app_core::state::{AudioTheme, Microphone};
use super_voice_shared::models::write_method::WriteMethod;

use crate::effects::{self, CorePage, Runner, runner};
use crate::icons::Phosphor;
use crate::pages::{Page, PageView, PaletteCommand, take};
use crate::shared::Shared;
use crate::ui::page_header::PageHeader;
use crate::ui::settings_row::SettingsRow;
use crate::ui::toast::{self, Toast};
use crate::ui::waveform::{Levels, Waveform};
use crate::ui::width;
use crate::{runtime, t, t_with};

/// The page's widest.
const MAX_WIDTH: Pixels = px(820.);
/// A group's border and padding, which its rows are narrower by.
const GROUP_INSET: Pixels = px(34.);
/// A Select's width beside its text.
const CONTROL: Pixels = px(280.);

type Choices = SelectState<Vec<SharedString>>;

/// The Behavior page of speech to text.
pub struct BehaviorPage {
    state: BehaviorState,
    test: RecordingTest,
    microphone: Microphone,
    levels: Levels,
    stop_mode: Entity<Choices>,
    write_method: Entity<Choices>,
    methods: Vec<WriteMethod>,
    theme: Entity<Choices>,
    /// The themes the Theme picker lists, in its order.
    themes: Vec<AudioTheme>,
    volume: Entity<SliderState>,
    typing: Entity<InputState>,
    /// The recording test's take, while one records.
    take: Option<take::Take>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl PageView for BehaviorPage {
    fn new(_: Page, _: &Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = BehaviorState::default();
        let stop_modes: Vec<SharedString> = page::STOP_MODES
            .iter()
            .map(|m| t(page::stop_mode_key(*m)))
            .collect();
        let methods = page::write_methods();
        let method_names: Vec<SharedString> = methods
            .iter()
            .map(|m| page::write_method_name(*m).into())
            .collect();
        let stop_mode = cx.new(|cx| SelectState::new(stop_modes, None, window, cx));
        let write_method = cx.new(|cx| SelectState::new(method_names, None, window, cx));
        let theme = cx.new(|cx| SelectState::new(Vec::new(), None, window, cx));
        let volume = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(f32::from(state.volume))
        });
        let typing = cx
            .new(|cx| InputState::new(window, cx).placeholder(t("typed-text-lands-here-progress")));
        let subscriptions = Self::subscribe(
            &stop_mode,
            &write_method,
            &theme,
            &volume,
            &typing,
            window,
            cx,
        );
        let mut page = Self {
            state,
            test: RecordingTest::default(),
            microphone: Microphone::default(),
            levels: Levels::default(),
            stop_mode,
            write_method,
            methods,
            theme,
            themes: Vec::new(),
            volume,
            typing,
            take: None,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        page.sync_controls(window, cx);
        page
    }

    fn reached(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // What the test showed belonged to a session that may be gone.
        self.send_test(test::Input::Reconnected, window, cx);
        Self::read(page::run_load(), window, cx);
    }

    fn heard(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
        // Another client changed a setting, or this one did: read it again.
        match setting_changed(event) {
            Some("recording_stop_mode") => Self::read_one(page::run_load_stop_mode(), window, cx),
            Some("preview_typing") => Self::read_one(page::run_load_preview_typing(), window, cx),
            Some("write_method") => Self::read_one(page::run_load_write_method(), window, cx),
            Some("audio_theme") => Self::read_one(page::run_load_theme(), window, cx),
            Some("volume") => Self::read_one(page::run_load_volume(), window, cx),
            _ => {}
        }
        if self.microphone.follow(event) {
            match *event {
                Event::Recording(true) => self.levels.clear(),
                Event::MicrophoneLevel { level, .. } if self.microphone.recording => {
                    self.levels.push(level);
                }
                _ => {}
            }
            cx.notify();
        }
    }

    fn commands(&self, _: &App) -> Vec<PaletteCommand> {
        vec![PaletteCommand::new(
            "test-recording",
            t("test-recording"),
            Phosphor::Microphone,
        )]
    }

    fn run(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if id == "test-recording" {
            self.send_test(test::Input::Start, window, cx);
        }
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, scenes: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        pretend::fill(self, scenes, window, cx);
    }
}

impl BehaviorPage {
    /// What the page hears from its controls.
    fn subscribe(
        stop_mode: &Entity<Choices>,
        write_method: &Entity<Choices>,
        theme: &Entity<Choices>,
        volume: &Entity<SliderState>,
        typing: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        vec![
            cx.subscribe_in(stop_mode, window, |this, _, event, window, cx| {
                let SelectEvent::Confirm(Some(name)) = event else {
                    return;
                };
                let picked = page::STOP_MODES
                    .into_iter()
                    .find(|m| t(page::stop_mode_key(*m)) == *name);
                if let Some(mode) = picked.filter(|m| *m != this.state.stop_mode) {
                    this.send(Input::StopModeChanged(mode), window, cx);
                }
            }),
            cx.subscribe_in(write_method, window, |this, _, event, window, cx| {
                let SelectEvent::Confirm(Some(name)) = event else {
                    return;
                };
                let picked = this
                    .methods
                    .iter()
                    .copied()
                    .find(|m| page::write_method_name(*m) == name.as_ref());
                if let Some(method) = picked.filter(|m| *m != this.state.write_method) {
                    this.send(Input::WriteMethodChanged(method), window, cx);
                }
            }),
            cx.subscribe_in(theme, window, |this, _, event, window, cx| {
                let SelectEvent::Confirm(Some(name)) = event else {
                    return;
                };
                let picked = this
                    .themes
                    .iter()
                    .copied()
                    .find(|theme| t(page::theme_key(*theme)) == *name);
                if let Some(theme) = picked.filter(|theme| *theme != this.state.audio_theme) {
                    this.send(Input::ThemePicked(theme), window, cx);
                }
            }),
            cx.subscribe_in(
                volume,
                window,
                |this, _, event: &SliderEvent, window, cx| {
                    let input = match event {
                        SliderEvent::Change(value) => Input::VolumeChanged(percent(value.start())),
                        SliderEvent::Release(value) => {
                            this.send(Input::VolumeChanged(percent(value.start())), window, cx);
                            Input::VolumeCommitted
                        }
                    };
                    this.send(input, window, cx);
                },
            ),
            cx.subscribe_in(
                typing,
                window,
                |this, typing, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        let text = typing.read(cx).value().to_string();
                        if text != this.state.typing_test_text {
                            this.send(Input::TypingTestTextChanged(text), window, cx);
                        }
                    }
                },
            ),
        ]
    }
}

/// A slider's value as a volume, 0 to 100.
fn percent(value: f32) -> u8 {
    // reason: clamped to 0..=100 first, so it fits.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let volume = value.clamp(0., 100.).round() as u8;
    volume
}

impl CorePage for BehaviorPage {
    type Input = Input;
    type Effect = Effect;

    fn send(&mut self, input: Input, window: &mut Window, cx: &mut Context<Self>) {
        let outcome = page::update(&mut self.state, input);
        self.sync_controls(window, cx);
        effects::run_effects(self, outcome.effects, window, cx);
        toast::notice::<Self>(
            window,
            cx,
            outcome.clear_notice.is_some(),
            outcome
                .notice
                .map(|notice| Toast::error(page::notice_text(&notice))),
        );
        cx.notify();
    }

    fn effect(
        &mut self,
        effect: Effect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Runner<Input>> {
        match effect {
            Effect::SaveStopMode(mode) => Some(runner(page::run_save_stop_mode(mode))),
            Effect::SavePreviewTyping(on) => Some(runner(page::run_save_preview_typing(on))),
            Effect::SaveWriteMethod(method) => Some(runner(page::run_save_write_method(method))),
            Effect::Tick => Some(runner(page::run_tick())),
            Effect::TestTyping { into_field } => {
                // The daemon types into whatever has focus: the field, not
                // the button just pressed.
                if into_field {
                    self.typing.update(cx, |field, cx| field.focus(window, cx));
                }
                Some(runner(page::run_test_typing()))
            }
            Effect::SaveTheme {
                chosen,
                play,
                theme,
                sounding,
            } => {
                Self::read_maybe(
                    page::run_save_theme(chosen, play, theme, sounding),
                    window,
                    cx,
                );
                None
            }
            Effect::SaveVolume { volume, stored } => {
                Self::read_maybe(page::run_save_volume(volume, stored), window, cx);
                None
            }
        }
    }
}

impl BehaviorPage {
    /// Run `work` on the core's runtime and take in each input it answers.
    fn read(
        work: impl Future<Output = Vec<Input>> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            let Ok(inputs) = runtime::spawn(work).await else {
                return;
            };
            _ = this.update_in(cx, |page, window, cx| {
                for input in inputs {
                    page.send(input, window, cx);
                }
            });
        })
        .detach();
    }

    fn read_one(
        work: impl Future<Output = Input> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Self::read(async move { vec![work.await] }, window, cx);
    }

    fn read_maybe(
        work: impl Future<Output = Option<Input>> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Self::read(async move { work.await.into_iter().collect() }, window, cx);
    }

    /// Take in the recording test's `input`.
    fn send_test(&mut self, input: test::Input, window: &mut Window, cx: &mut Context<Self>) {
        let ended = matches!(input, test::Input::Transcribed(_));
        for effect in test::update(&mut self.test, &mut self.microphone, input) {
            match effect {
                test::Effect::Take => {
                    self.levels.clear();
                    self.take = Some(take::read(
                        test::take(),
                        window,
                        cx,
                        |page, input, window, cx| {
                            page.send_test(input, window, cx);
                        },
                    ));
                }
                test::Effect::Stop => drop(runtime::spawn(test::run_stop())),
            }
        }
        if ended {
            self.take = None;
        }
        cx.notify();
    }

    /// Show the core's values in the controls that hold their own.
    fn sync_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stop = page::STOP_MODES
            .iter()
            .position(|m| *m == self.state.stop_mode);
        select(&self.stop_mode, stop, window, cx);
        let method = self
            .methods
            .iter()
            .position(|m| *m == self.state.write_method);
        select(&self.write_method, method, window, cx);
        let themes = page::sounding_themes(&self.state);
        if themes != self.themes {
            let names: Vec<SharedString> =
                themes.iter().map(|th| t(page::theme_key(*th))).collect();
            self.theme
                .update(cx, |select, cx| select.set_items(names, window, cx));
            self.themes = themes;
        }
        let sounding = self
            .themes
            .iter()
            .position(|th| *th == self.state.sounding_theme);
        select(&self.theme, sounding, window, cx);
        let volume = f32::from(self.state.volume);
        if (self.volume.read(cx).value().start() - volume).abs() >= 0.5 {
            self.volume
                .update(cx, |slider, cx| slider.set_value(volume, window, cx));
        }
        let text = self.state.typing_test_text.clone();
        if self.typing.read(cx).value().as_ref() != text {
            self.typing
                .update(cx, |field, cx| field.set_value(text, window, cx));
        }
    }
}

/// Pick row `row` in `choices`, if it isn't picked already.
fn select(choices: &Entity<Choices>, row: Option<usize>, window: &mut Window, cx: &mut App) {
    let index = row.map(|row| IndexPath::default().row(row));
    if choices.read(cx).selected_index(cx) != index {
        choices.update(cx, |select, cx| {
            select.set_selected_index(index, window, cx);
        });
    }
}

impl Render for BehaviorPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let padding = width::page_padding(window);
        let row = (width::content(window) - padding * 2.).min(MAX_WIDTH) - GROUP_INSET;
        let stacked = width::row_stacks(row);
        v_flex()
            .size_full()
            .child(PageHeader::new(Page::SttBehavior).description(t("behavior-description")))
            .child(
                div()
                    .id("stt-behavior")
                    .flex_1()
                    .min_h_0()
                    .track_scroll(&self.scroll)
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(MAX_WIDTH)
                            .px(padding)
                            .pt_2()
                            .pb_6()
                            .gap_6()
                            .child(self.recording(row, stacked, cx))
                            .child(self.typing_group(row, stacked, cx))
                            .child(self.sounds(row, stacked, cx))
                            .child(self.tests(row, stacked, cx)),
                    ),
            )
    }
}

/// A group of rows, as the Settings page draws one: a title over an
/// outlined box, the rows close together.
fn group(id: &'static str, title: SharedString) -> GroupBox {
    GroupBox::new().id(id).outline().title(title).content_style(
        StyleRefinement::default()
            .px(px(16.))
            .py(px(4.))
            .gap(px(0.)),
    )
}

impl BehaviorPage {
    /// A control's width: a fixed one beside its text, the row's when under
    /// it.
    fn sized(control: impl IntoElement, stacked: bool) -> AnyElement {
        div()
            .when(stacked, gpui_kit::Styled::w_full)
            .when(!stacked, |d| d.w(CONTROL))
            .child(control)
            .into_any_element()
    }

    fn recording(&self, row: Pixels, stacked: bool, cx: &mut Context<Self>) -> impl IntoElement {
        group("recording", t("recording"))
            .child(
                SettingsRow::new(t("stop-mode"))
                    .description(t("controls-how-to-stop-transcribing"))
                    .width(row)
                    .control(Self::sized(Select::new(&self.stop_mode), stacked)),
            )
            .child(
                SettingsRow::new(t("preview-typing"))
                    .description(t("shows-transcription-as-you-speak-experimental-may"))
                    .width(row)
                    .inline()
                    .control(
                        Switch::new("preview-typing")
                            .checked(self.state.preview_typing)
                            .accessibility_label(t("preview-typing"))
                            .on_click(cx.listener(|this, on: &bool, window, cx| {
                                this.send(Input::PreviewTypingToggled(*on), window, cx);
                            })),
                    ),
            )
    }

    fn typing_group(&self, row: Pixels, stacked: bool, _: &mut Context<Self>) -> impl IntoElement {
        group("typing", t("typing"))
            .child(
                SettingsRow::new(t("write-method"))
                    .description(t("controls-how-transcribed-text-is-typed-into"))
                    .width(row)
                    .control(Self::sized(Select::new(&self.write_method), stacked)),
            )
            // Auto names a chain, not a method: only a test says which one
            // typed.
            .when_some(self.state.typed_with, |group, method| {
                group.child(
                    SettingsRow::new(t("active-backend"))
                        .description(t("what-the-last-test-actually-typed-through"))
                        .width(row)
                        .inline()
                        .control(div().text_sm().child(page::write_method_name(method))),
                )
            })
    }

    fn sounds(&self, row: Pixels, stacked: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let on = self.state.sounds_on();
        let volume = h_flex()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .child(Slider::new(&self.volume).accessibility_label(t("volume"))),
            )
            .child(
                div()
                    .w(px(40.))
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{}%", self.state.volume)),
            );
        group("sounds", t("sounds"))
            .child(
                SettingsRow::new(t("audio-feedback"))
                    .description(t("play-a-sound-when-a-recording-starts-and-stops"))
                    .width(row)
                    .inline()
                    .control(
                        Switch::new("sounds")
                            .checked(on)
                            .accessibility_label(t("audio-feedback"))
                            .on_click(cx.listener(|this, on: &bool, window, cx| {
                                this.send(Input::SoundsToggled(*on), window, cx);
                            })),
                    ),
            )
            .when(on, |group| {
                group
                    .child(
                        SettingsRow::new(t("theme"))
                            .description(t("the-sounds-a-recording-plays"))
                            .width(row)
                            .control(Self::sized(Select::new(&self.theme), stacked)),
                    )
                    .child(
                        SettingsRow::new(t("volume"))
                            .width(row)
                            .control(Self::sized(volume, stacked)),
                    )
            })
    }

    fn tests(&self, row: Pixels, stacked: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let countdown: AnyElement = match self.state.typing_countdown {
            Some(seconds) => h_flex()
                .gap_3()
                .child(div().text_sm().text_color(muted).child(t_with(
                    "typing-countdown",
                    &[("secs", &seconds.to_string())],
                )))
                .child(
                    Button::new("cancel-countdown")
                        .danger()
                        .label(t("cancel"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.send(Input::CancelTypingTest, window, cx);
                        })),
                )
                .into_any_element(),
            None => Button::new("start-countdown")
                .label(t("start-countdown"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.send(Input::TestTypingLater, window, cx);
                }))
                .into_any_element(),
        };
        let typing = h_flex()
            .gap_2()
            .when(!stacked, |r| r.w(px(360.)))
            .child(div().flex_1().child(TextInput::new(&self.typing)))
            .child(
                Button::new("type-test-text")
                    .label(t("type-test-text"))
                    .disabled(self.state.typing_countdown.is_some())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.send(Input::TestTyping, window, cx);
                    })),
            );
        group("test", t("test"))
            .child(self.recording_test(row, cx))
            .child(
                SettingsRow::new(t("typing-test"))
                    .description(t("types-a-short-string-into-the-field"))
                    .width(row)
                    .control(typing),
            )
            .child(
                SettingsRow::new(t("test-another-window"))
                    .description(t("counts-down-before-typing-so-you-can"))
                    .width(row)
                    .control(countdown),
            )
    }

    /// The recording test: Test recording or Stop, what the microphone
    /// hears, its words as they come, then its transcript.
    fn recording_test(&self, row: Pixels, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, surface, radius) = (theme.muted_foreground, theme.muted, theme.radius);
        let recording = self.microphone.recording;
        let testing = self.take.is_some() || recording;
        let record = if testing {
            Button::new("test-recording")
                .danger()
                .icon(Phosphor::Stop)
                .label(t("stop-recording"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.send_test(test::Input::Stop, window, cx);
                }))
        } else {
            Button::new("test-recording")
                .primary()
                .icon(Phosphor::Microphone)
                .label(t("test-recording"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.send_test(test::Input::Start, window, cx);
                }))
        };
        let live =
            (!self.test.preview.is_empty()).then(|| {
                v_flex()
                    .gap_0p5()
                    .child(div().text_xs().text_color(muted).child(
                        match self.test.preview_source {
                            Some(source) => t(test::spans_key(source)),
                            None => t("live"),
                        },
                    ))
                    .child(div().text_sm().child(self.test.preview.clone()))
            });
        let heard = self.test.heard.as_ref();
        let transcript = div()
            .id("test-transcript")
            .w_full()
            .min_h(px(64.))
            .max_h(px(120.))
            .overflow_y_scroll()
            .p_3()
            .rounded(radius)
            .bg(surface)
            .text_sm()
            .when(heard.is_none(), |box_| box_.text_color(muted))
            .child(test::heard_text(heard));
        v_flex()
            .gap_3()
            .pb_3()
            .child(
                SettingsRow::new(t("test-recording"))
                    .description(t(test::hearing_key(&self.microphone)))
                    .width(row)
                    .inline()
                    .control(record),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(Waveform::new(&self.levels).live(recording))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(muted)
                            .child(t("audio-level")),
                    ),
            )
            .children(live)
            .child(transcript)
    }
}

#[cfg(feature = "screenshot")]
mod pretend;

#[cfg(test)]
mod tests {
    /// Every word the page shows is in the English catalog.
    #[test]
    fn every_word_is_in_the_catalog() {
        for key in [
            "active-backend",
            "audio-feedback",
            "audio-level",
            "behavior-description",
            "cancel",
            "controls-how-to-stop-transcribing",
            "controls-how-transcribed-text-is-typed-into",
            "counts-down-before-typing-so-you-can",
            "live",
            "play-a-sound-when-a-recording-starts-and-stops",
            "preview-typing",
            "recording",
            "shows-transcription-as-you-speak-experimental-may",
            "sounds",
            "start-countdown",
            "stop-mode",
            "stop-recording",
            "test",
            "test-another-window",
            "test-recording",
            "the-sounds-a-recording-plays",
            "theme",
            "type-test-text",
            "typed-text-lands-here-progress",
            "types-a-short-string-into-the-field",
            "typing",
            "typing-countdown",
            "typing-test",
            "volume",
            "what-the-last-test-actually-typed-through",
            "write-method",
        ] {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }
}
