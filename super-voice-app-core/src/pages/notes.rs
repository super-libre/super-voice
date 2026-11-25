// SPDX-License-Identifier: GPL-3.0-only
//! The Notes page: reading and writing the notes folder, and the takes that
//! record into it.
//!
//! A take is the daemon recording until Stop and sending the transcript back
//! here instead of typing it. A new note's file is made when its transcript
//! arrives; more dictated into an open note goes at its end, as a paragraph of
//! its own.

use std::time::Duration;

use futures_util::{Stream, StreamExt};
use super_voice_shared::models::protocol::PreviewSource;

use crate::daemon::client::{RecordEvent, record_command_stream, stop_record_command};
use crate::notes::{self, Note};
use crate::state::TextBuffer;
use crate::state::notes::{FileJob, NoteTake, NotesState, OpenNote, TakeTarget};

/// How long the open note's edits stand still before they are saved.
pub const SAVE_AFTER: Duration = Duration::from_millis(500);

/// How long the app, asked to close mid-take, waits for the take's transcript
/// before it closes anyway.
pub const EXIT_WAIT: Duration = Duration::from_secs(20);

/// What happened on the page: something the user did, or the answer to work
/// an [`Effect`] started.
#[derive(Debug, Clone)]
pub enum Input {
    /// Read the folder again.
    Reload,
    /// The folder was read.
    Loaded(Result<Vec<Note>, String>),
    /// A write to the folder finished.
    Written {
        job: FileJob,
        result: Result<(), String>,
    },
    /// Record a new note.
    Record,
    /// Record more into the open note.
    DictateMore,
    /// Stop the take.
    Stop,
    /// What the take has heard so far.
    Preview {
        text: String,
        source: Option<PreviewSource>,
    },
    /// The take's transcript, or why there is none.
    Transcribed(Result<String, String>),
    /// A new note's file was made.
    Created(Result<Note, String>),
    /// Open a note in place of the list.
    Open(String),
    /// Close the open note.
    Close,
    /// The open note's editor changed its text.
    Edited,
    /// The open note's edits have stood still since this many: save them, if
    /// none came after.
    SaveDue(u64),
    /// Save the open note's edits now: the user left the page.
    SaveNow,
    /// Export the open note.
    Export,
    /// The export finished: where the note went, or `None` when the user
    /// closed the dialog.
    Exported(Result<Option<String>, String>),
    /// Delete was pressed once.
    DeleteRequested,
    /// The delete was called off.
    DeleteCancelled,
    /// Delete was pressed again: delete the open note.
    DeleteConfirmed,
    /// The app is closing mid-take: finish the take and keep its words first.
    FinishBeforeExit,
    /// Close now, whether the take's transcript came or not.
    ExitNow,
    /// The app is closing: write everything not yet written.
    SaveBeforeExit,
}

/// Work [`update`] asks for. Each says which runner does it and what it
/// answers with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Read the folder ([`run_list`]); answers [`Input::Loaded`].
    List,
    /// Make one write to the folder ([`run_write`]); answers [`Input::Written`].
    Write(FileJob),
    /// Make a new note of these words ([`run_create`]); answers
    /// [`Input::Created`].
    Create(String),
    /// Record a take ([`take`]); answers with [`Input::Preview`]s, then
    /// [`Input::Transcribed`].
    StartTake,
    /// Ask the daemon to stop the take ([`run_stop`]). It answers nothing:
    /// the transcript comes on the take's own stream.
    StopTake,
    /// Wait, then answer [`Input::SaveDue`] with this edit count
    /// ([`run_save_after`]).
    SaveAfter(u64),
    /// Ask the user where to export the note, under this name, and write this
    /// text there; answers [`Input::Exported`]. The dialog is the app's own;
    /// [`notes::export`] writes the file.
    Export { name: String, text: String },
    /// Wait for the take a while, then answer [`Input::ExitNow`]
    /// ([`run_give_up`]).
    GiveUpLater,
    /// Make these writes now, on this thread, and a new note of `new_note`'s
    /// words, before the app closes ([`write_now`]).
    WriteNow {
        jobs: Vec<FileJob>,
        new_note: Option<String>,
    },
    /// Close the app.
    Exit,
}

/// Something to tell the user. Each app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The folder could not be read.
    CouldNotRead(String),
    /// A note could not be saved.
    CouldNotSave(String),
    /// A note could not be deleted.
    CouldNotDelete(String),
    /// A note could not be exported.
    CouldNotExport(String),
    /// The take heard no speech.
    NoSpeech,
    /// Another recording holds the microphone: the Behavior tab's test, or
    /// the dictation shortcut.
    AnotherRecording,
    /// The take failed.
    RecordingFailed(String),
}

/// What an [`update`] asks of the app.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The work to start, in order.
    pub effects: Vec<Effect>,
    /// Whether the page's notice is out of date and goes. Applied before
    /// [`notice`](Self::notice).
    pub clear_notice: bool,
    /// What to tell the user.
    pub notice: Option<Notice>,
}

/// What `input` does to the page.
pub fn update<B: TextBuffer>(state: &mut NotesState<B>, input: Input) -> Outcome {
    let mut out = Outcome::default();
    match input {
        Input::Reload => out.effects.push(Effect::List),
        Input::Loaded(result) => loaded(state, result, &mut out),
        Input::Written { job, result } => written(state, &job, result, &mut out),

        Input::Record => start_take(state, TakeTarget::New, &mut out),
        Input::DictateMore => {
            if let Some(id) = state.open.as_ref().map(|o| o.id.clone()) {
                start_take(state, TakeTarget::Append(id), &mut out);
            }
        }
        Input::Stop => {
            if let Some(take) = state.take.as_mut() {
                take.stopping = true;
                out.effects.push(Effect::StopTake);
            }
        }
        Input::Preview { text, source } => {
            if let Some(take) = state.take.as_mut() {
                take.preview = text;
                take.preview_source = source;
            }
        }
        Input::Transcribed(result) => take_finished(state, result, &mut out),
        Input::Created(result) => {
            let opening = state.opening.take();
            match result {
                Ok(note) => {
                    let (id, open) = (note.id.clone(), opening.as_ref() == Some(&note.text));
                    state.put(note);
                    // The take that made it was shown where the note goes.
                    if open {
                        open_note(state, &id, &mut out);
                    }
                }
                Err(e) => out.notice = Some(Notice::CouldNotSave(e)),
            }
        }

        Input::Open(id) => open_note(state, &id, &mut out),
        Input::Close => {
            state.opening = None;
            save_open(state, &mut out);
            state.open = None;
            state.confirming_delete = false;
            state.exported = None;
        }
        Input::Edited => {
            if let Some(open) = state.open.as_mut() {
                open.edits += 1;
                out.effects.push(Effect::SaveAfter(open.edits));
                state.exported = None;
            }
        }
        Input::SaveDue(edits) => {
            if state.open.as_ref().is_some_and(|o| o.edits == edits) {
                save_open(state, &mut out);
            }
        }
        Input::SaveNow => save_open(state, &mut out),
        Input::Export => export(state, &mut out),
        Input::Exported(result) => match result {
            Ok(Some(path)) => state.exported = Some(path),
            Ok(None) => {}
            Err(e) => out.notice = Some(Notice::CouldNotExport(e)),
        },
        Input::DeleteRequested => state.confirming_delete = true,
        Input::DeleteCancelled => state.confirming_delete = false,
        Input::DeleteConfirmed => {
            state.confirming_delete = false;
            if let Some(open) = state.open.take() {
                state.exported = None;
                write(state, FileJob::Delete { id: open.id }, &mut out);
            }
        }

        Input::FinishBeforeExit => {
            if let Some(take) = state.take.as_mut() {
                take.closing = true;
                if !take.stopping {
                    take.stopping = true;
                    out.effects.push(Effect::StopTake);
                }
                // A daemon that never answers must not keep the app open.
                out.effects.push(Effect::GiveUpLater);
            }
        }
        Input::ExitNow => {
            if state.take.is_some() {
                log::warn!("Closing before the note's transcript came; its words are not saved");
            }
            out.effects.push(Effect::WriteNow {
                jobs: state.drain_for_exit(),
                new_note: None,
            });
            out.effects.push(Effect::Exit);
        }
        Input::SaveBeforeExit => out.effects.push(Effect::WriteNow {
            jobs: state.drain_for_exit(),
            new_note: None,
        }),
    }
    out
}

/// The folder was read. A write still waiting holds newer text than the file
/// just read, so it wins.
fn loaded<B>(state: &mut NotesState<B>, result: Result<Vec<Note>, String>, out: &mut Outcome) {
    state.loaded = true;
    match result {
        Ok(read) => {
            state.items = read;
            for job in &state.queue {
                if let FileJob::Save { id, text, .. } = job
                    && let Some(note) = state.items.iter_mut().find(|n| &n.id == id)
                {
                    note.text.clone_from(text);
                }
            }
        }
        Err(e) => out.notice = Some(Notice::CouldNotRead(e)),
    }
}

/// A write finished: the next one starts.
fn written<B: TextBuffer>(
    state: &mut NotesState<B>,
    job: &FileJob,
    result: Result<(), String>,
    out: &mut Outcome,
) {
    match (job, result) {
        (FileJob::Save { id, edits, .. }, Ok(())) => {
            if let Some(open) = state.open.as_mut()
                && &open.id == id
            {
                open.saved = open.saved.max(*edits);
            }
        }
        (FileJob::Save { .. }, Err(e)) => out.notice = Some(Notice::CouldNotSave(e)),
        (FileJob::Delete { .. }, Ok(())) => {}
        (FileJob::Delete { .. }, Err(e)) => {
            out.notice = Some(Notice::CouldNotDelete(e));
            // The list dropped it already; read back what is really there.
            next_job(state, out);
            out.effects.push(Effect::List);
            return;
        }
    }
    next_job(state, out);
}

fn start_take<B>(state: &mut NotesState<B>, target: TakeTarget, out: &mut Outcome) {
    if state.take.is_some() {
        return;
    }
    out.clear_notice = true;
    state.exported = None;
    state.take = Some(NoteTake::new(target));
    out.effects.push(Effect::StartTake);
}

/// The take ended: make its note, or add it to the end of the one it was for.
/// A note deleted while more was dictated into it comes back as a new one, so
/// no words are lost. When the app is closing, the words are written at once
/// and the app closes.
fn take_finished<B: TextBuffer>(
    state: &mut NotesState<B>,
    result: Result<String, String>,
    out: &mut Outcome,
) {
    let Some(take) = state.take.take() else {
        return;
    };
    let said = match result {
        Ok(text) if text.trim().is_empty() => {
            out.notice = Some(Notice::NoSpeech);
            None
        }
        Ok(text) => Some(text.trim().to_string()),
        Err(e) if e.contains("recording_in_progress") => {
            out.notice = Some(Notice::AnotherRecording);
            None
        }
        Err(e) => {
            out.notice = Some(Notice::RecordingFailed(e));
            None
        }
    };
    let for_new_note = take.target == TakeTarget::New;
    let (start, new_note) = match said {
        None => (None, None),
        Some(said) => place_words(state, take.target, said),
    };
    if take.closing {
        let mut jobs: Vec<FileJob> = start.into_iter().collect();
        jobs.extend(state.drain_for_exit());
        out.effects.push(Effect::WriteNow { jobs, new_note });
        out.effects.push(Effect::Exit);
        return;
    }
    if let Some(job) = start {
        out.effects.push(Effect::Write(job));
    }
    if let Some(text) = new_note {
        // A take for a new note is shown where the note goes, so the note
        // opens once it is made. Words that were for a note deleted meanwhile
        // become a note of their own, but the user is elsewhere by then.
        if for_new_note {
            state.opening = Some(text.clone());
        }
        out.effects.push(Effect::Create(text));
    }
}

/// Open the note `id` in place of the list, saving the one open first.
fn open_note<B: TextBuffer>(state: &mut NotesState<B>, id: &str, out: &mut Outcome) {
    state.opening = None;
    save_open(state, out);
    if let Some(open) = state.note(id).map(OpenNote::new) {
        state.open = Some(open);
        state.confirming_delete = false;
        state.exported = None;
        out.clear_notice = true;
    }
}

/// Put a take's words where they go. Returns the write to start now, if one
/// is not already waiting its turn, and the words of a new note to make.
fn place_words<B: TextBuffer>(
    state: &mut NotesState<B>,
    target: TakeTarget,
    said: String,
) -> (Option<FileJob>, Option<String>) {
    let TakeTarget::Append(id) = target else {
        return (None, Some(said));
    };
    if let Some(open) = state.open.as_mut().filter(|o| o.id == id) {
        let text = notes::append(&open.content.text(), &said);
        open.content.replace_text(&text);
        open.edits += 1;
        let job = open.save_job();
        return (state.enqueue(job), None);
    }
    match state.note(&id) {
        Some(note) => {
            let text = notes::append(&note.text, &said);
            (state.enqueue(FileJob::Save { id, text, edits: 0 }), None)
        }
        None => (None, Some(said)),
    }
}

/// Export the open note: its name, and what its editor holds.
fn export<B: TextBuffer>(state: &mut NotesState<B>, out: &mut Outcome) {
    // A retry starts clean: an earlier failure's notice, or where the last
    // export went, would describe the wrong attempt.
    out.clear_notice = true;
    state.exported = None;
    let Some(open) = state.open.as_ref() else {
        return;
    };
    let name = state
        .note(&open.id)
        .map_or_else(|| format!("{}.md", open.id), notes::export_name);
    out.effects.push(Effect::Export {
        name,
        text: open.content.text(),
    });
}

/// Save what the open note's editor holds, when the file is behind it.
fn save_open<B: TextBuffer>(state: &mut NotesState<B>, out: &mut Outcome) {
    if let Some(job) = state
        .open
        .as_ref()
        .filter(|o| o.unsaved())
        .map(OpenNote::save_job)
    {
        write(state, job, out);
    }
}

/// Queue a write to the folder; start it if nothing else is writing.
fn write<B: TextBuffer>(state: &mut NotesState<B>, job: FileJob, out: &mut Outcome) {
    if let Some(job) = state.enqueue(job) {
        out.effects.push(Effect::Write(job));
    }
}

fn next_job<B: TextBuffer>(state: &mut NotesState<B>, out: &mut Outcome) {
    if let Some(job) = state.next_job() {
        out.effects.push(Effect::Write(job));
    }
}

// What the page says about a note, for any app that draws it.

/// How much of a note's first line its row shows.
pub const ROW_PREVIEW_CHARS: usize = 90;

/// How long a take may run. A note's take stops by hand only, so its limit is
/// that stop mode's.
#[must_use]
pub const fn take_limit() -> Duration {
    super_voice_shared::models::recording_stop_mode::RecordingStopMode::ManualOnly.take_limit()
}

/// The message `key` with `args`, without the marks Fluent puts around each
/// value: the words go into labels, where the marks would only ride along.
fn words(key: &str, args: &[(&str, &str)]) -> String {
    crate::i18n::text(key, args).replace(['\u{2068}', '\u{2069}'], "")
}

/// When a note was made: "Today, 09:42", "Yesterday, 18:15", or the date for
/// anything older. A note's name in the list and over the open note.
#[must_use]
pub fn when(created: chrono::NaiveDateTime, today: chrono::NaiveDate) -> String {
    let time = created.format("%H:%M").to_string();
    let day = created.date();
    if day == today {
        words("today-at", &[("time", &time)])
    } else if today.pred_opt() == Some(day) {
        words("yesterday-at", &[("time", &time)])
    } else {
        created.format("%Y-%m-%d %H:%M").to_string()
    }
}

/// How many words, in words: "1 word", "8 words".
#[must_use]
pub fn words_label(count: usize) -> String {
    if count == 1 {
        words("count-1-word", &[])
    } else {
        words("words-count", &[("n", &count.to_string())])
    }
}

/// A row's second line: when the note was made, and how long it is.
#[must_use]
pub fn row_about(note: &Note, today: chrono::NaiveDate) -> String {
    format!(
        "{} \u{b7} {}",
        when(note.created, today),
        words_label(note.words())
    )
}

/// `elapsed` as minutes and seconds: "0:42", "10:00".
#[must_use]
pub fn clock(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// How long a take has run, of how long it may: "0:05 of 10:00".
#[must_use]
pub fn take_elapsed(take: &NoteTake) -> String {
    words(
        "take-elapsed",
        &[
            ("elapsed", &clock(take.started.elapsed())),
            ("limit", &clock(take_limit())),
        ],
    )
}

/// A note's first line with words on it, cut to `max_chars` with an
/// ellipsis: at the last whole word, without the punctuation before the cut.
#[must_use]
pub fn first_line(text: &str, max_chars: usize) -> String {
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

/// The notes whose words hold every word of `query`, in any case and order,
/// newest first. An empty query matches every note.
#[must_use]
pub fn matching<'a>(notes: &'a [Note], query: &str) -> Vec<&'a Note> {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    notes
        .iter()
        .filter(|note| {
            let text = note.text.to_lowercase();
            terms.iter().all(|term| text.contains(term.as_str()))
        })
        .collect()
}

/// What the open note's header says under its name: how long it is and
/// whether its file has caught up, or that a take is adding to it.
#[must_use]
pub fn open_about<B: TextBuffer>(state: &NotesState<B>, open: &OpenNote<B>) -> String {
    let adding = state
        .take
        .as_ref()
        .is_some_and(|take| take.target == TakeTarget::Append(open.id.clone()));
    if adding {
        return words("adding-to-this-note", &[]);
    }
    let saved = if open.unsaved() {
        words("saving", &[])
    } else {
        words("saved", &[])
    };
    format!(
        "{} \u{b7} {saved}",
        words_label(notes::words(&open.content.text()))
    )
}

/// The name a note goes by in a question about it, such as Delete's: its
/// first line, shorter than a row's.
#[must_use]
pub fn short_name(note: &Note) -> String {
    first_line(&note.text, 48)
}

/// Today, in local time: what [`when`] and [`row_about`] measure from.
#[must_use]
pub fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// Write the export to `chosen`, the file the user picked in the dialog
/// ([`notes::export_path`] adds `.md` when they typed no extension).
pub async fn run_export_to(chosen: std::path::PathBuf, text: String) -> Input {
    let path = notes::export_path(&chosen);
    Input::Exported(
        notes::export(&text, &path)
            .await
            .map(|()| Some(path.display().to_string()))
            .map_err(|e| format!("{}: {e}", path.display())),
    )
}

/// Read the folder.
pub async fn run_list() -> Input {
    Input::Loaded(notes::list(&notes::dir()).await.map_err(|e| e.to_string()))
}

/// Carry out one write to the folder.
pub async fn run_write(job: FileJob) -> Input {
    let dir = notes::dir();
    let result = match &job {
        FileJob::Save { id, text, .. } => notes::save(&dir, id, text).await,
        FileJob::Delete { id } => notes::delete(&dir, id).await,
    };
    Input::Written {
        job,
        result: result.map_err(|e| e.to_string()),
    }
}

/// Make a note that says `text`, named for now.
pub async fn run_create(text: String) -> Input {
    let now = chrono::Local::now().naive_local();
    Input::Created(
        notes::create(&notes::dir(), &text, now)
            .await
            .map_err(|e| e.to_string()),
    )
}

/// Record a take: what it hears so far, then its transcript. Dropping the
/// stream stops the recording.
pub fn take() -> impl Stream<Item = Input> + Send + 'static {
    record_command_stream().map(|event| match event {
        RecordEvent::Preview { text, source } => Input::Preview { text, source },
        RecordEvent::Final(result) => Input::Transcribed(result),
    })
}

/// Ask the daemon to stop the take.
pub async fn run_stop() {
    if let Err(e) = stop_record_command().await {
        log::warn!("Stopping the note's recording failed: {e}");
    }
}

/// Wait out [`SAVE_AFTER`], then say the edits as of `edits` are due.
pub async fn run_save_after(edits: u64) -> Input {
    tokio::time::sleep(SAVE_AFTER).await;
    Input::SaveDue(edits)
}

/// Wait out [`EXIT_WAIT`], then say to close anyway.
pub async fn run_give_up() -> Input {
    tokio::time::sleep(EXIT_WAIT).await;
    Input::ExitNow
}

/// Make every write not yet made, on this thread, before the app exits:
/// `jobs`, then a new note of `new_note`'s words. Background writes not yet
/// started are dropped by the state that drained them, since these are newer.
pub fn write_now(jobs: Vec<FileJob>, new_note: Option<String>) {
    let last = notes::last_writes(&notes::dir());
    for job in jobs {
        let written = match &job {
            FileJob::Save { id, text, .. } => last.save(id, text),
            FileJob::Delete { id } => last.delete(id),
        };
        if let Err(e) = written {
            log::error!("Could not write {job:?} before closing: {e}");
        }
    }
    if let Some(text) = new_note
        && let Err(e) = last.create(&text, chrono::Local::now().naive_local())
    {
        log::error!("Could not save the note {text:?} before closing: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type State = NotesState<String>;

    fn note(id: &str, text: &str) -> Note {
        Note {
            id: id.to_string(),
            created: chrono::NaiveDateTime::parse_from_str("2026-11-20 10:00", "%Y-%m-%d %H:%M")
                .expect("a time"),
            text: text.to_string(),
        }
    }

    /// A state with note `a`, saying `text`, open.
    fn open(text: &str) -> State {
        let mut state = State::default();
        state.put(note("a", text));
        update(&mut state, Input::Open("a".into()));
        state
    }

    #[test]
    fn edits_are_saved_once_they_stand_still() {
        let mut state = open("draft");
        assert_eq!(
            update(&mut state, Input::Edited).effects,
            [Effect::SaveAfter(1)]
        );
        state.open.as_mut().expect("open").content = "draft, longer".into();
        assert_eq!(
            update(&mut state, Input::Edited).effects,
            [Effect::SaveAfter(2)]
        );

        // The first wait is overtaken by the second edit.
        assert_eq!(update(&mut state, Input::SaveDue(1)).effects, []);
        assert_eq!(
            update(&mut state, Input::SaveDue(2)).effects,
            [Effect::Write(FileJob::Save {
                id: "a".into(),
                text: "draft, longer".into(),
                edits: 2,
            })]
        );
    }

    #[test]
    fn a_take_into_the_open_note_goes_at_its_end_and_is_saved() {
        let mut state = open("first");
        assert_eq!(
            update(&mut state, Input::DictateMore).effects,
            [Effect::StartTake]
        );
        let out = update(&mut state, Input::Transcribed(Ok(" second ".into())));
        let text = notes::append("first", "second");
        assert_eq!(state.open.as_ref().expect("open").content, text);
        assert_eq!(
            out.effects,
            [Effect::Write(FileJob::Save {
                id: "a".into(),
                text,
                edits: 1,
            })]
        );
        assert!(state.take.is_none());
    }

    #[test]
    fn a_new_take_makes_a_new_note() {
        let mut state = State::default();
        update(&mut state, Input::Record);
        // A second press while recording starts nothing.
        assert_eq!(update(&mut state, Input::Record).effects, []);
        assert_eq!(
            update(&mut state, Input::Transcribed(Ok("hello".into()))).effects,
            [Effect::Create("hello".into())]
        );
    }

    #[test]
    fn a_new_note_opens_once_its_take_is_kept() {
        let mut state = State::default();
        update(&mut state, Input::Record);
        update(&mut state, Input::Transcribed(Ok("hello".into())));
        update(&mut state, Input::Created(Ok(note("new", "hello"))));
        assert_eq!(state.open.as_ref().map(|o| o.id.as_str()), Some("new"));
        assert_eq!(
            state.open.as_ref().map(|o| o.content.as_str()),
            Some("hello")
        );
    }

    #[test]
    fn a_note_made_any_other_way_stays_closed() {
        // Opened another note while the file was made: that one stays.
        let mut state = State::default();
        state.put(note("a", "older"));
        update(&mut state, Input::Record);
        update(&mut state, Input::Transcribed(Ok("hello".into())));
        update(&mut state, Input::Open("a".into()));
        update(&mut state, Input::Created(Ok(note("new", "hello"))));
        assert_eq!(state.open.as_ref().map(|o| o.id.as_str()), Some("a"));

        // More dictated into a note deleted meanwhile becomes a note of its
        // own, which doesn't open: the user is elsewhere by then.
        let mut state = open("doomed");
        update(&mut state, Input::DictateMore);
        update(&mut state, Input::DeleteConfirmed);
        let out = update(&mut state, Input::Transcribed(Ok("more".into())));
        assert_eq!(out.effects, [Effect::Create("more".into())]);
        update(&mut state, Input::Created(Ok(note("more", "more"))));
        assert!(state.open.is_none());
    }

    #[test]
    fn closing_mid_take_finishes_it_then_writes_and_exits() {
        let mut state = State::default();
        update(&mut state, Input::Record);
        assert_eq!(
            update(&mut state, Input::FinishBeforeExit).effects,
            [Effect::StopTake, Effect::GiveUpLater]
        );
        assert_eq!(
            update(&mut state, Input::Transcribed(Ok("last words".into()))).effects,
            [
                Effect::WriteNow {
                    jobs: Vec::new(),
                    new_note: Some("last words".into()),
                },
                Effect::Exit,
            ]
        );
    }

    #[test]
    fn a_take_that_heard_nothing_or_was_refused_says_why() {
        for (result, notice) in [
            (Ok("  ".to_string()), Notice::NoSpeech),
            (
                Err("recording_in_progress".to_string()),
                Notice::AnotherRecording,
            ),
            (
                Err("no microphone".to_string()),
                Notice::RecordingFailed("no microphone".into()),
            ),
        ] {
            let mut state = State::default();
            update(&mut state, Input::Record);
            let out = update(&mut state, Input::Transcribed(result));
            assert_eq!(out.notice, Some(notice));
            assert_eq!(out.effects, []);
        }
    }

    #[test]
    fn a_failed_delete_reads_the_folder_back() {
        let mut state = open("doomed");
        let out = update(&mut state, Input::DeleteConfirmed);
        let job = FileJob::Delete { id: "a".into() };
        assert_eq!(out.effects, [Effect::Write(job.clone())]);

        let out = update(
            &mut state,
            Input::Written {
                job,
                result: Err("read-only".into()),
            },
        );
        assert_eq!(out.notice, Some(Notice::CouldNotDelete("read-only".into())));
        assert_eq!(out.effects, [Effect::List]);
    }

    #[test]
    fn a_reload_keeps_a_save_still_waiting() {
        let mut state = State::default();
        state.put(note("a", "old"));
        state.put(note("b", "old"));
        // `a` is being written, so `b`'s save waits its turn.
        state.enqueue(FileJob::Save {
            id: "a".into(),
            text: "running".into(),
            edits: 0,
        });
        state.enqueue(FileJob::Save {
            id: "b".into(),
            text: "newest".into(),
            edits: 0,
        });
        update(
            &mut state,
            Input::Loaded(Ok(vec![note("a", "old"), note("b", "old")])),
        );
        assert_eq!(state.note("b").map(|n| n.text.as_str()), Some("newest"));
        assert!(state.loaded);
    }

    fn day(date: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").expect("a date")
    }

    fn at(time: &str) -> chrono::NaiveDateTime {
        chrono::NaiveDateTime::parse_from_str(time, "%Y-%m-%d %H:%M").expect("a time")
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
        assert!(today.contains("09:42") && today != yesterday, "{today}");
        assert!(yesterday.contains("18:15"), "{yesterday}");
        assert!(!today.contains('\u{2068}'), "no isolation marks");
    }

    #[test]
    fn the_clock_reads_minutes_and_seconds() {
        assert_eq!(clock(Duration::from_secs(42)), "0:42");
        assert_eq!(clock(Duration::from_mins(10)), "10:00");
        assert_eq!(clock(take_limit()), "10:00");
    }

    #[test]
    fn a_long_first_line_is_cut_at_a_whole_word() {
        assert_eq!(first_line("\n  Short note.  \nMore", 90), "Short note.");
        assert_eq!(
            first_line("Call the landlord about the heater, before Friday", 36),
            "Call the landlord about the heater\u{2026}"
        );
    }

    #[test]
    fn a_search_matches_every_word_in_any_case() {
        let notes = [
            note("a", "Groceries: oat milk and lemons"),
            note("b", "Call the landlord"),
        ];
        let ids = |query| {
            matching(&notes, query)
                .iter()
                .map(|n| n.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(""), ["a", "b"]);
        assert_eq!(ids("LEMONS oat"), ["a"]);
        assert!(ids("lemons landlord").is_empty());
    }

    #[test]
    fn the_open_note_says_whether_it_is_saved_or_being_added_to() {
        let mut state = open("two words");
        let about = |state: &State| open_about(state, state.open.as_ref().expect("open"));
        let saved = about(&state);
        assert!(saved.starts_with(&words_label(2)), "{saved}");
        update(&mut state, Input::Edited);
        assert_ne!(about(&state), saved, "saving, until the file catches up");
        update(&mut state, Input::DictateMore);
        assert_eq!(about(&state), words("adding-to-this-note", &[]));
    }

    #[test]
    fn export_names_the_file_and_takes_what_the_editor_holds() {
        let mut state = open("saved text");
        state.open.as_mut().expect("open").content = "typed since".into();
        let out = update(&mut state, Input::Export);
        assert!(out.clear_notice);
        assert_eq!(
            out.effects,
            [Effect::Export {
                name: notes::export_name(&note("a", "")),
                text: "typed since".into(),
            }]
        );
    }
}
