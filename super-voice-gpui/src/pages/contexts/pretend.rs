// SPDX-License-Identifier: GPL-3.0-only
//! The Contexts page's made-up state, for `just app-shot`: two contexts,
//! Coding in use and open in the editor, and 2 of 3 installed backends able
//! to use one. Scenes: `new`, a new context in the editor; `closed`, no
//! editor open; `empty`, no contexts at all.

use gpui_kit::{Context, Window};
use super_voice_app_core::daemon::client::v1::contexts::ContextCatalog;
use super_voice_app_core::pages::contexts::{self as page, Input, Reach};
use super_voice_shared::models::contexts::DictationContext;

use super::ContextsPage;

pub(super) fn fill(
    contexts: &mut ContextsPage,
    scenes: &[&str],
    window: &mut Window,
    cx: &mut Context<ContextsPage>,
) {
    let catalog = if scenes.contains(&"empty") {
        ContextCatalog::default()
    } else {
        ContextCatalog {
            active: Some("coding".into()),
            contexts: vec![
                context(
                    "coding",
                    "Coding",
                    "Technical dictation about a Rust desktop app.",
                    &[
                        "Rust",
                        "GPUI",
                        "libcosmic",
                        "cargo",
                        "tokio",
                        "Whisper",
                        "Voxtral",
                        "wgpu",
                        "serde",
                    ],
                ),
                context(
                    "email",
                    "Email",
                    "",
                    &["Ana Lucía", "Bjørn", "Priya", "Okonkwo"],
                ),
            ],
        }
    };
    page::update(&mut contexts.state, Input::Loaded(Ok(catalog)));
    contexts.reach = Some(Reach::Partly {
        accepting: 2,
        total: 3,
    });
    let input = if scenes.contains(&"new") {
        Some(Input::Create)
    } else if scenes.contains(&"closed") || scenes.contains(&"empty") {
        None
    } else {
        Some(Input::Edit("coding".into()))
    };
    if let Some(input) = input {
        page::update(&mut contexts.state, input);
    }
    contexts.sync_editor(window, cx);
    cx.notify();
}

fn context(id: &str, name: &str, prompt: &str, words: &[&str]) -> DictationContext {
    DictationContext {
        id: id.into(),
        name: name.into(),
        prompt: prompt.into(),
        vocabulary: words.iter().map(|w| (*w).to_string()).collect(),
    }
}
