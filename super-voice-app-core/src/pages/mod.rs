// SPDX-License-Identifier: GPL-3.0-only
//! What happens on each page: the logic an app's buttons run.
//!
//! Each page has an `update` that takes what happened (an `Input`: a press, or
//! the answer to work started earlier), changes the page's state, and returns
//! the work to start (`Effect`s) and anything to tell the user (a `Notice`).
//! It does no work itself, so it is quick to call and easy to test. The page's
//! runners do the work, each answering with the next `Input`. An app wires the
//! two together on its own runtime, and says each notice in its own words.

pub mod assistants;
pub mod behavior;
pub mod clips;
pub mod contexts;
pub mod library;
pub mod models;
pub mod notes;
pub mod recording_test;
pub mod settings;
pub mod shortcuts;
pub mod speech;
pub mod updates;
pub mod voices;
