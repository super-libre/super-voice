// SPDX-License-Identifier: GPL-3.0-only
//! The Clips tab: the clips the app keeps, the text of the next one, the one
//! being made, and the one playing.
//!
//! The files are the truth (see [`crate::clips`]). A clip is written once,
//! when it is made, and never changed, so the list here is simply what was
//! last read, plus what was made or deleted since.

use std::time::Instant;

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

    /// Whether the clip `id` is the one playing.
    #[must_use]
    pub fn is_playing(&self, id: &str) -> bool {
        self.playing.as_ref().is_some_and(|p| p.id == id)
    }
}

/// A clip being made: the daemon synthesizing it, then the app writing it.
#[derive(Debug)]
pub struct Making {
    pub started: Instant,
    /// Aborting the task closes the request, which stops the synthesis.
    pub handle: futures_util::future::AbortHandle,
}

/// A clip playing, or about to: its samples are read first.
#[derive(Debug)]
pub struct Playing {
    pub id: String,
    /// `None` while the clip's audio is still being read.
    pub player: Option<Player>,
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
    fn a_new_clip_goes_to_the_top() {
        let mut state = ClipsState::default();
        state.put(clip("older"));
        state.put(clip("newer"));
        let ids: Vec<&str> = state.items.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["newer", "older"]);
    }
}
