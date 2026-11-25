// SPDX-License-Identifier: GPL-3.0-only
//! The Clips tab: the clips the app keeps, the text of the next one, the one
//! being made, and the one playing.
//!
//! The files are the truth (see [`crate::clips`]). A clip is written once,
//! when it is made, and never changed, so the list here is simply what was
//! last read, plus what was made or deleted since.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio::player::Player;
use crate::clips::Clip;

/// Everything the Clips tab holds. `B` is the next clip's editor buffer, the
/// app's toolkit's own (see [`TextBuffer`](crate::state::TextBuffer)).
#[derive(Debug, Default)]
pub struct ClipsState<B = String> {
    /// The clips, newest first.
    pub items: Vec<Clip>,
    /// Whether the first read has landed. Until it has, no clips means "not
    /// read yet", not "none".
    pub loaded: bool,
    /// What the next clip will say.
    pub draft: B,
    /// The clip being made.
    pub making: Option<Making>,
    /// The clip playing.
    pub playing: Option<Playing>,
    /// The clip whose Delete was pressed once and waits for the second.
    pub confirming_delete: Option<String>,
    /// The last export: which clip, and where it went.
    pub exported: Option<(String, String)>,
}

impl<B> ClipsState<B> {
    /// Put `clip` at the top of the list, where the newest goes.
    pub fn put(&mut self, clip: Clip) {
        self.items.retain(|c| c.id != clip.id);
        self.items.insert(0, clip);
    }

    /// The clip `id`, if the list has it.
    #[must_use]
    pub fn clip(&self, id: &str) -> Option<&Clip> {
        self.items.iter().find(|c| c.id == id)
    }

    /// Whether the clip `id` is the one playing, or paused.
    #[must_use]
    pub fn is_playing(&self, id: &str) -> bool {
        self.playing.as_ref().is_some_and(|p| p.id == id)
    }

    /// Whether the clip `id` is the one playing, and not paused.
    #[must_use]
    pub fn is_playing_on(&self, id: &str) -> bool {
        self.playing
            .as_ref()
            .is_some_and(|p| p.id == id && !p.paused)
    }
}

/// A clip being made: the daemon synthesizing it, then the app writing it.
#[derive(Debug)]
pub struct Making {
    pub started: Instant,
    /// Aborting the task closes the request, which stops the synthesis.
    pub handle: futures_util::future::AbortHandle,
}

/// A clip playing, about to (its samples are read first), or paused.
#[derive(Debug)]
pub struct Playing {
    pub id: String,
    /// `None` while the clip's audio is still being read, and while paused.
    pub player: Option<Player>,
    /// The clip's audio and its rate, once read: what a seek or a resume
    /// plays from.
    pub audio: Option<(Arc<[f32]>, u32)>,
    /// Where in the clip the player started: zero, or where it was sought to
    /// or resumed from. While paused, where it stopped.
    pub from: Duration,
    /// Paused: no player sounds, and it resumes from [`Self::from`].
    pub paused: bool,
}

impl Playing {
    /// The clip `id`, its audio still to be read.
    #[must_use]
    pub fn new(id: String) -> Self {
        Self {
            id,
            player: None,
            audio: None,
            from: Duration::ZERO,
            paused: false,
        }
    }

    /// How far into the clip it has played; `None` while its audio is read.
    #[must_use]
    pub fn position(&self) -> Option<Duration> {
        match &self.player {
            Some(player) => Some(self.from + player.position()),
            None => self.paused.then_some(self.from),
        }
    }

    /// How long the clip is; `None` while its audio is read.
    #[must_use]
    pub fn length(&self) -> Option<Duration> {
        if let Some((audio, rate)) = &self.audio
            && *rate > 0
        {
            #[allow(clippy::cast_precision_loss)]
            return Some(Duration::from_secs_f64(
                audio.len() as f64 / f64::from(*rate),
            ));
        }
        self.player.as_ref().map(|p| self.from + p.length())
    }

    /// How far it has played, from 0 to 1; 0 while its audio is read.
    #[must_use]
    pub fn fraction(&self) -> f32 {
        match (self.position(), self.length()) {
            (Some(at), Some(length)) if !length.is_zero() => {
                (at.as_secs_f32() / length.as_secs_f32()).clamp(0.0, 1.0)
            }
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    type ClipsState = super::ClipsState<String>;

    fn clip(id: &str) -> Clip {
        Clip {
            id: id.to_string(),
            created: NaiveDateTime::parse_from_str("2026-11-18 10:00", "%Y-%m-%d %H:%M")
                .expect("a time"),
            text: id.to_string(),
            voice: None,
            seconds: 1.0,
        }
    }

    #[test]
    fn a_held_clip_says_how_far_it_has_played() {
        let mut playing = Playing::new("a".into());
        assert!(playing.position().is_none());
        assert!(playing.fraction().abs() < f32::EPSILON);
        playing.from = Duration::from_secs(1);
        playing.player = Some(Player::held_at(
            Duration::from_secs(3),
            Duration::from_secs(1),
        ));
        assert_eq!(playing.position(), Some(Duration::from_secs(2)));
        assert_eq!(playing.length(), Some(Duration::from_secs(4)));
        assert!((playing.fraction() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_paused_clip_says_where_it_stopped() {
        let mut playing = Playing::new("a".into());
        playing.audio = Some((vec![0.0; 4_000].into(), 1_000));
        playing.from = Duration::from_secs(1);
        playing.paused = true;
        assert_eq!(playing.position(), Some(Duration::from_secs(1)));
        assert_eq!(playing.length(), Some(Duration::from_secs(4)));
        assert!((playing.fraction() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_new_clip_goes_to_the_top() {
        let mut state = ClipsState::default();
        state.put(clip("older"));
        state.put(clip("newer"));
        let ids: Vec<&str> = state.items.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["newer", "older"]);
    }
}
