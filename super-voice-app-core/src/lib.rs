// SPDX-License-Identifier: GPL-3.0-only
//! What Super Voice's apps share, with no UI library in it.
//!
//! The settings app draws its pages with libcosmic; whatever decides what they
//! show lives here, so another app can draw the same thing with its own
//! toolkit. So far that is the daemon client and its event stream, the app's
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
