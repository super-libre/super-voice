// SPDX-License-Identifier: GPL-3.0-only
//! Recording, on speech to text's Behavior tab: when a recording stops,
//! whether its text is typed as it comes, the sounds it plays, and the test
//! that records.
use cosmic::iced::widget::row;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, settings, space::horizontal as horizontal_space, text};
use cosmic::{Apply, Element};
use super_voice_shared::models::protocol::PreviewSource;
use super_voice_shared::models::recording_stop_mode::RecordingStopMode;
use super_voice_shared::theme::AudioTheme;

use crate::state::RecordingStatus;
use super_voice_app_core::pages::behavior::Input;

use crate::ui::messages::{BehaviorMessage, Message, RecordingMessage};
use crate::ui::press_guard::press_guard;

/// Recording section: stop mode + preview typing.
pub(super) fn settings_section(
    recording_stop_mode: RecordingStopMode,
    preview_typing_enabled: bool,
) -> Element<'static, Message> {
    let modes = [
        RecordingStopMode::SilenceOnly,
        RecordingStopMode::SilenceAndManual,
        RecordingStopMode::ManualOnly,
    ];
    let mode_names: Vec<String> = modes
        .iter()
        .map(|m| crate::i18n::recording_stop_mode_name(*m))
        .collect();
    let selected_index = modes.iter().position(|m| *m == recording_stop_mode);

    settings::section()
        .title(crate::fl!("recording"))
        .add(
            settings::item::builder(crate::fl!("stop-mode"))
                .description(crate::fl!("controls-how-to-stop-transcribing"))
                .control(widget::dropdown(mode_names, selected_index, move |index| {
                    Message::Behavior(BehaviorMessage::Page(Input::StopModeChanged(modes[index])))
                })),
        )
        .add(
            settings::item::builder(crate::fl!("preview-typing"))
                .description(crate::fl!(
                    "shows-transcription-as-you-speak-experimental-may"
                ))
                .control(press_guard(
                    cosmic::widget::toggler(preview_typing_enabled).on_toggle(|b| {
                        Message::Behavior(BehaviorMessage::Page(Input::PreviewTypingToggled(b)))
                    }),
                )),
        )
        .into()
}

/// Sounds section: whether a recording plays a sound as it starts and stops,
/// which, and how loud. Only a recording plays them.
pub(super) fn sounds_section<'a>(
    audio_themes: &'a [AudioTheme],
    selected_audio_theme: &'a AudioTheme,
    volume: u8,
) -> Element<'a, Message> {
    let audio_enabled = *selected_audio_theme != AudioTheme::Silent;

    // Filter Silent out of the dropdown — it's controlled by the toggle
    let non_silent_themes: Vec<&AudioTheme> = audio_themes
        .iter()
        .filter(|t| **t != AudioTheme::Silent)
        .collect();
    let theme_names: Vec<String> = non_silent_themes
        .iter()
        .map(|t| crate::i18n::audio_theme_name(**t))
        .collect();
    let selected_index = non_silent_themes
        .iter()
        .position(|t| *t == selected_audio_theme);

    let mut section = settings::section().title(crate::fl!("sounds")).add(
        settings::item::builder(crate::fl!("audio-feedback"))
            .description(crate::fl!("play-a-sound-when-a-recording-starts-and-stops"))
            .control(press_guard(
                cosmic::widget::toggler(audio_enabled).on_toggle(|b| {
                    Message::Behavior(BehaviorMessage::Page(Input::SoundsToggled(b)))
                }),
            )),
    );

    if audio_enabled {
        let non_silent_clone: Vec<AudioTheme> =
            non_silent_themes.iter().copied().copied().collect();
        let theme_control: Element<'a, Message> = if non_silent_themes.is_empty() {
            text::caption(crate::fl!("loading-themes")).into()
        } else {
            widget::dropdown(theme_names, selected_index, move |index| {
                if let Some(&theme) = non_silent_clone.get(index) {
                    Message::Behavior(BehaviorMessage::Page(Input::ThemePicked(theme)))
                } else {
                    Message::Behavior(BehaviorMessage::Page(Input::ThemePicked(
                        AudioTheme::Classic,
                    )))
                }
            })
            .into()
        };

        // Drag ticks update the local value; the daemon POST fires once on
        // release (Tier 1 #19), so a single drag isn't hundreds of set_volume
        // requests.
        let slider = widget::slider(0..=100, volume, |v| {
            Message::Behavior(BehaviorMessage::Page(Input::VolumeChanged(v)))
        })
        .on_release(Message::Behavior(BehaviorMessage::Page(
            Input::VolumeCommitted,
        )))
        .width(Length::Fill)
        .apply(widget::container)
        .max_width(super::common::LEVEL_METER_WIDTH);

        let volume_control = widget::row::with_capacity(3)
            .align_y(Alignment::Center)
            .push(
                text::body(format!("{volume}%"))
                    .width(Length::Fixed(32.0))
                    .align_x(Alignment::Center),
            )
            .push(horizontal_space().width(8.))
            .push(slider);

        section = section
            .add(
                settings::item::builder(crate::fl!("theme"))
                    .description(crate::fl!("the-sounds-a-recording-plays"))
                    .control(theme_control),
            )
            .add(
                settings::item::builder(crate::fl!("volume"))
                    .flex_control(volume_control)
                    .align_items(Alignment::Center),
            );
    }

    section.into()
}

/// How much of the live preview to keep on screen. Sized to fill the row
/// without wrapping at the default window width.
const LIVE_LINE_CHARS: usize = 90;

/// Height of the transcript box. Three lines of body text, so a short test
/// transcript reads without scrolling and a long one still has a fixed size.
const TRANSCRIPT_HEIGHT: f32 = 66.0;

/// The last `max_chars` characters of `text`, prefixed with an ellipsis when
/// something was dropped. Counts chars, not bytes — a multibyte transcript must
/// not be sliced mid-character.
fn live_tail(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    let tail: String = text.chars().skip(total - max_chars).collect();
    format!("\u{2026}{tail}")
}

/// What the Live row says the text is, for each kind of preview.
///
/// A window preview restarts every few seconds, which looks like text being
/// lost unless the row says it only ever shows the last few seconds.
fn live_caption(source: PreviewSource) -> String {
    match source {
        PreviewSource::Stream => crate::fl!("everything-said-so-far"),
        PreviewSource::Window => crate::fl!("the-last-few-seconds"),
    }
}

/// The Live row: the newest incremental text, held to one line so the section
/// doesn't reflow on every update, with what it spans under the row's name.
///
/// The tail is the part worth showing either way, but what the text *is*
/// depends on the model: a stream preview is the whole take so far, a window
/// preview only the last few seconds. That goes in the row's description,
/// not beside the words, where it read as the first words of the transcript.
pub(super) fn live_row(preview: &str, source: Option<PreviewSource>) -> Element<'_, Message> {
    let content = if preview.is_empty() {
        "\u{2014}".to_string()
    } else {
        live_tail(preview, LIVE_LINE_CHARS)
    };
    let line = widget::container(
        text::body(content)
            .wrapping(cosmic::iced::widget::text::Wrapping::None)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .clip(true);
    let mut item = settings::item::builder(crate::fl!("live"));
    if let Some(caption) = source.map(live_caption) {
        item = item.description(caption);
    }
    item.flex_control(line).into()
}

/// The recording test's rows, for the Behavior tab's one Test section: the
/// status with the button that starts and stops the take, the audio level,
/// the live preview line, and the transcript.
pub(super) fn test_rows<'a>(
    recording_status: &'a RecordingStatus,
    transcription_text: &'a str,
    preview_text: &'a str,
    preview_source: Option<PreviewSource>,
    audio_level: f32,
    is_speech_detected: bool,
) -> Vec<Element<'a, Message>> {
    let recording_text = match recording_status {
        RecordingStatus::Recording => {
            if is_speech_detected {
                crate::fl!("speech")
            } else {
                crate::fl!("silence")
            }
        }
        RecordingStatus::Idle => crate::fl!("not-recording"),
    };

    let record_button = match recording_status {
        RecordingStatus::Recording => button::destructive(crate::fl!("stop-recording"))
            .on_press(Message::Recording(RecordingMessage::StopRecording)),
        RecordingStatus::Idle => button::suggested(crate::fl!("test-recording"))
            .on_press(Message::Recording(RecordingMessage::StartRecording)),
    };

    // The button beside the state it changes, and the meter on a line of its
    // own, where a level reads as a level rather than as part of the button.
    let status_widget = row![text::body(recording_text), record_button]
        .align_y(Alignment::Center)
        .spacing(cosmic::theme::spacing().space_s);
    let audio_meter =
        widget::determinate_linear(audio_level.max(if audio_level > 0.0 { 0.1 } else { 0.0 }))
            .width(Length::Fixed(super::common::LEVEL_METER_WIDTH));

    // The transcript has no label, so it goes into the section as a plain list
    // item. Routing it through `flex_item("", ...)` instead gave the empty
    // label a flex slot of its own, which grew the row and left the text
    // sitting against the bottom of it with a gap above.
    let transcription_widget = {
        let content = if transcription_text.is_empty() {
            crate::fl!("transcriptions-will-appear-here-after-test-recordings")
        } else {
            transcription_text.to_string()
        };

        widget::scrollable(text::body(content).width(Length::Fill))
            .height(Length::Fixed(TRANSCRIPT_HEIGHT))
            .width(Length::Fill)
    };

    vec![
        settings::item(crate::fl!("status"), status_widget).into(),
        settings::item(crate::fl!("audio-level"), audio_meter).into(),
        live_row(preview_text, preview_source),
        transcription_widget.into(),
    ]
}
