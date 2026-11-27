// SPDX-License-Identifier: GPL-3.0-only
//! Text to speech › Behavior: what the Read aloud shortcut reads (Linux), and
//! a test: type something and hear it, see what the daemon is doing and how
//! loud, and on Linux read aloud now the way the shortcut does.
//!
//! Two core pages: the speech test (`pages::speech`) and Read aloud
//! (`pages::read_aloud`). What the test shows follows the daemon's events, so
//! another app's utterance shows here too.
//!
//! Read aloud reads the Wayland session's selection or clipboard, so it is on
//! Linux alone; on macOS the page is the test.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::select::{Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::{Disableable as _, IconName, IndexPath, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, px,
};
use super_voice_app_core::events::{self, Event};
use super_voice_app_core::pages::read_aloud::{self, Notice as ReadNotice};
use super_voice_app_core::pages::speech::{self, Notice as SpeechNotice, Saying};
use super_voice_app_core::state::SpeechState;
use super_voice_shared::models::read_method::ReadMethod;

use crate::effects::{self, CorePage, Runner, runner};
use crate::pages::{Page, PageView, PaletteCommand};
use crate::shared::Shared;
use crate::ui::card::Card;
use crate::ui::page_header::PageHeader;
use crate::ui::settings_row::SettingsRow;
use crate::ui::status::{Dot, StatusPill};
use crate::ui::toast::{self, Toast};
use crate::ui::width;
use crate::{t, t_with};

/// Whether this platform reads aloud: the daemon reads the Wayland session's
/// selection or clipboard.
const READS_ALOUD: bool = cfg!(target_os = "linux");

/// What happened on the page: the speech test's or Read aloud's.
#[derive(Debug, Clone)]
pub enum In {
    Speech(speech::Input),
    Read(read_aloud::Input),
    /// An answer with nothing in it: the daemon took the work.
    Nothing,
}

/// The work either core page asks for.
pub enum Out {
    Speech(speech::Effect),
    Read(read_aloud::Effect),
}

/// One row of the Read from picker.
#[derive(Clone)]
struct ReadFrom(ReadMethod);

impl SelectItem for ReadFrom {
    type Value = ReadMethod;

    fn title(&self) -> SharedString {
        read_method_name(self.0)
    }

    fn value(&self) -> &ReadMethod {
        &self.0
    }
}

fn read_method_name(method: ReadMethod) -> SharedString {
    match method {
        ReadMethod::Selection => t("read-method-selection"),
        ReadMethod::Clipboard => t("read-method-clipboard"),
    }
}

pub struct TtsBehaviorPage {
    speech: SpeechState,
    read_method: ReadMethod,
    text: Entity<InputState>,
    read_from: Entity<SelectState<Vec<ReadFrom>>>,
    _subscriptions: Vec<Subscription>,
}

impl PageView for TtsBehaviorPage {
    fn new(_: Page, _: &Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text =
            cx.new(|cx| InputState::new(window, cx).placeholder(t("type-something-to-hear-it")));
        let read_from = cx.new(|cx| {
            SelectState::new(
                ReadMethod::ALL.into_iter().map(ReadFrom).collect(),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &text,
                window,
                |this, state, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        let value = state.read(cx).value().to_string();
                        this.send(In::Speech(speech::Input::TextChanged(value)), window, cx);
                    }
                    InputEvent::PressEnter { .. } if speech::can_speak(&this.speech) => {
                        this.send(In::Speech(speech::Input::Speak), window, cx);
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &read_from,
                window,
                |this, _, event: &SelectEvent<Vec<ReadFrom>>, window, cx| {
                    let SelectEvent::Confirm(Some(method)) = event else {
                        return;
                    };
                    if *method != this.read_method {
                        this.send(In::Read(read_aloud::Input::Pick(*method)), window, cx);
                    }
                },
            ),
        ];
        Self {
            speech: SpeechState::default(),
            read_method: ReadMethod::default(),
            text,
            read_from,
            _subscriptions: subscriptions,
        }
    }

    fn reached(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if READS_ALOUD {
            self.send(In::Read(read_aloud::Input::Load), window, cx);
        }
    }

    fn heard(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            Event::Speaking {
                is_speaking,
                utterance_id,
            } => self.send(
                In::Speech(speech::Input::Speaking {
                    is_speaking: *is_speaking,
                    utterance_id: utterance_id.clone(),
                }),
                window,
                cx,
            ),
            Event::PlaybackLevel { level, is_speech } => self.send(
                In::Speech(speech::Input::Level {
                    level: *level,
                    is_speech: *is_speech,
                }),
                window,
                cx,
            ),
            // Another client changed what the shortcut reads.
            _ if READS_ALOUD && events::setting_changed(event) == Some("read_method") => {
                self.send(In::Read(read_aloud::Input::Load), window, cx);
            }
            _ => {}
        }
    }

    fn commands(&self, _: &App) -> Vec<PaletteCommand> {
        let feature = t("text-to-speech");
        let mut commands = vec![
            PaletteCommand::new("test-speech", t("test-the-speech"), IconName::Play)
                .detail(feature.clone()),
        ];
        if self.speech.status != super_voice_app_core::state::SpeakingStatus::Idle {
            commands.push(
                PaletteCommand::new("stop-speaking", t("stop-speaking"), IconName::Square)
                    .detail(feature.clone()),
            );
        }
        if READS_ALOUD {
            commands.push(
                PaletteCommand::new("read-aloud-now", t("read-aloud-now"), IconName::BookOpen)
                    .detail(feature),
            );
        }
        commands
    }

    fn run(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            "test-speech" => self.text.update(cx, |text, cx| text.focus(window, cx)),
            "stop-speaking" => self.send(In::Speech(speech::Input::Stop), window, cx),
            "read-aloud-now" => self.send(In::Read(read_aloud::Input::Test), window, cx),
            _ => {}
        }
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, scenes: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        if !scenes.contains(&"speaking") {
            return;
        }
        let words = "Stand-up moves to 10:30 tomorrow, in the small room.";
        self.text
            .update(cx, |text, cx| text.set_value(words, window, cx));
        self.speech.test_text = words.to_string();
        speech::update(
            &mut self.speech,
            speech::Input::Speaking {
                is_speaking: true,
                utterance_id: Some("u1".into()),
            },
        );
        speech::update(
            &mut self.speech,
            speech::Input::Level {
                level: 0.62,
                is_speech: true,
            },
        );
    }
}

impl CorePage for TtsBehaviorPage {
    type Input = In;
    type Effect = Out;

    fn send(&mut self, input: In, window: &mut Window, cx: &mut Context<Self>) {
        match input {
            In::Speech(input) => {
                let outcome = speech::update(&mut self.speech, input);
                effects::run_effects(
                    self,
                    outcome.effects.into_iter().map(Out::Speech),
                    window,
                    cx,
                );
                toast::notice::<SpeechNotice>(
                    window,
                    cx,
                    outcome.clear_notice,
                    outcome.notice.map(|notice| match notice {
                        SpeechNotice::CouldNotSpeak(e) => Toast::error(e),
                    }),
                );
            }
            In::Read(input) => {
                let before = self.read_method;
                let outcome = read_aloud::update(&mut self.read_method, input);
                if self.read_method != before {
                    self.show_read_method(window, cx);
                }
                effects::run_effects(self, outcome.effects.into_iter().map(Out::Read), window, cx);
                toast::notice::<ReadNotice>(
                    window,
                    cx,
                    outcome.clear_notice,
                    outcome.notice.map(read_notice),
                );
            }
            In::Nothing => {}
        }
        cx.notify();
    }

    fn effect(&mut self, effect: Out, _: &mut Window, _: &mut Context<Self>) -> Option<Runner<In>> {
        Some(match effect {
            Out::Speech(speech::Effect::Speak(text)) => {
                runner(async move { In::Speech(speech::run_speak(text).await) })
            }
            Out::Speech(speech::Effect::Stop) => runner(async {
                speech::run_stop().await;
                In::Nothing
            }),
            Out::Read(read_aloud::Effect::Load) => {
                runner(async { In::Read(read_aloud::run_load().await) })
            }
            Out::Read(read_aloud::Effect::Save(method)) => {
                runner(async move { In::Read(read_aloud::run_save(method).await) })
            }
            Out::Read(read_aloud::Effect::Test) => {
                runner(async { read_aloud::run_test().await.map_or(In::Nothing, In::Read) })
            }
        })
    }
}

/// What Read aloud tells the user, in the app's words.
fn read_notice(notice: ReadNotice) -> Toast {
    match notice {
        ReadNotice::CouldNotSave(e) => {
            Toast::error(t_with("read-method-could-not-save", &[("error", &e)]))
        }
        ReadNotice::NothingSelected => Toast::warning(t("read-nothing-selection")),
        ReadNotice::NothingInClipboard => Toast::warning(t("read-nothing-clipboard")),
        ReadNotice::TooLong => Toast::warning(t("read-too-much")),
        ReadNotice::CouldNotRead(e) => Toast::error(t_with("read-failed", &[("error", &e)])),
    }
}

impl TtsBehaviorPage {
    /// Show the stored read method in the picker.
    fn show_read_method(&self, window: &mut Window, cx: &mut Context<Self>) {
        let method = self.read_method;
        self.read_from.update(cx, |select, cx| {
            select.set_selected_value(&method, window, cx);
        });
    }

    /// Where the Read aloud shortcut reads from.
    fn read_aloud(&self) -> impl IntoElement {
        Card::new().title(t("read-aloud-section")).child(
            SettingsRow::new(t("read-from"))
                .description(t("read-from-description"))
                .control(
                    Select::new(&self.read_from)
                        .w(px(200.))
                        .accessibility_label(t("read-from")),
                ),
        )
    }

    /// The test: text to speak and Speak or Stop, what the daemon is doing
    /// and how loud, and on Linux, Read now.
    fn test(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let saying = speech::saying(&self.speech);
        let (dot, words) = match saying {
            Saying::Nothing => (Dot::Grey, "speech-not-speaking"),
            Saying::Starting => (Dot::Yellow, "speech-starting"),
            Saying::Speaking => (Dot::Green, "speech-speaking"),
        };
        // Speak is off on empty text rather than sending it: the daemon
        // refuses an empty utterance. While anything speaks, Stop.
        let action = if saying == Saying::Nothing {
            Button::new("speak")
                .primary()
                .icon(IconName::Play)
                .label(t("speak"))
                .disabled(!speech::can_speak(&self.speech))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.send(In::Speech(speech::Input::Speak), window, cx);
                }))
        } else {
            Button::new("stop-speaking")
                .danger()
                .icon(IconName::Square)
                .label(t("stop"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.send(In::Speech(speech::Input::Stop), window, cx);
                }))
        };
        // A floor under any sound at all, so quiet speech still moves it.
        let level = self.speech.level;
        let level = if level > 0.0 { level.max(0.1) } else { 0.0 };
        let mut card = Card::new()
            .title(t("test"))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.text).aria_label(t("text"))),
                    )
                    .child(action),
            )
            .child(
                v_flex()
                    .child(
                        SettingsRow::new(t("status"))
                            .inline()
                            .control(StatusPill::new(dot, t(words))),
                    )
                    .child(
                        SettingsRow::new(t("speech-output-level")).control(
                            Progress::new("output-level")
                                .small()
                                .w(px(200.))
                                .value(level * 100.)
                                .accessibility_label(t("speech-output-level")),
                        ),
                    ),
            );
        if READS_ALOUD {
            let hint = match self.read_method {
                ReadMethod::Selection => t("read-test-selection"),
                ReadMethod::Clipboard => t("read-test-clipboard"),
            };
            // The speech test's Stop stops a reading too.
            card = card.child(
                SettingsRow::new(t("read-test")).description(hint).control(
                    Button::new("read-now")
                        .icon(IconName::BookOpen)
                        .label(t("read"))
                        .disabled(saying != Saying::Nothing)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.send(In::Read(read_aloud::Input::Test), window, cx);
                        })),
                ),
            );
        }
        card
    }
}

impl Render for TtsBehaviorPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let padding = width::page_padding(window);
        let description = if READS_ALOUD {
            t("tts-behavior-description")
        } else {
            t("tts-behavior-description-test-only")
        };
        v_flex()
            .size_full()
            .child(PageHeader::new(Page::TtsBehavior).description(description))
            .child(
                div()
                    .id("tts-behavior")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(720.))
                            .px(padding)
                            .pt_2()
                            .pb_6()
                            .gap_4()
                            .children(READS_ALOUD.then(|| self.read_aloud()))
                            .child(self.test(cx)),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    /// Every word the page shows is in the English catalog: a missing key
    /// would show as the key itself.
    #[test]
    fn every_word_is_in_the_catalog() {
        for key in [
            "read",
            "read-failed",
            "read-aloud-now",
            "read-aloud-section",
            "read-from",
            "read-from-description",
            "read-method-clipboard",
            "read-method-could-not-save",
            "read-method-selection",
            "read-nothing-clipboard",
            "read-nothing-selection",
            "read-test",
            "read-test-clipboard",
            "read-test-selection",
            "read-too-much",
            "speak",
            "speech-not-speaking",
            "speech-output-level",
            "speech-speaking",
            "speech-starting",
            "status",
            "stop",
            "stop-speaking",
            "test",
            "test-the-speech",
            "text",
            "text-to-speech",
            "tts-behavior-description",
            "tts-behavior-description-test-only",
            "type-something-to-hear-it",
        ] {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }
}
