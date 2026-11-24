// SPDX-License-Identifier: GPL-3.0-only
//! What Super Voice's apps share, with no UI library in it.
//!
//! The settings app draws its pages with libcosmic; whatever decides what they
//! show lives here, so another app can draw the same thing with its own
//! toolkit. So far that is the daemon client, the app's state, the rules that
//! decide what its pages show, the notes and clips the app keeps, and the
//! audio it records and plays itself.

pub mod audio;
pub mod clips;
pub mod daemon;
pub mod notes;
pub mod rules;
pub mod state;
