// SPDX-License-Identifier: GPL-3.0-only
//! `/v1` — one module per path, named for the path it answers on.
//!
//! Mirrors the daemon's `http/v1/` tree: a directory where a path has
//! sub-resources worth separating ([`backends`], [`pipeline`], [`registry`]); a
//! file otherwise, holding that path and any sub-path small enough to read
//! beside it. [`macros`] is the one module not named for a path, because it is
//! not an endpoint.
//!
//! Not every daemon path is wrapped — only what the settings app calls.

// Must come first: `#[macro_use]` puts the settings macros in scope for every
// module declared after it.
#[macro_use]
mod macros;

pub mod assistants;
pub mod backends;
pub mod contexts;
pub mod gpu_info;
pub mod ping;
pub mod pipeline;
pub mod registry;
pub mod settings;
pub mod shortcuts;
pub mod speak;
pub mod synthesize;
pub mod transcribe;
pub mod update;
pub mod voices;
