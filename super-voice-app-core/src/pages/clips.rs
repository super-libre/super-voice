// SPDX-License-Identifier: GPL-3.0-only
//! The Clips page: making a clip from text through the daemon, keeping it in
//! the clips folder, and playing, exporting and deleting what is kept.
//!
//! The files are the truth (see [`crate::clips`]). A clip is written once,
//! when it is made, and never changed.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::future::{AbortHandle, AbortRegistration, Abortable};

use crate::audio::player::Player;
use crate::clips::{self, Clip};
use crate::pages::model_voice;
use crate::state::clips::{ClipsState, Making, Playing};
use crate::state::{TextBuffer, VoiceState};

/// What happened on the page: something the user did, or the answer to work
/// an [`Effect`] started.
#[derive(Debug, Clone)]
pub enum Input {
    /// Read the folder again.
    Reload,
    /// The folder was read.
    Loaded(Result<Vec<Clip>, String>),
    /// Make a clip of what the draft says. `voice` is the voice text to speech
    /// speaks in, as the app names it, kept with the clip and shown on its row.
    Make { voice: Option<String> },
    /// The clip was made and kept, or why not.
    Made(Result<Clip, String>),
    /// Stop making the clip.
    Cancel,
    /// A moment passed: a clip that has played out is let go.
    Tick,
    /// Play the clip with this id.
    Play(String),
    /// The clip's audio was read: its samples and rate.
    Ready {
        id: String,
        result: Result<(Vec<f32>, u32), String>,
    },
    /// Play the clip playing from this far into it, 0 to 1.
    Seek(f32),
    /// Stop the clip playing.
    Stop,
    /// Export the clip with this id.
    Export(String),
    /// The export finished: where the clip went, or `None` when the user
    /// closed the dialog.
    Exported {
        id: String,
        result: Result<Option<String>, String>,
    },
    /// Delete was pressed once for this clip.
    DeleteRequested(String),
    /// The delete was called off.
    DeleteCancelled,
    /// Delete was pressed again: delete this clip.
    DeleteConfirmed(String),
    /// The clip's files were deleted, or why not.
    Deleted {
        id: String,
        result: Result<(), String>,
    },
}

/// Work [`update`] asks for. Each says which runner does it and what it
/// answers with.
#[derive(Debug)]
pub enum Effect {
    /// Read the folder ([`run_list`]); answers [`Input::Loaded`].
    List,
    /// Have the daemon speak `text` and keep it as a clip ([`run_make`]);
    /// answers [`Input::Made`], or nothing once [`Input::Cancel`] aborted it
    /// through `abort`.
    Make {
        text: String,
        voice: Option<String>,
        abort: AbortRegistration,
    },
    /// Read a clip's audio ([`run_samples`]); answers [`Input::Ready`].
    ReadSamples(String),
    /// Ask the user where to export the clip, under this name, then write it
    /// there with [`export_to`]; answers [`Input::Exported`]. The dialog is
    /// the app's own.
    Export { id: String, name: String },
    /// Delete a clip's files ([`run_delete`]); answers [`Input::Deleted`].
    Delete(String),
}

/// Something to tell the user. Each app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The folder could not be read.
    CouldNotRead(String),
    /// The clip could not be made.
    CouldNotMake(String),
    /// The clip could not play.
    CouldNotPlay(String),
    /// The clip could not be exported.
    CouldNotExport(String),
    /// The clip could not be deleted.
    CouldNotDelete(String),
}

/// What an [`update`] asks of the app.
#[derive(Debug, Default)]
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
pub fn update<B: TextBuffer>(state: &mut ClipsState<B>, input: Input) -> Outcome {
    let mut out = Outcome::default();
    match input {
        Input::Reload => out.effects.push(Effect::List),
        Input::Loaded(result) => {
            state.loaded = true;
            match result {
                Ok(items) => state.items = items,
                Err(e) => out.notice = Some(Notice::CouldNotRead(e)),
            }
        }
        make @ (Input::Make { .. } | Input::Made(_) | Input::Cancel) => {
            making(state, make, &mut out);
        }
        play @ (Input::Tick
        | Input::Play(_)
        | Input::Ready { .. }
        | Input::Seek(_)
        | Input::Stop) => {
            playing(state, play, &mut out);
        }
        kept => kept_clip(state, kept, &mut out),
    }
    out
}

/// The next clip: its text, making it, and stopping that.
fn making<B: TextBuffer>(state: &mut ClipsState<B>, input: Input, out: &mut Outcome) {
    match input {
        Input::Make { voice } => {
            let text = state.draft.text().trim().to_string();
            if text.is_empty() || state.making.is_some() {
                return;
            }
            out.clear_notice = true;
            state.exported = None;
            let (handle, abort) = AbortHandle::new_pair();
            state.making = Some(Making {
                started: Instant::now(),
                handle,
            });
            out.effects.push(Effect::Make { text, voice, abort });
        }
        Input::Made(result) => {
            state.making = None;
            match result {
                Ok(clip) => {
                    state.put(clip);
                    state.draft.replace_text("");
                }
                Err(e) => out.notice = Some(Notice::CouldNotMake(e)),
            }
        }
        Input::Cancel => {
            // Aborting the request closes it, which stops the synthesis.
            if let Some(making) = state.making.take() {
                making.handle.abort();
            }
        }
        _ => {}
    }
}

/// Playing a clip, and stopping it.
fn playing<B>(state: &mut ClipsState<B>, input: Input, out: &mut Outcome) {
    match input {
        // A clip that has played out is let go; one that could not play says
        // why.
        Input::Tick => {
            let ended = state
                .playing
                .as_ref()
                .and_then(|p| p.player.as_ref())
                .filter(|player| player.finished())
                .map(Player::error);
            if let Some(error) = ended {
                state.playing = None;
                if let Some(e) = error {
                    out.notice = Some(Notice::CouldNotPlay(e));
                }
            }
        }
        Input::Play(id) => {
            out.clear_notice = true;
            // One at a time: the one playing stops when it is let go.
            state.playing = Some(Playing::new(id.clone()));
            out.effects.push(Effect::ReadSamples(id));
        }
        Input::Ready { id, result } => {
            // Stopped, or another clip asked for, while this one was read.
            let Some(playing) = state.playing.as_mut().filter(|p| p.id == id) else {
                return;
            };
            match result {
                // The player is held in the state, and plays until let go.
                Ok((samples, rate)) => {
                    let audio: Arc<[f32]> = samples.into();
                    playing.player = Some(Player::start(audio.to_vec(), rate));
                    playing.audio = Some((audio, rate));
                }
                Err(e) => {
                    state.playing = None;
                    out.notice = Some(Notice::CouldNotPlay(e));
                }
            }
        }
        // A new player from there: the one playing stops when it is let go.
        Input::Seek(fraction) => {
            let Some(playing) = state.playing.as_mut() else {
                return;
            };
            let Some((audio, rate)) = playing.audio.clone() else {
                return;
            };
            if rate == 0 {
                return;
            }
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let frame = ((audio.len() as f64) * f64::from(fraction.clamp(0.0, 1.0))) as usize;
            let frame = frame.min(audio.len());
            #[allow(clippy::cast_precision_loss)]
            let from = Duration::from_secs_f64(frame as f64 / f64::from(rate));
            playing.player = Some(Player::start(audio[frame..].to_vec(), rate));
            playing.from = from;
        }
        Input::Stop => state.playing = None,
        _ => {}
    }
}

/// The voice a clip made now is spoken in, as the user knows it: the voice
/// text to speech speaks in, by its label. What [`Input::Make`] carries.
/// `None` while `voice` doesn't describe `(source, model)`, the synthesis
/// model, or says no voice.
#[must_use]
pub fn voice_label(voice: &VoiceState, source: &str, model: &str) -> Option<String> {
    if !model_voice::shows(voice, source, model) {
        return None;
    }
    let id = voice.resolution.as_ref()?.effective.as_ref()?;
    Some(
        voice
            .choices
            .iter()
            .find(|v| &v.id == id)
            .map_or_else(|| id.clone(), |v| v.label.clone()),
    )
}

/// When a clip was made, as its row says it: "Today, 09:40", "Yesterday,
/// 17:12", or the date and time.
#[must_use]
pub fn when(created: chrono::NaiveDateTime, today: chrono::NaiveDate) -> String {
    let time = created.format("%H:%M").to_string();
    let day = created.date();
    if day == today {
        crate::i18n::text("today-at", &[("time", &time)])
    } else if today.pred_opt() == Some(day) {
        crate::i18n::text("yesterday-at", &[("time", &time)])
    } else {
        created.format("%Y-%m-%d %H:%M").to_string()
    }
    .replace(['\u{2068}', '\u{2069}'], "")
}

/// `length` as minutes and seconds: "0:06", "1:42".
#[must_use]
pub fn clock(length: Duration) -> String {
    let seconds = length.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// A clip's first line with words on it, cut to `max_chars` with an
/// ellipsis, at the last whole word: its name in a list or a question.
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

/// Roughly how fast speech goes, for the estimate of how long a clip will
/// play: about 150 words a minute.
const SECONDS_PER_WORD: f32 = 0.4;

/// About how many seconds `words` words take to say, rounded up.
#[must_use]
pub fn estimated_seconds(words: usize) -> u64 {
    #[allow(clippy::cast_precision_loss)]
    let seconds = (words as f32 * SECONDS_PER_WORD).ceil();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let seconds = seconds as u64;
    seconds
}

/// Exporting and deleting a kept clip.
fn kept_clip<B>(state: &mut ClipsState<B>, input: Input, out: &mut Outcome) {
    match input {
        Input::Export(id) => {
            // A retry starts clean, as the Notes page's does.
            out.clear_notice = true;
            state.exported = None;
            if let Some(clip) = state.clip(&id) {
                let name = clips::export_name(clip);
                out.effects.push(Effect::Export { id, name });
            }
        }
        Input::Exported { id, result } => match result {
            Ok(Some(path)) => state.exported = Some((id, path)),
            Ok(None) => {}
            Err(e) => out.notice = Some(Notice::CouldNotExport(e)),
        },

        Input::DeleteRequested(id) => state.confirming_delete = Some(id),
        Input::DeleteCancelled => state.confirming_delete = None,
        Input::DeleteConfirmed(id) => {
            state.confirming_delete = None;
            if state.is_playing(&id) {
                state.playing = None;
            }
            if state.exported.as_ref().is_some_and(|(e, _)| *e == id) {
                state.exported = None;
            }
            state.items.retain(|c| c.id != id);
            out.effects.push(Effect::Delete(id));
        }
        Input::Deleted { id, result: Err(e) } => {
            log::warn!("Could not delete the clip {id}: {e}");
            out.notice = Some(Notice::CouldNotDelete(e));
            // The list dropped it already; read back what is there.
            out.effects.push(Effect::List);
        }
        _ => {}
    }
}

/// Read the folder.
pub async fn run_list() -> Input {
    Input::Loaded(clips::list(&clips::dir()).await.map_err(|e| e.to_string()))
}

/// Have the daemon speak `text`, and keep the audio as a clip made now. `None`
/// when [`Input::Cancel`] aborted it first.
pub async fn run_make(
    text: String,
    voice: Option<String>,
    abort: AbortRegistration,
) -> Option<Input> {
    Abortable::new(make(text, voice), abort)
        .await
        .ok()
        .map(Input::Made)
}

async fn make(text: String, voice: Option<String>) -> Result<Clip, String> {
    let wav = crate::daemon::client::v1::synthesize::synthesize(text.clone())
        .await
        .map_err(|e| e.to_string())?;
    let now = chrono::Local::now().naive_local();
    clips::create(&clips::dir(), wav, text, voice, now)
        .await
        .map_err(|e| e.to_string())
}

/// Read the clip `id`'s audio.
pub async fn run_samples(id: String) -> Input {
    let result = clips::samples(&clips::dir(), &id)
        .await
        .map_err(|e| e.to_string());
    Input::Ready { id, result }
}

/// Copy the clip `id` to where the user chose. Answers what
/// [`Input::Exported`] carries: where it went.
///
/// # Errors
///
/// When the clip could not be written there, saying where.
pub async fn export_to(id: &str, chosen: &Path) -> Result<Option<String>, String> {
    let path = clips::export_path(chosen);
    clips::export(&clips::dir(), id, &path)
        .await
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

/// Delete the clip `id`'s files.
pub async fn run_delete(id: String) -> Input {
    let result = clips::delete(&clips::dir(), &id)
        .await
        .map_err(|e| e.to_string());
    Input::Deleted { id, result }
}

#[cfg(test)]
mod tests {
    use super::*;

    type State = ClipsState<String>;

    fn clip(id: &str) -> Clip {
        Clip {
            id: id.to_string(),
            created: chrono::NaiveDateTime::parse_from_str("2026-11-20 10:00", "%Y-%m-%d %H:%M")
                .expect("a time"),
            text: format!("{id} says this"),
            voice: None,
            seconds: 1.0,
        }
    }

    #[test]
    fn nothing_is_made_of_an_empty_draft_or_while_one_is_making() {
        let mut state = State::default();
        assert!(
            update(&mut state, Input::Make { voice: None })
                .effects
                .is_empty()
        );

        state.draft = "  hello  ".into();
        let out = update(
            &mut state,
            Input::Make {
                voice: Some("Ava".into()),
            },
        );
        assert!(matches!(
            out.effects.as_slice(),
            [Effect::Make { text, voice: Some(v), .. }] if text == "hello" && v == "Ava"
        ));
        assert!(state.making.is_some());
        assert!(
            update(&mut state, Input::Make { voice: None })
                .effects
                .is_empty()
        );
    }

    #[test]
    fn a_made_clip_goes_to_the_top_and_clears_the_draft() {
        let mut state = State::default();
        state.put(clip("older"));
        state.draft = "words".into();
        update(&mut state, Input::Make { voice: None });

        update(&mut state, Input::Made(Ok(clip("newer"))));
        assert!(state.making.is_none());
        assert_eq!(state.draft, "");
        let ids: Vec<&str> = state.items.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["newer", "older"]);
    }

    #[test]
    fn cancelling_aborts_the_request() {
        let mut state = State::default();
        state.draft = "words".into();
        update(&mut state, Input::Make { voice: None });
        let handle = state.making.as_ref().expect("making").handle.clone();

        update(&mut state, Input::Cancel);
        assert!(handle.is_aborted());
        assert!(state.making.is_none());
    }

    #[test]
    fn audio_read_for_a_clip_no_longer_wanted_is_dropped() {
        let mut state = State::default();
        update(&mut state, Input::Play("a".into()));
        update(&mut state, Input::Play("b".into()));
        let out = update(
            &mut state,
            Input::Ready {
                id: "a".into(),
                result: Ok((Vec::new(), 24_000)),
            },
        );
        assert!(out.notice.is_none());
        assert!(
            state
                .playing
                .as_ref()
                .is_some_and(|p| p.id == "b" && p.player.is_none())
        );
    }

    #[test]
    fn a_deleted_clip_leaves_the_list_and_stops_playing() {
        let mut state = State::default();
        state.put(clip("a"));
        state.put(clip("b"));
        update(&mut state, Input::Play("a".into()));
        state.exported = Some(("a".into(), "/tmp/a.wav".into()));

        let out = update(&mut state, Input::DeleteConfirmed("a".into()));
        assert!(matches!(out.effects.as_slice(), [Effect::Delete(id)] if id == "a"));
        assert!(state.playing.is_none());
        assert!(state.exported.is_none());
        assert!(state.clip("a").is_none());
    }

    #[test]
    fn a_failed_delete_reads_the_folder_back() {
        let mut state = State::default();
        let out = update(
            &mut state,
            Input::Deleted {
                id: "a".into(),
                result: Err("read-only".into()),
            },
        );
        assert_eq!(out.notice, Some(Notice::CouldNotDelete("read-only".into())));
        assert!(matches!(out.effects.as_slice(), [Effect::List]));
    }

    #[test]
    fn export_names_the_file_after_the_clip() {
        let mut state = State::default();
        state.put(clip("a"));
        let out = update(&mut state, Input::Export("a".into()));
        assert!(out.clear_notice);
        assert!(matches!(
            out.effects.as_slice(),
            [Effect::Export { id, name }] if id == "a" && *name == clips::export_name(&clip("a"))
        ));
    }

    #[test]
    fn a_seek_waits_for_the_audio() {
        let mut state = State::default();
        update(&mut state, Input::Play("a".into()));
        update(&mut state, Input::Seek(0.5));
        let playing = state.playing.as_ref().expect("playing");
        assert!(playing.player.is_none());
        assert_eq!(playing.from, Duration::ZERO);
        update(&mut state, Input::Stop);
        update(&mut state, Input::Seek(0.5));
        assert!(state.playing.is_none());
    }

    #[test]
    fn the_voice_is_named_only_for_the_synthesis_model() {
        use crate::state::{VoiceChoice, VoiceResolution};
        let voice = VoiceState {
            resolution: Some(VoiceResolution {
                effective: Some("af_heart".into()),
                ..VoiceResolution::default()
            }),
            choices: vec![VoiceChoice {
                id: "af_heart".into(),
                label: "Heart".into(),
                kind: "preset".into(),
            }],
            target: Some(("kokoro".into(), "kokoro-82m".into())),
        };
        assert_eq!(
            voice_label(&voice, "kokoro", "kokoro-82m").as_deref(),
            Some("Heart")
        );
        assert!(voice_label(&voice, "kokoro", "another").is_none());
    }

    #[test]
    fn a_clip_is_about_two_and_a_half_words_a_second() {
        assert_eq!(estimated_seconds(0), 0);
        assert_eq!(estimated_seconds(1), 1);
        assert_eq!(estimated_seconds(10), 4);
    }

    #[test]
    fn a_clip_is_named_by_its_first_words() {
        assert_eq!(first_line("\n  Hello there.\nMore", 40), "Hello there.");
        assert_eq!(
            first_line("Stand-up moves to 10:30 tomorrow, in the small room.", 20),
            "Stand-up moves to\u{2026}"
        );
    }

    #[test]
    fn a_clock_is_minutes_and_seconds() {
        assert_eq!(clock(Duration::from_secs(6)), "0:06");
        assert_eq!(clock(Duration::from_secs(102)), "1:42");
    }

    #[test]
    fn when_says_today_yesterday_or_the_date() {
        let at =
            |s: &str| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").expect("a time");
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 25).expect("a day");
        assert_eq!(when(at("2026-11-25 09:40"), today), "Today, 09:40");
        assert_eq!(when(at("2026-11-24 17:12"), today), "Yesterday, 17:12");
        assert_eq!(when(at("2026-11-20 08:05"), today), "2026-11-20 08:05");
    }
}
