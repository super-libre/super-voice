// SPDX-License-Identifier: GPL-3.0-only
//! The speech clips the user makes on the Clips tab, kept by the app in its
//! own folder: one WAV file per clip, with a small JSON file beside it saying
//! what the clip says and in which voice.
//!
//! The daemon makes the audio (`POST /tts/synthesize`); the app keeps it,
//! plays it and exports it. Named like notes, for the time each was made, so
//! the folder reads in order without the app.
//!
//! Every call takes the folder, so the tests point it at a scratch one. The
//! app uses [`dir`].

use std::io;
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::notes::{STAMP, check_id, created_from_id};

/// The audio's extension.
const AUDIO: &str = "wav";

/// The extension of the file that says what a clip is.
const ABOUT: &str = "json";

/// One clip.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// The file name without its extension. Unique in the folder.
    pub id: String,
    /// When the clip was made, in local time.
    pub created: NaiveDateTime,
    /// What it says.
    pub text: String,
    /// The voice it was made in, as the user knew it; `None` when not known.
    pub voice: Option<String>,
    /// How long it plays.
    pub seconds: f32,
}

/// What the JSON file beside a clip holds.
#[derive(Debug, Default, Serialize, Deserialize)]
struct About {
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    voice: Option<String>,
}

/// The folder the app keeps its clips in: `clips` in Super Voice's data
/// folder. In a Flatpak that folder is the app's own.
#[must_use]
pub fn dir() -> PathBuf {
    super_voice_shared::paths::data_dir().join("clips")
}

/// Where the clip `id`'s audio is.
#[must_use]
pub fn audio_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.{AUDIO}"))
}

fn about_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.{ABOUT}"))
}

/// Run blocking file work off the async runtime.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(io::Error::other)?
}

/// Every clip in `dir`, newest first. A folder that is not there yet holds
/// none; a WAV that cannot be read is left out.
///
/// # Errors
///
/// When the folder cannot be read.
pub async fn list(dir: &Path) -> io::Result<Vec<Clip>> {
    let dir = dir.to_path_buf();
    blocking(move || list_now(&dir)).await
}

fn list_now(dir: &Path) -> io::Result<Vec<Clip>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut clips = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some(AUDIO) {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if id.starts_with('.') {
            continue;
        }
        let Ok(seconds) = seconds_of_file(&path) else {
            log::warn!("Leaving out {}: not a readable WAV", path.display());
            continue;
        };
        let about = std::fs::read(about_path(dir, id))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<About>(&bytes).ok())
            .unwrap_or_default();
        let created = if let Some(created) = created_from_id(id) {
            created
        } else {
            let modified = std::fs::metadata(&path)?.modified()?;
            chrono::DateTime::<chrono::Local>::from(modified).naive_local()
        };
        clips.push(Clip {
            id: id.to_string(),
            created,
            text: about.text,
            voice: about.voice,
            seconds,
        });
    }
    clips.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
    Ok(clips)
}

/// Keep `wav`, the audio of `text` spoken in `voice`, as a clip made at
/// `now`. Named for `now`; a second clip in the same second gets `-2`.
///
/// # Errors
///
/// When `wav` is not a readable WAV, or the files cannot be written.
pub async fn create(
    dir: &Path,
    wav: Vec<u8>,
    text: String,
    voice: Option<String>,
    now: NaiveDateTime,
) -> io::Result<Clip> {
    let dir = dir.to_path_buf();
    blocking(move || create_now(&dir, &wav, text, voice, now)).await
}

fn create_now(
    dir: &Path,
    wav: &[u8],
    text: String,
    voice: Option<String>,
    now: NaiveDateTime,
) -> io::Result<Clip> {
    let seconds =
        seconds_of(&hound::WavReader::new(io::Cursor::new(wav)).map_err(|e| invalid(&e))?);
    std::fs::create_dir_all(dir)?;
    let stamp = now.format(STAMP).to_string();
    let mut number = 1;
    let id = loop {
        let id = if number == 1 {
            stamp.clone()
        } else {
            format!("{stamp}-{number}")
        };
        // Claim the name first, so a clip already there is never written
        // over.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(audio_path(dir, &id))
        {
            Ok(_) => break id,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => number += 1,
            Err(e) => return Err(e),
        }
    };
    let about = About { text, voice };
    let written = serde_json::to_vec_pretty(&about)
        .map_err(io::Error::from)
        .and_then(|json| write_beside(dir, &about_path(dir, &id), &json))
        .and_then(|()| write_beside(dir, &audio_path(dir, &id), wav));
    if let Err(e) = written {
        // Leave no half a clip behind: the name claimed, and nothing in it.
        let _ = std::fs::remove_file(audio_path(dir, &id));
        let _ = std::fs::remove_file(about_path(dir, &id));
        return Err(e);
    }
    Ok(Clip {
        id,
        created: now,
        text: about.text,
        voice: about.voice,
        seconds,
    })
}

/// Write `bytes` beside `path` and move them over it, so a file is never
/// left half written.
fn write_beside(dir: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let staged = dir.join(format!(".{name}.saving"));
    std::fs::write(&staged, bytes)?;
    std::fs::rename(&staged, path)
}

/// Delete the clip `id`: its audio and what it says. One already gone is not
/// an error.
///
/// # Errors
///
/// When `id` is not a clip's name, or a file cannot be removed.
pub async fn delete(dir: &Path, id: &str) -> io::Result<()> {
    check_id(id)?;
    let (dir, id) = (dir.to_path_buf(), id.to_string());
    blocking(move || {
        for path in [audio_path(&dir, &id), about_path(&dir, &id)] {
            match std::fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        Ok(())
    })
    .await
}

/// The clip `id`'s audio, mono, and its sample rate: what the player plays.
///
/// # Errors
///
/// When `id` is not a clip's name, or its WAV cannot be read.
pub async fn samples(dir: &Path, id: &str) -> io::Result<(Vec<f32>, u32)> {
    check_id(id)?;
    let path = audio_path(dir, id);
    blocking(move || {
        let reader = hound::WavReader::open(&path).map_err(|e| invalid(&e))?;
        decode(reader)
    })
    .await
}

/// Copy the clip `id`'s audio to `to`, a file the user chose.
///
/// # Errors
///
/// When `id` is not a clip's name, or the file cannot be copied.
pub async fn export(dir: &Path, id: &str, to: &Path) -> io::Result<()> {
    check_id(id)?;
    tokio::fs::copy(audio_path(dir, id), to).await.map(|_| ())
}

/// Where an export the user named `chosen` goes. A name without an extension
/// gets `.wav`, since the save dialog adds none of its own, unless a file of
/// that name is already there: the dialog asked about overwriting the name
/// as typed, not that one.
#[must_use]
pub fn export_path(chosen: &Path) -> PathBuf {
    if chosen.extension().is_some() {
        return chosen.to_path_buf();
    }
    let wav = chosen.with_extension(AUDIO);
    if wav.exists() {
        chosen.to_path_buf()
    } else {
        wav
    }
}

/// The name an exported clip is offered under: `super_voice_`, that it is a
/// clip, and when it was made, so a file found later says where it came from.
#[must_use]
pub fn export_name(clip: &Clip) -> String {
    format!(
        "super_voice_clip_{}.{AUDIO}",
        clip.created.format("%Y-%m-%d_%H-%M")
    )
}

fn invalid(e: &hound::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

fn seconds_of_file(path: &Path) -> io::Result<f32> {
    Ok(seconds_of(
        &hound::WavReader::open(path).map_err(|e| invalid(&e))?,
    ))
}

fn seconds_of<R: io::Read>(reader: &hound::WavReader<R>) -> f32 {
    let spec = reader.spec();
    if spec.sample_rate == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let frames = reader.duration() as f32;
    #[allow(clippy::cast_precision_loss)]
    let rate = spec.sample_rate as f32;
    frames / rate
}

/// Every sample as `f32`, the channels of each frame averaged into one.
fn decode<R: io::Read>(reader: hound::WavReader<R>) -> io::Result<(Vec<f32>, u32)> {
    let spec = reader.spec();
    let channels = usize::from(spec.channels.max(1));
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| invalid(&e))?,
        hound::SampleFormat::Int => {
            #[allow(clippy::cast_precision_loss)]
            let scale = (1_i64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            #[allow(clippy::cast_precision_loss)]
            reader
                .into_samples::<i32>()
                .map(|s| s.map(|s| s as f32 / scale))
                .collect::<Result<_, _>>()
                .map_err(|e| invalid(&e))?
        }
    };
    if channels == 1 {
        return Ok((interleaved, spec.sample_rate));
    }
    #[allow(clippy::cast_precision_loss)]
    let mono = interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect();
    Ok((mono, spec.sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(stamp: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(stamp, "%Y-%m-%d %H:%M:%S").expect("a valid time")
    }

    /// A mono 16-bit WAV of `frames` samples at 24 kHz.
    fn wav(frames: usize) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 24_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = io::Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("a writer");
            for i in 0..frames {
                writer
                    .write_sample(i16::try_from(i % 1000).expect("fits"))
                    .expect("a sample");
            }
            writer.finalize().expect("a WAV");
        }
        cursor.into_inner()
    }

    #[tokio::test]
    async fn a_folder_not_made_yet_holds_no_clips() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        assert!(
            list(&scratch.path().join("clips"))
                .await
                .expect("a list")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_clip_is_its_audio_and_what_it_says_named_for_when_it_was_made() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path().join("clips");

        let clip = create(
            &dir,
            wav(36_000),
            "Your table is ready.".into(),
            Some("Bella".into()),
            at("2026-11-18 11:40:05"),
        )
        .await
        .expect("a clip");

        assert_eq!(clip.id, "2026-11-18_11-40-05");
        assert!((clip.seconds - 1.5).abs() < 1e-3, "{}", clip.seconds);
        assert!(dir.join("2026-11-18_11-40-05.wav").is_file());
        assert!(dir.join("2026-11-18_11-40-05.json").is_file());
        assert_eq!(list(&dir).await.expect("a list"), vec![clip]);
    }

    #[tokio::test]
    async fn clips_list_newest_first_and_two_in_one_second_both_keep() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path();
        let older = at("2026-11-17 21:15:00");
        let now = at("2026-11-18 15:02:00");
        create(dir, wav(10), "older".into(), None, older)
            .await
            .expect("a clip");
        create(dir, wav(10), "first".into(), None, now)
            .await
            .expect("a clip");
        create(dir, wav(10), "second".into(), None, now)
            .await
            .expect("a clip");

        let texts: Vec<String> = list(dir)
            .await
            .expect("a list")
            .into_iter()
            .map(|c| c.text)
            .collect();
        assert_eq!(texts, ["second", "first", "older"]);
    }

    #[tokio::test]
    async fn audio_that_is_not_a_wav_makes_no_clip() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path();
        let made = create(
            dir,
            b"not a wav".to_vec(),
            "x".into(),
            None,
            at("2026-11-18 15:02:00"),
        )
        .await;
        assert!(made.is_err());
        assert!(list(dir).await.expect("a list").is_empty());
    }

    #[tokio::test]
    async fn a_deleted_clip_takes_both_files_and_deleting_again_is_fine() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path();
        let clip = create(dir, wav(10), "gone".into(), None, at("2026-11-18 15:02:00"))
            .await
            .expect("a clip");

        delete(dir, &clip.id).await.expect("a delete");
        delete(dir, &clip.id).await.expect("a second delete");

        assert_eq!(std::fs::read_dir(dir).expect("the folder").count(), 0);
        assert!(delete(dir, "../outside").await.is_err());
    }

    #[tokio::test]
    async fn the_player_gets_the_samples_back_as_written() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path();
        let clip = create(dir, wav(2_000), "x".into(), None, at("2026-11-18 15:02:00"))
            .await
            .expect("a clip");

        let (samples, rate) = samples(dir, &clip.id).await.expect("samples");

        assert_eq!(rate, 24_000);
        assert_eq!(samples.len(), 2_000);
        assert!((samples[999] - 999.0 / 32_768.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn an_export_is_the_clips_own_audio() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path().join("clips");
        let audio = wav(500);
        let clip = create(
            &dir,
            audio.clone(),
            "x".into(),
            None,
            at("2026-11-18 15:02:00"),
        )
        .await
        .expect("a clip");
        let to = scratch.path().join("out.wav");

        export(&dir, &clip.id, &to).await.expect("an export");

        assert_eq!(std::fs::read(to).expect("the export"), audio);
    }

    #[test]
    fn an_export_is_offered_under_the_apps_name_and_the_time() {
        let clip = Clip {
            id: "2026-11-18_21-53-26".to_string(),
            created: created_from_id("2026-11-18_21-53-26").expect("a stamp"),
            text: String::new(),
            voice: None,
            seconds: 0.0,
        };
        assert_eq!(export_name(&clip), "super_voice_clip_2026-11-18_21-53.wav");
    }

    #[test]
    fn an_export_named_without_an_extension_is_a_wav() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dir = scratch.path();
        assert_eq!(export_path(&dir.join("table")), dir.join("table.wav"));
        assert_eq!(export_path(&dir.join("table.wav")), dir.join("table.wav"));
        std::fs::write(dir.join("taken.wav"), "someone else's").expect("a file");
        assert_eq!(export_path(&dir.join("taken")), dir.join("taken"));
    }

    #[test]
    fn stereo_is_averaged_to_mono_for_the_player() {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = io::Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("a writer");
            for s in [16_384_i16, 0, 16_384, 16_384] {
                writer.write_sample(s).expect("a sample");
            }
            writer.finalize().expect("a WAV");
        }
        let reader = hound::WavReader::new(io::Cursor::new(cursor.into_inner())).expect("a WAV");

        let (mono, rate) = decode(reader).expect("decoded");

        assert_eq!(rate, 48_000);
        assert_eq!(mono, [0.25, 0.5]);
    }
}
