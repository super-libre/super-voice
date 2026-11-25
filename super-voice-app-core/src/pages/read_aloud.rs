// SPDX-License-Identifier: GPL-3.0-only
//! Reading aloud: where the shortcut reads its text from (the selection or the
//! clipboard), and a test that reads it now. Linux only: the daemon reads the
//! Wayland session's selection.
//!
//! The choice shows once the daemon has stored it. The test's progress follows
//! the daemon's speaking events, as everything spoken does, so only a refusal
//! comes back here.

use super_voice_shared::models::read_method::ReadMethod;

use crate::daemon::client::v1::speak::speak_read_command;
use crate::daemon::client::{get_read_method, set_read_method};

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Read where the text comes from: the page was opened, or the daemon
    /// said it changed.
    Load,
    /// What the daemon has; its default when it couldn't say.
    Loaded(ReadMethod),
    /// The user picked where the text comes from.
    Pick(ReadMethod),
    /// The daemon stored it, or why not.
    Saved(Result<ReadMethod, String>),
    /// Read the text aloud now.
    Test,
    /// The daemon refused the test: its words.
    TestFailed(String),
}

/// What the app should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// [`run_load`].
    Load,
    /// [`run_save`].
    Save(ReadMethod),
    /// [`run_test`].
    Test,
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    CouldNotSave(String),
    /// Nothing was selected to read.
    NothingSelected,
    /// The clipboard held nothing to read.
    NothingInClipboard,
    /// More text than the daemon reads at once.
    TooLong,
    /// Any other refusal: the daemon's words.
    CouldNotRead(String),
}

/// What an input came to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub effects: Vec<Effect>,
    /// Take the page's notice down.
    pub clear_notice: bool,
    pub notice: Option<Notice>,
}

impl Outcome {
    fn effect(effect: Effect, clear_notice: bool) -> Self {
        Self {
            effects: vec![effect],
            clear_notice,
            notice: None,
        }
    }
}

/// Take in `input`, and say what to do. `method` is where the text comes from,
/// as the daemon last said.
pub fn update(method: &mut ReadMethod, input: Input) -> Outcome {
    match input {
        Input::Load => Outcome::effect(Effect::Load, false),
        Input::Loaded(loaded) => {
            *method = loaded;
            Outcome::default()
        }
        Input::Pick(picked) => Outcome::effect(Effect::Save(picked), false),
        Input::Saved(Ok(saved)) => {
            *method = saved;
            Outcome {
                clear_notice: true,
                ..Outcome::default()
            }
        }
        Input::Saved(Err(e)) => {
            log::warn!("Could not save where reading aloud takes its text from: {e}");
            Outcome {
                notice: Some(Notice::CouldNotSave(e)),
                ..Outcome::default()
            }
        }
        Input::Test => Outcome::effect(Effect::Test, true),
        Input::TestFailed(error) => Outcome {
            notice: Some(refusal(*method, &error)),
            ..Outcome::default()
        },
    }
}

/// What a refused test says. The two refusals a user can fix in a moment say
/// how; anything else is the daemon's own words.
#[must_use]
pub fn refusal(method: ReadMethod, error: &str) -> Notice {
    if error.contains("nothing_to_read") {
        match method {
            ReadMethod::Selection => Notice::NothingSelected,
            ReadMethod::Clipboard => Notice::NothingInClipboard,
        }
    } else if error.contains("text_too_long") {
        Notice::TooLong
    } else {
        Notice::CouldNotRead(error.to_string())
    }
}

/// Read where the text comes from; the default when the daemon can't say.
pub async fn run_load() -> Input {
    let method = match get_read_method().await {
        Ok(method) => method.parse().unwrap_or_default(),
        Err(e) => {
            log::warn!("Could not read where reading aloud takes its text from: {e}");
            ReadMethod::default()
        }
    };
    Input::Loaded(method)
}

pub async fn run_save(method: ReadMethod) -> Input {
    Input::Saved(
        set_read_method(method.to_string())
            .await
            .map(|()| method)
            .map_err(|e| e.to_string()),
    )
}

/// Read the text aloud now. `None` once the daemon took it: its speaking
/// events say how it goes.
pub async fn run_test() -> Option<Input> {
    speak_read_command()
        .await
        .err()
        .map(|e| Input::TestFailed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_says_how_to_fix_it_when_it_can() {
        assert_eq!(
            refusal(ReadMethod::Selection, "nothing_to_read (HTTP 409)"),
            Notice::NothingSelected
        );
        assert_eq!(
            refusal(ReadMethod::Clipboard, "nothing_to_read"),
            Notice::NothingInClipboard
        );
        assert_eq!(
            refusal(ReadMethod::Clipboard, "text_too_long"),
            Notice::TooLong
        );
        assert_eq!(
            refusal(ReadMethod::Selection, "no bus"),
            Notice::CouldNotRead("no bus".into())
        );
    }

    /// The choice shows only once the daemon stored it.
    #[test]
    fn a_pick_shows_once_saved() {
        let mut method = ReadMethod::Selection;
        let pick = update(&mut method, Input::Pick(ReadMethod::Clipboard));
        assert_eq!(pick.effects, [Effect::Save(ReadMethod::Clipboard)]);
        assert_eq!(method, ReadMethod::Selection);
        update(&mut method, Input::Saved(Ok(ReadMethod::Clipboard)));
        assert_eq!(method, ReadMethod::Clipboard);
    }
}
