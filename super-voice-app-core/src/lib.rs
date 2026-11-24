// SPDX-License-Identifier: GPL-3.0-only
//! What Super Voice's apps share, with no UI library in it.
//!
//! The settings app draws its pages with libcosmic; whatever decides what they
//! show lives here, so another app can draw the same thing with its own
//! toolkit. So far that is the daemon client and the types it reads.

pub mod daemon;
pub mod state;
