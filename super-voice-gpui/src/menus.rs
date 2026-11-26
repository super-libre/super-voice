// SPDX-License-Identifier: GPL-3.0-only
//! The menu bar on macOS: the app's menu (About, Settings, Hide, Quit), Edit
//! and Window.
//!
//! Only macOS gets one. On Linux GPUI keeps the menus and draws nothing, so
//! the app sets none there, and binds none of their keys.
//!
//! Edit's items are the kit's text field actions, under the system's own
//! selectors (cut:, copy:, paste:, selectAll:): macOS enables each only
//! while a text field can take it. Quit and Close go the way of the
//! window's close button ([`crate::shell::Shell::close`]): a note being
//! recorded is saved first, and closing the window quits the app.

use gpui_kit::component::input as text;
use gpui_kit::{App, KeyBinding, Menu, MenuItem, OsAction, actions};

use crate::pages::Page;
use crate::shell::{self, Shell};
use crate::t;

actions!(
    super_voice_menu,
    [
        /// Show the About page.
        ShowAbout,
        /// Show the Settings page.
        ShowSettings,
        /// Hide the app.
        HideApp,
        /// Hide the other apps.
        HideOthers,
        /// Show every app again.
        ShowAll,
        /// Close the window, which quits the app.
        Quit,
        /// Minimize the window.
        Minimize,
        /// Zoom the window.
        Zoom,
        /// Close the window, which quits the app.
        CloseWindow,
    ]
);

/// The menu bar's message keys.
#[cfg(test)]
pub const KEYS: &[&str] = &[
    "app-title",
    "about-super-voice",
    "menu-settings",
    "menu-hide-app",
    "menu-hide-others",
    "menu-show-all",
    "menu-quit-app",
    "menu-edit",
    "menu-undo",
    "menu-redo",
    "menu-cut",
    "copy",
    "menu-paste",
    "menu-select-all",
    "menu-window",
    "menu-minimize",
    "menu-zoom",
    "menu-close",
];

/// Set up the menu bar, on macOS. Call it after [`crate::setup`], so the
/// kit's text field keys are bound: each item shows the keys bound to it.
pub fn install(cx: &mut App) {
    if !cfg!(target_os = "macos") {
        return;
    }
    cx.on_action(|_: &ShowAbout, cx| show(Page::About, cx));
    cx.on_action(|_: &ShowSettings, cx| show(Page::Settings, cx));
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Quit, cx| close(cx));
    cx.on_action(|_: &CloseWindow, cx| close(cx));
    cx.on_action(|_: &Minimize, cx| {
        shell::with_current(cx, |_, window, _| window.minimize_window());
    });
    cx.on_action(|_: &Zoom, cx| {
        shell::with_current(cx, |_, window, _| window.zoom_window());
    });
    cx.bind_keys([
        KeyBinding::new("cmd-,", ShowSettings, None),
        KeyBinding::new("cmd-h", HideApp, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
    ]);
    cx.set_menus(bar());
}

/// The menus, left to right. macOS shows the first under the app's name.
fn bar() -> Vec<Menu> {
    vec![
        Menu::new(t("app-title")).items([
            MenuItem::action(t("about-super-voice"), ShowAbout),
            MenuItem::separator(),
            MenuItem::action(t("menu-settings"), ShowSettings),
            MenuItem::separator(),
            MenuItem::action(t("menu-hide-app"), HideApp),
            MenuItem::action(t("menu-hide-others"), HideOthers),
            MenuItem::action(t("menu-show-all"), ShowAll),
            MenuItem::separator(),
            MenuItem::action(t("menu-quit-app"), Quit),
        ]),
        Menu::new(t("menu-edit")).items([
            MenuItem::os_action(t("menu-undo"), text::Undo, OsAction::Undo),
            MenuItem::os_action(t("menu-redo"), text::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action(t("menu-cut"), text::Cut, OsAction::Cut),
            MenuItem::os_action(t("copy"), text::Copy, OsAction::Copy),
            MenuItem::os_action(t("menu-paste"), text::Paste, OsAction::Paste),
            MenuItem::os_action(t("menu-select-all"), text::SelectAll, OsAction::SelectAll),
        ]),
        Menu::new(t("menu-window")).items([
            MenuItem::action(t("menu-minimize"), Minimize),
            MenuItem::action(t("menu-zoom"), Zoom),
            MenuItem::separator(),
            MenuItem::action(t("menu-close"), CloseWindow),
        ]),
    ]
}

/// Show `page` in the window.
fn show(page: Page, cx: &mut App) {
    shell::with_current(cx, |shell, window, cx| shell.go(page, window, cx));
}

/// Close the window as its close button does, which quits the app; quit at
/// once when no window is open.
fn close(cx: &mut App) {
    if !shell::with_current(cx, Shell::close_window) {
        cx.quit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every word the menu bar shows is in the English catalog.
    #[test]
    fn every_item_has_its_words() {
        for key in KEYS {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }

    /// The bar is the app's menu, Edit and Window, each with its items.
    #[test]
    fn the_bar_has_three_menus() {
        let bar = bar();
        let names: Vec<&str> = bar.iter().map(|menu| menu.name.as_ref()).collect();
        assert_eq!(names, ["Super Voice", "Edit", "Window"]);
        let actions = |menu: &Menu| {
            menu.items
                .iter()
                .filter(|item| matches!(item, MenuItem::Action { .. }))
                .count()
        };
        assert_eq!(bar.iter().map(actions).collect::<Vec<_>>(), [6, 6, 3]);
    }
}
