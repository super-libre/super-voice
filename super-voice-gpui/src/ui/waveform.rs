// SPDX-License-Identifier: GPL-3.0-only
//! A live waveform of the daemon's microphone, drawn the way the kit's
//! `SpeechWaveform` draws one: a bar per reading, growing up and down from the
//! middle, the newest at the trailing edge; a dot for silence; the accent
//! while recording, muted otherwise.
//!
//! Built here rather than taken from the kit because the kit's waveform reads
//! a `SpeechState`, which records the microphone itself (an older cpal, no
//! device choice). The app's recordings are the daemon's, which reports only
//! a level and whether it sounds like speech: [`Levels`] keeps those, and
//! [`Waveform`] draws them.
//!
//! ```ignore
//! Waveform::new(&self.levels).live(self.microphone.recording).h(px(24.))
//! ```

use std::collections::VecDeque;

use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex};
use gpui_kit::{
    App, IntoElement, ParentElement as _, Pixels, RenderOnce, StyleRefinement, Styled, Window, div,
    px,
};

/// How many readings are kept: more than the widest waveform shows.
const KEPT: usize = 160;
/// A bar's width, and the space after it.
const BAR: Pixels = px(3.);
const GAP: Pixels = px(4.);
/// A reading under this is silence, drawn as a dot.
const QUIET: f32 = 0.04;

/// The microphone's last readings, oldest first.
#[derive(Debug, Clone, Default)]
pub struct Levels(VecDeque<f32>);

impl Levels {
    /// Add a reading, 0 to 1.
    pub fn push(&mut self, level: f32) {
        if self.0.len() == KEPT {
            self.0.pop_front();
        }
        self.0.push_back(level.clamp(0., 1.));
    }

    /// Forget every reading: a new recording starts.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Readings from `levels`, for made-up state.
    #[cfg(feature = "screenshot")]
    #[must_use]
    pub fn from_iter(levels: impl IntoIterator<Item = f32>) -> Self {
        let mut kept = Self::default();
        for level in levels {
            kept.push(level);
        }
        kept
    }
}

/// The waveform of some [`Levels`].
#[derive(IntoElement)]
pub struct Waveform {
    levels: Vec<f32>,
    live: bool,
    style: StyleRefinement,
}

impl Waveform {
    /// The waveform of `levels`: 24 px high and as wide as its parent lets
    /// it, unless styled otherwise.
    #[must_use]
    pub fn new(levels: &Levels) -> Self {
        Self {
            levels: levels.0.iter().copied().collect(),
            live: false,
            style: StyleRefinement::default(),
        }
    }

    /// Whether the microphone is recording: the bars take the accent.
    #[must_use]
    pub fn live(mut self, live: bool) -> Self {
        self.live = live;
        self
    }
}

impl Styled for Waveform {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Waveform {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = if self.live {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        };
        // The newest at the trailing edge; the oldest are cut at the leading
        // one once the row is full.
        h_flex()
            .h(px(24.))
            .min_w_0()
            .flex_1()
            .overflow_hidden()
            .justify_end()
            .items_center()
            .gap(GAP)
            .refine_style(&self.style)
            .children(self.levels.into_iter().map(move |level| {
                let bar = if level < QUIET {
                    BAR
                } else {
                    // A floor under any sound, so quiet speech still shows.
                    px(4. + 20. * level.max(0.15))
                };
                div()
                    .flex_shrink_0()
                    .w(BAR)
                    .h(bar)
                    .rounded_full()
                    .bg(if level < QUIET {
                        color.opacity(0.5)
                    } else {
                        color
                    })
            }))
    }
}
