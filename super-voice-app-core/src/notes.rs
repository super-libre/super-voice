// SPDX-License-Identifier: GPL-3.0-only
//! The notes the user speaks on the Notes tab, kept by the app in its own
//! folder: one Markdown file per note.
//!
//! The app keeps them, not the daemon. A note is text the user wrote, read
//! and edited here; nothing else asks for it. A file per note keeps the folder
//! readable without the app, and the file name, the time the note was made,
//! is what orders them.
//!
//! Every call takes the folder, so the tests point it at a scratch one. The
//! app uses [`dir`].

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::NaiveDateTime;

/// How a note's file name spells the time it was made. Clips are named the
/// same way.
pub(crate) const STAMP: &str = "%Y-%m-%d_%H-%M-%S";

/// The extension every note has.
const EXTENSION: &str = "md";

/// One note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// The file name without its extension. Unique in the folder.
    pub id: String,
    /// When the note was made, in local time.
    pub created: NaiveDateTime,
    /// What it says.
    pub text: String,
}

impl Note {
    /// How many words it has.
    #[must_use]
    pub fn words(&self) -> usize {
        words(&self.text)
    }
}

/// How many words `text` has.
#[must_use]
pub fn words(text: &str) -> usize {
    text.split_whitespace().count()
}

/// The folder the app keeps its notes in: `notes` in Super Voice's data
/// folder. In a Flatpak that folder is the app's own.
#[must_use]
pub fn dir() -> PathBuf {
    super_voice_shared::paths::data_dir().join("notes")
}

/// Every note in `dir`, newest first. A folder that is not there yet holds
/// none.
///
/// # Errors
///
/// When the folder or a note in it cannot be read.
pub async fn list(dir: &Path) -> io::Result<Vec<Note>> {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut notes = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        // Hidden files are the ones a save is still writing.
        if id.starts_with('.') {
            continue;
        }
        let text = tokio::fs::read_to_string(&path).await?;
        let created = if let Some(created) = created_from_id(id) {
            created
        } else {
            // A note someone put in the folder by hand: when it was last
            // changed is the best there is.
            let modified = entry.metadata().await?.modified()?;
            chrono::DateTime::<chrono::Local>::from(modified).naive_local()
        };
        notes.push(Note {
            id: id.to_string(),
            created,
            text,
        });
    }
    sort_newest_first(&mut notes);
    Ok(notes)
}

/// Newest first; two made in the same second, by name.
pub fn sort_newest_first(notes: &mut [Note]) {
    notes.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
}

/// Make a note in `dir` that says `text`, made at `now`.
///
/// Named for `now`; a second note in the same second gets `-2`, then `-3`.
///
/// # Errors
///
/// When the folder cannot be made or the file written.
pub async fn create(dir: &Path, text: &str, now: NaiveDateTime) -> io::Result<Note> {
    let (dir, text) = (dir.to_path_buf(), text.to_string());
    // Never dropped, unlike a save: a new note is the only copy of its words.
    tokio::task::spawn_blocking(move || {
        let _one_at_a_time = closed();
        create_now(&dir, &text, now)
    })
    .await
    .map_err(io::Error::other)?
}

/// Write `text` as the note `id` says.
///
/// Written beside it and moved over it, so a note is never left half
/// written.
///
/// # Errors
///
/// When `id` is not a note's name, or the file cannot be written.
pub async fn save(dir: &Path, id: &str, text: &str) -> io::Result<()> {
    let (dir, id, text) = (dir.to_path_buf(), id.to_string(), text.to_string());
    in_background(dir.clone(), move || save_now(&dir, &id, &text)).await
}

/// Delete the note `id`. One already gone is not an error.
///
/// # Errors
///
/// When `id` is not a note's name, or the file cannot be removed.
pub async fn delete(dir: &Path, id: &str) -> io::Result<()> {
    let (dir, id) = (dir.to_path_buf(), id.to_string());
    in_background(dir.clone(), move || delete_now(&dir, &id)).await
}

/// The folders whose last writes have been made, because the app is closing:
/// see [`last_writes`]. Held while any write runs, so writes never overlap.
static CLOSED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn closed() -> MutexGuard<'static, Vec<PathBuf>> {
    CLOSED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Run a save or a delete off the async runtime. One asked for before the
/// app began closing but not yet started is dropped once it has: the closing
/// writes hold newer text, and must be the last to land.
async fn in_background(
    dir: PathBuf,
    write: impl FnOnce() -> io::Result<()> + Send + 'static,
) -> io::Result<()> {
    tokio::task::spawn_blocking(move || {
        let closed = closed();
        if closed.contains(&dir) {
            return Ok(());
        }
        write()
    })
    .await
    .map_err(io::Error::other)?
}

/// The app is closing and cannot wait for its writes to run in the
/// background: these are the last, made on the calling thread. Taking them
/// waits for a write already running, and drops every one not yet started.
#[must_use]
pub fn last_writes(dir: &Path) -> LastWrites {
    let mut closed = closed();
    if !closed.iter().any(|d| d == dir) {
        closed.push(dir.to_path_buf());
    }
    LastWrites {
        dir: dir.to_path_buf(),
        _closed: closed,
    }
}

/// The writes made as the app closes; see [`last_writes`].
pub struct LastWrites {
    dir: PathBuf,
    _closed: MutexGuard<'static, Vec<PathBuf>>,
}

impl LastWrites {
    /// As [`create`] does.
    ///
    /// # Errors
    ///
    /// As [`create`].
    pub fn create(&self, text: &str, now: NaiveDateTime) -> io::Result<Note> {
        create_now(&self.dir, text, now)
    }

    /// As [`save`] does.
    ///
    /// # Errors
    ///
    /// As [`save`].
    pub fn save(&self, id: &str, text: &str) -> io::Result<()> {
        save_now(&self.dir, id, text)
    }

    /// As [`delete`] does.
    ///
    /// # Errors
    ///
    /// As [`delete`].
    pub fn delete(&self, id: &str) -> io::Result<()> {
        delete_now(&self.dir, id)
    }
}

fn create_now(dir: &Path, text: &str, now: NaiveDateTime) -> io::Result<Note> {
    std::fs::create_dir_all(dir)?;
    let stamp = now.format(STAMP).to_string();
    let mut number = 1;
    loop {
        let id = if number == 1 {
            stamp.clone()
        } else {
            format!("{stamp}-{number}")
        };
        // Claim the name first, so a note already there is never written
        // over, then fill it the way every save does.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path_of(dir, &id))
        {
            Ok(_) => {
                save_now(dir, &id, text)?;
                return Ok(Note {
                    id,
                    created: now,
                    text: text.to_string(),
                });
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => number += 1,
            Err(e) => return Err(e),
        }
    }
}

fn save_now(dir: &Path, id: &str, text: &str) -> io::Result<()> {
    check_id(id)?;
    let staged = dir.join(format!(".{id}.{EXTENSION}.saving"));
    std::fs::write(&staged, text)?;
    std::fs::rename(&staged, path_of(dir, id))
}

fn delete_now(dir: &Path, id: &str) -> io::Result<()> {
    check_id(id)?;
    match std::fs::remove_file(path_of(dir, id)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Write a note's text to `to`, a file the user chose. The same text whether
/// they named it `.md` or `.txt`: a note is plain text, which is Markdown too.
///
/// # Errors
///
/// When the file cannot be written.
pub async fn export(text: &str, to: &Path) -> io::Result<()> {
    let mut text = text.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    tokio::fs::write(to, text).await
}

/// Where an export the user named `chosen` goes. A name without an extension
/// gets `.md`, since the save dialog adds none of its own, unless a file of
/// that name is already there: the dialog asked about overwriting the name
/// as typed, not that one.
#[must_use]
pub fn export_path(chosen: &Path) -> PathBuf {
    if chosen.extension().is_some() {
        return chosen.to_path_buf();
    }
    let markdown = chosen.with_extension(EXTENSION);
    if markdown.exists() {
        chosen.to_path_buf()
    } else {
        markdown
    }
}

/// The name an exported note is offered under: `super_voice_`, that it is a
/// note, and when it was made, so a file found later says where it came from.
#[must_use]
pub fn export_name(note: &Note) -> String {
    format!(
        "super_voice_note_{}.{EXTENSION}",
        note.created.format("%Y-%m-%d_%H-%M")
    )
}

/// `text` with `more` said after it, as a paragraph of its own.
#[must_use]
pub fn append(text: &str, more: &str) -> String {
    let text = text.trim_end();
    let more = more.trim();
    if text.is_empty() {
        more.to_string()
    } else {
        format!("{text}\n\n{more}")
    }
}

fn path_of(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.{EXTENSION}"))
}

/// When the note named `id` was made, read from its name; `None` for a name
/// the app did not give.
pub(crate) fn created_from_id(id: &str) -> Option<NaiveDateTime> {
    let stamp = id.get(..19)?;
    let rest = &id[19..];
    let numbered = rest.is_empty()
        || rest
            .strip_prefix('-')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    if !numbered {
        return None;
    }
    NaiveDateTime::parse_from_str(stamp, STAMP).ok()
}

/// Refuse anything but a plain file name, so an id can never reach outside
/// the folder.
pub(crate) fn check_id(id: &str) -> io::Result<()> {
    let plain = !id.is_empty()
        && !id.starts_with('.')
        && Path::new(id).file_name().and_then(|n| n.to_str()) == Some(id);
    if plain {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{id:?} is not a note"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(stamp: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(stamp, "%Y-%m-%d %H:%M:%S").expect("a valid time")
    }

    #[tokio::test]
    async fn a_folder_not_made_yet_holds_no_notes() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let notes = list(&scratch.path().join("notes")).await.expect("a list");
        assert!(notes.is_empty());
    }

    #[tokio::test]
    async fn a_note_is_a_markdown_file_named_for_when_it_was_made() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path().join("notes");

        let note = create(&dir, "Eggs, oat milk, lemons.", at("2026-11-18 09:42:07"))
            .await
            .expect("a note");

        assert_eq!(note.id, "2026-11-18_09-42-07");
        let file = dir.join("2026-11-18_09-42-07.md");
        assert_eq!(
            std::fs::read_to_string(file).expect("the file"),
            "Eggs, oat milk, lemons."
        );
        assert_eq!(list(&dir).await.expect("a list"), vec![note]);
    }

    #[tokio::test]
    async fn two_notes_in_one_second_both_keep_their_words() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path();
        let now = at("2026-11-18 09:42:07");

        let first = create(dir, "first", now).await.expect("a note");
        let second = create(dir, "second", now).await.expect("a note");

        assert_eq!(first.id, "2026-11-18_09-42-07");
        assert_eq!(second.id, "2026-11-18_09-42-07-2");
        let notes = list(dir).await.expect("a list");
        let texts: Vec<&str> = notes.iter().map(|n| n.text.as_str()).collect();
        assert_eq!(texts, ["second", "first"]);
    }

    #[tokio::test]
    async fn notes_list_newest_first() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path();
        create(dir, "older", at("2026-11-14 16:27:00"))
            .await
            .expect("a note");
        create(dir, "newest", at("2026-11-18 09:42:07"))
            .await
            .expect("a note");
        create(dir, "middle", at("2026-11-16 11:03:00"))
            .await
            .expect("a note");

        let notes = list(dir).await.expect("a list");

        let texts: Vec<&str> = notes.iter().map(|n| n.text.as_str()).collect();
        assert_eq!(texts, ["newest", "middle", "older"]);
    }

    #[tokio::test]
    async fn a_save_replaces_the_text_and_leaves_nothing_beside_it() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path();
        let note = create(dir, "draft", at("2026-11-18 09:42:07"))
            .await
            .expect("a note");

        save(dir, &note.id, "final words").await.expect("a save");

        let notes = list(dir).await.expect("a list");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].text, "final words");
        let files: Vec<_> = std::fs::read_dir(dir)
            .expect("the folder")
            .map(|e| e.expect("an entry").file_name())
            .collect();
        assert_eq!(files, ["2026-11-18_09-42-07.md"]);
    }

    #[tokio::test]
    async fn a_deleted_note_is_gone_and_deleting_it_again_is_fine() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path();
        let note = create(dir, "gone soon", at("2026-11-18 09:42:07"))
            .await
            .expect("a note");

        delete(dir, &note.id).await.expect("a delete");
        delete(dir, &note.id).await.expect("a second delete");

        assert!(list(dir).await.expect("a list").is_empty());
    }

    #[tokio::test]
    async fn an_id_cannot_reach_outside_the_folder() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path().join("notes");
        std::fs::create_dir(&dir).expect("the folder");
        let outside = scratch.path().join("outside.md");
        std::fs::write(&outside, "keep me").expect("a file outside");

        for id in ["../outside", "/tmp/x", "", ".hidden", "a/b"] {
            assert!(save(&dir, id, "x").await.is_err(), "{id:?} was saved");
            assert!(delete(&dir, id).await.is_err(), "{id:?} was deleted");
        }
        assert_eq!(
            std::fs::read_to_string(outside).expect("the file outside"),
            "keep me"
        );
    }

    #[tokio::test]
    async fn a_note_put_in_the_folder_by_hand_is_listed() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path();
        std::fs::write(dir.join("shopping.md"), "bread").expect("a note");
        std::fs::write(dir.join("readme.txt"), "not a note").expect("a file");

        let notes = list(dir).await.expect("a list");

        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, "shopping");
        assert_eq!(notes[0].text, "bread");
    }

    #[test]
    fn only_names_the_app_gives_are_read_as_times() {
        assert_eq!(
            created_from_id("2026-11-18_09-42-07"),
            Some(at("2026-11-18 09:42:07"))
        );
        assert_eq!(
            created_from_id("2026-11-18_09-42-07-12"),
            Some(at("2026-11-18 09:42:07"))
        );
        assert_eq!(created_from_id("2026-11-18_09-42-07-"), None);
        assert_eq!(created_from_id("2026-11-18_09-42-07 copy"), None);
        assert_eq!(created_from_id("shopping"), None);
    }

    #[tokio::test]
    async fn an_export_ends_its_last_line() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let to = scratch.path().join("note.txt");

        export("one line", &to).await.expect("an export");

        assert_eq!(std::fs::read_to_string(to).expect("the file"), "one line\n");
    }

    #[tokio::test]
    async fn the_last_writes_land_and_nothing_queued_lands_after_them() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path();
        let note = create(dir, "first words", at("2026-11-18 09:42:07"))
            .await
            .expect("a note");

        let last = last_writes(dir);
        last.save(&note.id, "newest words").expect("the last save");
        let made = last
            .create("said as the app closed", at("2026-11-18 09:50:00"))
            .expect("the last note");
        drop(last);
        // Asked for before the app closed, run after: dropped.
        save(dir, &note.id, "older words")
            .await
            .expect("a dropped save");
        delete(dir, &made.id).await.expect("a dropped delete");

        let notes = list(dir).await.expect("a list");
        let texts: Vec<&str> = notes.iter().map(|n| n.text.as_str()).collect();
        assert_eq!(texts, ["said as the app closed", "newest words"]);
    }

    #[test]
    fn an_export_is_offered_under_the_apps_name_and_the_time() {
        let note = Note {
            id: "2026-11-18_21-53-26".to_string(),
            created: created_from_id("2026-11-18_21-53-26").expect("a stamp"),
            text: String::new(),
        };
        assert_eq!(export_name(&note), "super_voice_note_2026-11-18_21-53.md");
    }

    #[test]
    fn an_export_named_without_an_extension_is_markdown() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path();
        assert_eq!(export_path(&dir.join("dentist")), dir.join("dentist.md"));
        assert_eq!(
            export_path(&dir.join("dentist.txt")),
            dir.join("dentist.txt")
        );
        std::fs::write(dir.join("taken.md"), "someone else's").expect("a file");
        assert_eq!(export_path(&dir.join("taken")), dir.join("taken"));
    }

    #[test]
    fn more_words_start_a_paragraph_of_their_own() {
        assert_eq!(
            append("First thought.", " Second. "),
            "First thought.\n\nSecond."
        );
        assert_eq!(
            append("Ends with space.  \n", "More."),
            "Ends with space.\n\nMore."
        );
        assert_eq!(append("", "Only this."), "Only this.");
    }

    #[test]
    fn words_are_counted_on_whitespace() {
        assert_eq!(words("eggs, oat milk,\n\ntwo lemons"), 5);
        assert_eq!(words("   "), 0);
    }
}
