// SPDX-License-Identifier: GPL-3.0-only
//! The Notes page's made-up state, for `just app-shot`: three notes, and its
//! scenes. `empty`: no notes yet. `open`: the newest note open. `dictating`:
//! the newest open, with more being dictated into it. `recording`: a new note
//! being recorded.

use std::time::{Duration, Instant};

use gpui_kit::{Context, Window};
use super_voice_app_core::notes::Note;
use super_voice_app_core::pages::notes::{self as page, Input};
use super_voice_app_core::state::Microphone;
use super_voice_app_core::state::notes::{NoteTake, TakeTarget};

use super::NotesPage;
use crate::ui::waveform::Levels;

/// The made-up notes, newest first: (days ago, hour, minute, text).
const NOTES: [(u32, u32, u32, &str); 3] = [
    (1, 8, 2, "Call the landlord about the heater before Friday."),
    (
        3,
        16,
        40,
        "Idea for the talk on Thursday: start with the demo, not the slides. Show a note being dictated, then edited, then exported.",
    ),
    (
        4,
        9,
        14,
        "Groceries for the weekend: oat milk, lemons, two loaves of rye, and coffee beans from the place on Fifth.",
    ),
];

/// What the take has heard so far.
const HEARD: &str = "Ask whether the plumber can come on Thursday morning";

pub(super) fn fill(
    notes: &mut NotesPage,
    scenes: &[&str],
    window: &mut Window,
    cx: &mut Context<NotesPage>,
) {
    let items = if scenes.contains(&"empty") {
        Vec::new()
    } else {
        NOTES
            .iter()
            .map(|&(ago, h, m, text)| note(ago, h, m, text))
            .collect()
    };
    page::update(&mut notes.state, Input::Loaded(Ok(items)));
    let first = notes.state.items.first().map(|n| n.id.clone());
    if let Some(id) = first.filter(|_| scenes.contains(&"open") || scenes.contains(&"dictating")) {
        page::update(&mut notes.state, Input::Open(id.clone()));
        if scenes.contains(&"dictating") {
            take(notes, TakeTarget::Append(id));
        }
    }
    if scenes.contains(&"recording") {
        take(notes, TakeTarget::New);
    }
    notes.sync_editor(window, cx);
    // The list has the keyboard's focus; the shot presses Down.
    if scenes.contains(&"notes-keys") {
        notes.keys.focus(window, cx);
    }
    cx.notify();
}

/// A take five seconds in, hearing speech.
fn take(notes: &mut NotesPage, target: TakeTarget) {
    let mut take = NoteTake::new(target);
    take.started = Instant::now()
        .checked_sub(Duration::from_secs(5))
        .unwrap_or_else(Instant::now);
    take.preview = HEARD.to_string();
    notes.state.take = Some(take);
    notes.microphone = Microphone {
        recording: true,
        level: 0.6,
        speech: true,
    };
    notes.levels = Levels::from_iter((0..120).map(|i: u16| {
        let i = f32::from(i);
        // Words, then a breath of silence at the end.
        if i > 112. {
            0.
        } else {
            (0.35 + 0.65 * (i / 6.5).sin().abs()) * 0.275f32.mul_add((i * 7.3).sin(), 0.725)
        }
    }));
}

/// A note made `days_ago` at `hour`:`minute`.
fn note(days_ago: u32, hour: u32, minute: u32, text: &str) -> Note {
    let mut day = page::today();
    for _ in 0..days_ago {
        day = day.pred_opt().unwrap_or(day);
    }
    let created = day.and_hms_opt(hour, minute, 0).expect("a time of day");
    Note {
        id: created.format("%Y-%m-%d_%H-%M-%S").to_string(),
        created,
        text: text.to_string(),
    }
}
