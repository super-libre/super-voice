// SPDX-License-Identifier: GPL-3.0-only
//! What happens on each page: the logic an app's buttons run.
//!
//! Each page has an `update` that takes what happened (an `Input`: a press, or
//! the answer to work started earlier), changes the page's state, and returns
//! the work to start (`Effect`s) and anything to tell the user (a `Notice`).
//! It does no work itself, so it is quick to call and easy to test. The page's
//! runners do the work, each answering with the next `Input`. An app wires the
//! two together on its own runtime, and says each notice in its own words.

pub mod about;
pub mod add_backend;
pub mod assistants;
pub mod behavior;
pub mod clips;
pub mod configure;
pub mod connection;
pub mod contexts;
pub mod library;
pub mod model_language;
pub mod model_voice;
pub mod models;
pub mod notes;
pub mod read_aloud;
pub mod readiness;
pub mod recording_test;
pub mod settings;
pub mod shortcuts;
pub mod speech;
pub mod updates;
pub mod voices;

/// Where an export dialog opens, for a note or a clip: the Documents folder,
/// or home. Left alone, a dialog opens wherever the app was started from.
#[must_use]
pub fn export_start() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").map_or_else(std::path::PathBuf::new, Into::into);
    export_start_in(&home)
}

/// [`export_start`] for the home folder `home`.
fn export_start_in(home: &std::path::Path) -> std::path::PathBuf {
    let documents = home.join("Documents");
    if documents.is_dir() {
        documents
    } else {
        home.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_export_opens_in_documents_or_else_home() {
        let home = tempfile::tempdir().expect("a folder");
        assert_eq!(export_start_in(home.path()), home.path());
        std::fs::create_dir(home.path().join("Documents")).expect("Documents");
        assert_eq!(export_start_in(home.path()), home.path().join("Documents"));
    }
}
