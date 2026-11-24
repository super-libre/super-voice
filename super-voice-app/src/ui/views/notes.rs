// SPDX-License-Identifier: GPL-3.0-only
//! The Notes tab: the notes the user has spoken, newest first, with a row that
//! records a new one; or one note open in the list's place, to read, edit,
//! dictate more into, export or delete.

use std::time::Duration;

use chrono::{NaiveDate, NaiveDateTime};
use cosmic::Element;
use cosmic::iced::widget::{column, row};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, icon, settings, space::horizontal as horizontal_space, text};
use super_voice_app_core::pages::notes::Input;
use super_voice_shared::models::recording_stop_mode::RecordingStopMode;

use super::common::{LEVEL_METER_WIDTH, error_banner, muted};
use super::recording::live_row;
use crate::core::app::AppModel;
use crate::notes::{self, Note};
use crate::state::Stage;
use crate::state::notes::{NoteTake, NotesState, OpenNote, TakeTarget};
use crate::state::{ErrorScope, RecordingStatus};
use crate::ui::messages::{Message, NotesMessage};

/// How much of a note's first line its row shows.
const ROW_PREVIEW_CHARS: usize = 90;

/// The least height the open note's editor takes, so a short note still
/// looks like a page to write on. A longer note grows it, and the tab
/// scrolls.
const EDITOR_MIN_HEIGHT: f32 = 320.0;

fn notes_message(message: NotesMessage) -> Message {
    Message::Notes(message)
}

/// A press the page's logic handles.
fn notes_input(input: Input) -> Message {
    Message::Notes(NotesMessage::Page(input))
}

/// The Notes tab.
pub fn page(app: &AppModel) -> Element<'_, Message> {
    let state = &app.notes;
    let mut blocks = Vec::new();
    if let Some(message) = app.action_error_for(ErrorScope::Notes) {
        blocks.push(error_banner(message));
    }
    if let Some(open) = state.open.as_ref() {
        blocks.extend(open_note(app, open));
    } else {
        blocks.push(intro());
        blocks.push(recorder(app, state.take.as_ref()));
        blocks.push(list(state));
    }
    settings::view_column(blocks).into()
}

/// What notes are, in two lines.
fn intro<'a>() -> Element<'a, Message> {
    column![
        text::body(crate::fl!("speak-a-note-and-it-is-kept-here")),
        muted(text::caption(crate::fl!(
            "notes-are-saved-on-this-computer"
        ))),
    ]
    .spacing(cosmic::theme::spacing().space_xxs)
    .into()
}

/// The New note row with its Record button, or the take recording.
fn recorder<'a>(app: &'a AppModel, take: Option<&'a NoteTake>) -> Element<'a, Message> {
    if let Some(take) = take {
        return take_section(app, take);
    }
    // The Behavior tab's test, or the dictation shortcut, holds the
    // microphone; the daemon would refuse a second take.
    let busy = app.recording_status == RecordingStatus::Recording;
    // A take with no model ready to transcribe it would record and then
    // fail, so Record stays off until speech to text is ready.
    let unready = super::models::not_ready_reason(app, Stage::Transcription);
    let free = !busy && unready.is_none();
    let description = if busy {
        crate::fl!("another-recording-is-running")
    } else {
        unready.unwrap_or_else(|| crate::fl!("press-record-and-speak"))
    };
    settings::section()
        .add(
            settings::item::builder(crate::fl!("new-note"))
                .description(description)
                .control(
                    button::suggested(crate::fl!("record"))
                        .on_press_maybe(free.then(|| notes_input(Input::Record))),
                ),
        )
        .into()
}

/// A take recording: how long it has run, whether it hears speech, Stop, the
/// level, and the words so far. The Behavior tab's test, laid out the same.
fn take_section<'a>(app: &'a AppModel, take: &'a NoteTake) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let label = match &take.target {
        TakeTarget::New => crate::fl!("new-note"),
        // Anywhere but on the note itself, say which note the words go to.
        TakeTarget::Append(id) if app.notes.open.as_ref().is_none_or(|o| &o.id != id) => {
            let today = chrono::Local::now().date_naive();
            app.notes.note(id).map_or_else(
                || crate::fl!("dictate-more"),
                |note| crate::fl!("adding-to-note", when = when(note.created, today)),
            )
        }
        TakeTarget::Append(_) => crate::fl!("dictate-more"),
    };
    // The takes here stop by hand only, so theirs is that mode's limit.
    let limit = RecordingStopMode::ManualOnly.take_limit();
    let elapsed = crate::fl!(
        "recording-elapsed",
        elapsed = clock(take.started.elapsed()),
        limit = clock(limit)
    );
    let heard = if app.is_speech_detected {
        crate::fl!("speech")
    } else {
        crate::fl!("silence")
    };
    let stop = if take.stopping {
        button::destructive(crate::fl!("stopping"))
    } else {
        button::destructive(crate::fl!("stop")).on_press(notes_input(Input::Stop))
    };
    let status = row![text::body(heard), stop]
        .spacing(spacing.space_s)
        .align_y(Alignment::Center);

    let level = app.audio_level;
    let meter = widget::determinate_linear(level.max(if level > 0.0 { 0.1 } else { 0.0 }))
        .width(Length::Fixed(LEVEL_METER_WIDTH));

    settings::section()
        .add(
            settings::item::builder(label)
                .description(elapsed)
                .control(status),
        )
        .add(settings::item(crate::fl!("audio-level"), meter))
        .add(live_row(&take.preview, take.preview_source))
        .into()
}

/// The notes, newest first; each row opens its note.
fn list(state: &NotesState) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let count = match state.items.len() {
        0 => None,
        n => Some(crate::fl!("notes-count", count = n)),
    };
    let mut header = row![text::heading(crate::fl!("notes")), horizontal_space()]
        .align_y(Alignment::Center)
        .width(Length::Fill);
    if let Some(count) = count {
        header = header.push(muted(text::caption(count)));
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
                    text::body(crate::fl!("no-notes-yet")),
                    muted(text::caption(crate::fl!("each-note-you-record-shows-here"))),
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
        .fold(section, |section, note| section.add(note_row(note, today)))
        .into()
}

/// One note in the list: how it starts, when it was made and how long it is,
/// and the chevron that says it opens.
fn note_row(note: &Note, today: NaiveDate) -> widget::list::ListButton<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let first_line = first_line(&note.text, ROW_PREVIEW_CHARS);
    let about = format!(
        "{} \u{b7} {}",
        when(note.created, today),
        words_label(note.words())
    );
    let content = row![
        column![
            text::body(first_line).wrapping(cosmic::iced::widget::text::Wrapping::None),
            muted(text::caption(about)),
        ]
        .spacing(2)
        .width(Length::Fill),
        icon::from_name("go-next-symbolic").size(16).icon(),
    ]
    .spacing(spacing.space_s)
    .align_y(Alignment::Center)
    .padding([spacing.space_xxs, 0]);
    widget::list::button(content).on_press(notes_input(Input::Open(note.id.clone())))
}

/// The open note, in the list's place: back to the list, when it was made,
/// the take if one is recording, the editor, and what can be done with it.
fn open_note<'a>(app: &'a AppModel, open: &'a OpenNote) -> Vec<Element<'a, Message>> {
    let spacing = cosmic::theme::spacing();
    let state = &app.notes;
    let today = chrono::Local::now().date_naive();

    let back = button::text(crate::fl!("all-notes"))
        .leading_icon(icon::from_name("go-previous-symbolic"))
        .on_press(notes_input(Input::Close));

    let title = state
        .note(&open.id)
        .map_or_else(|| open.id.clone(), |note| when(note.created, today));
    let saved = if open.unsaved() {
        crate::fl!("saving")
    } else {
        crate::fl!("saved")
    };
    let about = format!(
        "{} \u{b7} {saved}",
        words_label(notes::words(&open.content.0.text()))
    );
    let heading = row![text::title4(title), muted(text::caption(about))]
        .spacing(spacing.space_s)
        .align_y(Alignment::Center);

    let mut blocks: Vec<Element<'a, Message>> =
        vec![column![back, heading].spacing(spacing.space_xs).into()];
    if let Some(take) = state.take.as_ref() {
        blocks.push(take_section(app, take));
    }

    let editor = widget::text_editor(&open.content.0)
        .min_height(EDITOR_MIN_HEIGHT)
        .padding(spacing.space_s)
        .on_action(|action| notes_message(NotesMessage::Edit(action)));
    let mut body = column![editor].spacing(spacing.space_xs);
    if let Some(path) = state.exported.as_deref() {
        body = body.push(muted(text::caption(crate::fl!("exported-to", path = path))));
    }
    blocks.push(body.into());
    blocks.push(actions(app));
    blocks
}

/// Delete on the left, away from the rest; Export and Dictate more on the
/// right. Delete asks once more in place, as a context's does. Dictate more
/// waits while any take holds the microphone, and until speech to text is
/// ready, which the row says beside it.
fn actions(app: &AppModel) -> Element<'_, Message> {
    let state = &app.notes;
    let unready = super::models::not_ready_reason(app, Stage::Transcription);
    let free = !state.is_recording()
        && app.recording_status != RecordingStatus::Recording
        && unready.is_none();
    let spacing = cosmic::theme::spacing();
    let delete: Element<'_, Message> = if state.confirming_delete {
        row![
            text::body(crate::fl!("delete-this-note")),
            button::text(crate::fl!("cancel")).on_press(notes_input(Input::DeleteCancelled)),
            button::destructive(crate::fl!("delete")).on_press(notes_input(Input::DeleteConfirmed)),
        ]
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center)
        .into()
    } else {
        button::destructive(crate::fl!("delete"))
            .on_press(notes_input(Input::DeleteRequested))
            .into()
    };
    let dictate = button::suggested(crate::fl!("dictate-more"))
        .on_press_maybe(free.then(|| notes_input(Input::DictateMore)));
    let why: Element<'_, Message> = match unready {
        Some(reason) => muted(text::caption(reason)),
        None => horizontal_space().into(),
    };
    row![
        delete,
        horizontal_space(),
        why,
        button::standard(crate::fl!("export")).on_press(notes_input(Input::Export)),
        dictate,
    ]
    .spacing(spacing.space_xs)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

/// When a note was made: "Today, 09:42", "Yesterday, 18:15", or the date for
/// anything older.
pub(super) fn when(created: NaiveDateTime, today: NaiveDate) -> String {
    let time = created.format("%H:%M").to_string();
    let day = created.date();
    if day == today {
        crate::fl!("today-at", time = time)
    } else if today.pred_opt() == Some(day) {
        crate::fl!("yesterday-at", time = time)
    } else {
        created.format("%Y-%m-%d %H:%M").to_string()
    }
}

pub(super) fn words_label(words: usize) -> String {
    if words == 1 {
        crate::fl!("count-1-word")
    } else {
        crate::fl!("words-count", n = words.to_string())
    }
}

/// `elapsed` as minutes and seconds: "0:42", "10:00".
pub(super) fn clock(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// A note's first line with words on it, cut to `max_chars` with an
/// ellipsis: at the last whole word, without the punctuation before the cut.
pub(super) fn first_line(text: &str, max_chars: usize) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= max_chars {
        return line.to_string();
    }
    let mut end = max_chars;
    if !chars[end].is_whitespace()
        && let Some(space) = chars[..end].iter().rposition(|c| c.is_whitespace())
    {
        end = space;
    }
    let cut: String = chars[..end].iter().collect();
    let cut = cut.trim_end_matches(|c: char| c.is_whitespace() || ".,;:!?-".contains(c));
    format!("{cut}\u{2026}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(date: &str) -> NaiveDate {
        NaiveDate::parse_from_str(date, "%Y-%m-%d").expect("a date")
    }

    fn at(time: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(time, "%Y-%m-%d %H:%M").expect("a time")
    }

    #[test]
    fn a_note_older_than_yesterday_shows_its_date() {
        assert_eq!(
            when(at("2026-11-16 11:03"), day("2026-11-18")),
            "2026-11-16 11:03"
        );
    }

    #[test]
    fn todays_and_yesterdays_notes_say_so() {
        let today = when(at("2026-11-18 09:42"), day("2026-11-18"));
        let yesterday = when(at("2026-11-17 18:15"), day("2026-11-18"));
        assert!(
            today.starts_with("Today, ") && today.contains("09:42"),
            "{today}"
        );
        assert!(
            yesterday.starts_with("Yesterday, ") && yesterday.contains("18:15"),
            "{yesterday}"
        );
    }

    #[test]
    fn the_clock_reads_minutes_and_seconds() {
        assert_eq!(clock(Duration::from_secs(42)), "0:42");
        assert_eq!(clock(Duration::from_mins(10)), "10:00");
        assert_eq!(clock(Duration::from_millis(61_900)), "1:01");
    }

    #[test]
    fn a_row_shows_the_first_line_with_words() {
        assert_eq!(first_line("\n  \nGroceries\nmore", 90), "Groceries");
        assert_eq!(first_line("", 90), "");
        assert_eq!(first_line("one two three", 7), "one two\u{2026}");
        assert_eq!(first_line("one two three", 9), "one two\u{2026}");
        assert_eq!(first_line("ñandú ñandú", 5), "ñandú\u{2026}");
        assert_eq!(first_line("abcdefghij", 5), "abcde\u{2026}");
        assert_eq!(
            first_line("by Friday. Flushed on leave.", 11),
            "by Friday\u{2026}"
        );
    }
}
