// SPDX-License-Identifier: GPL-3.0-only
//! What the Super Voice app draws, with no UI library in it.
//!
//! The app (super-voice-gpui) draws its pages with GPUI; whatever decides what
//! they show lives here, so the drawing stays apart from the logic and any
//! other front end could draw the same thing with its own toolkit. That is the daemon client and its event stream, the app's
//! state, the rules that decide what its pages show, what its pages' buttons
//! do, the words in every language, the notes and clips the app keeps, and the
//! audio it records and plays itself.

pub mod audio;
pub mod clips;
pub mod daemon;
pub mod events;
pub mod i18n;
pub mod languages;
pub mod notes;
pub mod pages;
pub mod rules;
pub mod state;
pub mod updater;
