// SPDX-License-Identifier: GPL-3.0-only
//! What every page may read or use: whether the daemon answers, how ready
//! each feature is, and a way to show another page.

use gpui_kit::{App, Entity, WeakEntity, Window};

use crate::connection::Connection;
use crate::pages::Page;
use crate::readiness::Readiness;
use crate::setup::Setup;
use crate::shell::Shell;

/// Handed to every page when it is built.
// `nav` is for the pages built later (Open Library, Open Updates).
#[allow(dead_code)]
#[derive(Clone)]
pub struct Shared {
    /// Whether the daemon answers: `connection.read(cx).is_down()`.
    pub connection: Entity<Connection>,
    /// How ready each feature is, and the GPU's memory:
    /// `readiness.read(cx).of(Feature::SpeechToText)`.
    pub readiness: Entity<Readiness>,
    /// What macOS has to allow: the Typed card's state, whether macOS shows
    /// notifications, the pane's name.
    pub setup: Entity<Setup>,
    /// Show another page: `shared.nav.go(Page::Library, window, cx)`.
    pub nav: Navigator,
}

/// Shows a page: Open Library, Open Updates.
#[allow(dead_code)]
#[derive(Clone)]
pub struct Navigator(pub(crate) WeakEntity<Shell>);

#[allow(dead_code)]
impl Navigator {
    /// Show `page`, once the current update is done (so a page may call it
    /// from its own handlers).
    pub fn go(&self, page: Page, window: &mut Window, cx: &mut App) {
        let shell = self.0.clone();
        window.defer(cx, move |window, cx| {
            _ = shell.update(cx, |shell, cx| shell.go(page, window, cx));
        });
    }

    /// `page` has finished the work it held the window open for
    /// ([`crate::pages::PageView::closing`]): the window closes once every
    /// page that waited has.
    pub fn finished_closing(&self, page: Page, window: &mut Window, cx: &mut App) {
        let shell = self.0.clone();
        window.defer(cx, move |window, cx| {
            _ = shell.update(cx, |shell, cx| shell.finished_closing(page, window, cx));
        });
    }

    /// Show `page`, then run its command `id` (one of its palette commands'
    /// ids), once the current update is done.
    pub fn go_and_run(&self, page: Page, id: String, window: &mut Window, cx: &mut App) {
        let shell = self.0.clone();
        window.defer(cx, move |window, cx| {
            _ = shell.update(cx, |shell, cx| shell.go_and_run(page, &id, window, cx));
        });
    }
}
