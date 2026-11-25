// SPDX-License-Identifier: GPL-3.0-only
//! The Clips tab: a card to type the next clip into and make it, in the voice
//! text to speech uses; then the clips, newest first, each to play, export or
//! delete.

use chrono::NaiveDate;
use cosmic::Element;
use cosmic::iced::widget::{column, row};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, settings, space::horizontal as horizontal_space, text};

use super::common::{error_banner, muted};
use super::notes::{clock, first_line, when, words_label};
use super_voice_app_core::pages::clips::Input;

use crate::clips::Clip;
use crate::core::app::AppModel;
use crate::state::ErrorScope;
use crate::state::Stage;
use crate::state::clips::ClipsState;
use crate::ui::messages::{ClipsMessage, Message};

/// How much of a clip's first line its row shows. Shorter than a note's: the
/// row's buttons take the rest.
const ROW_PREVIEW_CHARS: usize = 60;

/// The least height of the text box, so a short clip still has room to type.
const DRAFT_MIN_HEIGHT: f32 = 88.0;

/// Roughly how fast speech goes, for the estimate under the text box: about
/// 150 words a minute.
const SECONDS_PER_WORD: f32 = 0.4;

fn clips_message(message: ClipsMessage) -> Message {
    Message::Clips(message)
}

/// A press the page's logic handles.
fn clips_input(input: Input) -> Message {
    Message::Clips(ClipsMessage::Page(input))
}

/// The Clips tab.
pub fn page(app: &AppModel) -> Element<'_, Message> {
    let mut blocks = Vec::new();
    if let Some(message) = app.action_error_for(ErrorScope::Clips) {
        blocks.push(error_banner(message));
    }
    blocks.push(intro());
    blocks.push(if app.clips.making.is_some() {
        making(app)
    } else {
        new_clip(app)
    });
    blocks.push(list(&app.clips));
    settings::view_column(blocks).into()
}

/// What clips are, in two lines.
fn intro<'a>() -> Element<'a, Message> {
    column![
        text::body(crate::fl!("type-something-and-keep-it-as-speech")),
        muted(text::caption(crate::fl!(
            "clips-are-saved-on-this-computer"
        ))),
    ]
    .spacing(cosmic::theme::spacing().space_xxs)
    .into()
}

/// The card the next clip is typed into: its text and Make clip. The voice is
/// not here: it and the model are chosen in one place, the Models page, and
/// each saved clip says which voice it was made in.
fn new_clip(app: &AppModel) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let draft = &app.clips.draft.0;
    let editor = widget::text_editor(draft)
        .min_height(DRAFT_MIN_HEIGHT)
        .padding(spacing.space_xs)
        .placeholder(crate::fl!("what-the-clip-should-say"))
        .on_action(|action| clips_message(ClipsMessage::DraftEdit(action)));
    let what = column![text::body(crate::fl!("new-clip")), editor]
        .spacing(spacing.space_xs)
        .padding([spacing.space_xs, 0]);

    let words = crate::notes::words(&draft.text());
    // Make clip stays off until text to speech can speak, with no voice to
    // speak in counted as not ready, and the caption says what is missing.
    let unready = super::models::not_ready_reason(app, Stage::Synthesis);
    let estimate = if let Some(reason) = unready.clone() {
        reason
    } else if words == 0 {
        crate::fl!("type-what-the-clip-should-say")
    } else {
        #[allow(clippy::cast_precision_loss)]
        let seconds = (words as f32 * SECONDS_PER_WORD).ceil();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let seconds = seconds as u64;
        format!(
            "{} \u{b7} {}",
            words_label(words),
            crate::fl!("about-seconds", seconds = seconds)
        )
    };
    let make = button::suggested(crate::fl!("make-clip")).on_press_maybe(
        (words > 0 && unready.is_none()).then(|| clips_message(ClipsMessage::Make)),
    );
    let footer = row![muted(text::caption(estimate)), horizontal_space(), make]
        .align_y(Alignment::Center)
        .width(Length::Fill);

    settings::section().add(what).add(footer).into()
}

/// The card while a clip is made: how long it has taken, Cancel, and a bar
/// that moves, since the daemon cannot say how far it has got.
fn making(app: &AppModel) -> Element<'_, Message> {
    let elapsed = app
        .clips
        .making
        .as_ref()
        .map_or_else(String::new, |m| clock(m.started.elapsed()));
    settings::section()
        .add(
            settings::item::builder(crate::fl!("making-the-clip"))
                .description(elapsed)
                .control(
                    button::standard(crate::fl!("cancel")).on_press(clips_input(Input::Cancel)),
                ),
        )
        .add(widget::indeterminate_linear().width(Length::Fill))
        .add(muted(
            text::body(first_line(&app.clips.draft.0.text(), 90))
                .wrapping(cosmic::iced::widget::text::Wrapping::None),
        ))
        .into()
}

/// The clips, newest first.
fn list(state: &ClipsState) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let mut header = row![text::heading(crate::fl!("clips")), horizontal_space()]
        .align_y(Alignment::Center)
        .width(Length::Fill);
    if !state.items.is_empty() {
        header = header.push(muted(text::caption(crate::fl!(
            "clips-count",
            count = state.items.len()
        ))));
    }
    let section = settings::section().header(header);
    if !state.loaded {
        return section
            .add(text::body(crate::fl!("loading-progress")))
            .into();
    }
    if state.items.is_empty() {
        return section
            .add(
                column![
                    text::body(crate::fl!("no-clips-yet")),
                    muted(text::caption(crate::fl!("each-clip-you-make-shows-here"))),
                ]
                .spacing(spacing.space_xxs)
                .padding([spacing.space_xs, 0]),
            )
            .into();
    }
    let today = chrono::Local::now().date_naive();
    state
        .items
        .iter()
        .fold(section, |section, clip| {
            section.add(clip_row(state, clip, today))
        })
        .into()
}

/// One clip: how it starts, when it was made, its voice and length, or how
/// far it has played; then Play or Stop, Export and Delete.
fn clip_row<'a>(state: &'a ClipsState, clip: &'a Clip, today: NaiveDate) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let playing = state.playing.as_ref().filter(|p| p.id == clip.id);
    let length = clock(std::time::Duration::from_secs_f32(clip.seconds.max(0.0)));
    let about = match playing.map(|p| p.player.as_ref()) {
        Some(Some(player)) => crate::fl!(
            "playing-position",
            position = clock(player.position()),
            length = clock(player.length())
        ),
        Some(None) => crate::fl!("starting"),
        None => {
            let mut parts = vec![when(clip.created, today)];
            if let Some(voice) = &clip.voice {
                parts.push(voice.clone());
            }
            parts.push(length);
            parts.join(" \u{b7} ")
        }
    };
    let mut lines = column![
        text::body(first_line(&clip.text, ROW_PREVIEW_CHARS))
            .wrapping(cosmic::iced::widget::text::Wrapping::None),
        muted(text::caption(about)),
    ]
    .spacing(2)
    .width(Length::Fill);
    if let Some((_, path)) = state.exported.as_ref().filter(|(id, _)| *id == clip.id) {
        lines = lines.push(muted(text::caption(crate::fl!(
            "exported-to",
            path = path.as_str()
        ))));
    }

    let mut row = row![lines]
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center);
    if state.confirming_delete.as_deref() == Some(clip.id.as_str()) {
        row = row
            .push(text::body(crate::fl!("delete-this-clip")))
            .push(button::text(crate::fl!("cancel")).on_press(clips_input(Input::DeleteCancelled)))
            .push(
                button::destructive(crate::fl!("delete")).on_press(clips_message(
                    ClipsMessage::Page(Input::DeleteConfirmed(clip.id.clone())),
                )),
            );
    } else {
        let play = if playing.is_some() {
            button::standard(crate::fl!("stop")).on_press(clips_input(Input::Stop))
        } else {
            button::standard(crate::fl!("play")).on_press(clips_input(Input::Play(clip.id.clone())))
        };
        row = row
            .push(play)
            .push(
                button::standard(crate::fl!("export"))
                    .on_press(clips_input(Input::Export(clip.id.clone()))),
            )
            .push(
                button::destructive(crate::fl!("delete")).on_press(clips_message(
                    ClipsMessage::Page(Input::DeleteRequested(clip.id.clone())),
                )),
            );
    }
    widget::container(row)
        .padding([spacing.space_xxs, 0])
        .width(Length::Fill)
        .into()
}
