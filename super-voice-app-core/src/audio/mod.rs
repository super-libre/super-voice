// SPDX-License-Identifier: GPL-3.0-only
//! The app's own audio: capturing a reference recording for a cloned voice,
//! and playing back the speech clips it keeps.
//!
//! Speech the user asks for goes through the daemon, which owns the output
//! for it. Two things the daemon cannot do on the app's behalf are done here.
//! Recording: the microphone is opened for as long as the user holds the
//! button and for nothing else, which is a property worth keeping visible in
//! the process the user is looking at. And playing a clip: a clip is a file
//! the app keeps, not something the daemon has, so the app plays it.

pub mod player;
pub mod recorder;
