// SPDX-License-Identifier: GPL-3.0-only
//! Super Voice's theme: gpui-kit's, with an indigo accent, in light and dark
//! (`themes/super-voice.json`), following the system's appearance.
//!
//! The JSON is built into the app. With `SUPER_VOICE_THEMES_DIR` set to a
//! folder of theme files (`just app-watch` sets it to this crate's `themes/`),
//! the app reads its theme from there instead and applies each saved change at
//! once, through `ThemeRegistry::watch_dir`.

use std::path::PathBuf;

use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Window};

/// The theme for a light appearance.
pub const LIGHT: &str = "Super Voice Light";
/// The theme for a dark appearance.
pub const DARK: &str = "Super Voice Dark";

/// The folder of theme files to follow, when set.
pub const THEMES_DIR_VAR: &str = "SUPER_VOICE_THEMES_DIR";

const BUILT_IN: &str = include_str!("../themes/super-voice.json");

/// Register Super Voice's theme and use it, and follow `SUPER_VOICE_THEMES_DIR`
/// when it is set.
pub fn install(cx: &mut App) {
    if let Err(e) = ThemeRegistry::global_mut(cx).load_themes_from_str(BUILT_IN) {
        log::error!("the built-in theme does not load: {e:#}");
    }
    use_ours(cx);

    if let Some(dir) = std::env::var_os(THEMES_DIR_VAR).map(PathBuf::from) {
        log::info!("following theme changes in {}", dir.display());
        if let Err(e) = ThemeRegistry::watch_dir(dir, cx, use_ours) {
            log::error!("cannot follow the theme folder: {e:#}");
        }
    }
}

/// Use the registered Super Voice themes for light and dark, and apply the
/// one for the current mode. Run again after every reload of the folder,
/// which replaces the registered themes.
fn use_ours(cx: &mut App) {
    let registry = ThemeRegistry::global(cx);
    let (Some(light), Some(dark)) = (
        registry.themes().get(LIGHT).cloned(),
        registry.themes().get(DARK).cloned(),
    ) else {
        log::error!("no themes named {LIGHT:?} and {DARK:?}");
        return;
    };
    let theme = Theme::global_mut(cx);
    theme.light_theme = light;
    theme.dark_theme = dark;
    let mode = theme.mode;
    Theme::change(mode, None, cx);
}

/// Light or dark, as the system has it now.
pub fn follow_system(window: &mut Window, cx: &mut App) {
    Theme::sync_system_appearance(Some(window), cx);
}

/// Light or dark, whatever the system has.
pub fn set_mode(mode: ThemeMode, cx: &mut App) {
    Theme::change(mode, None, cx);
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::{ThemeMode, ThemeSet};

    /// The built-in file parses, and has the two themes by the names the app
    /// asks for, each in its own mode: a typo would leave the kit's
    /// default theme in place, with no accent.
    #[test]
    fn the_built_in_theme_has_light_and_dark() {
        let set: ThemeSet = serde_json::from_str(super::BUILT_IN).expect("the theme file parses");
        let mode = |name: &str| {
            set.themes
                .iter()
                .find(|theme| theme.name == name)
                .map(|theme| theme.mode)
        };
        assert_eq!(mode(super::LIGHT), Some(ThemeMode::Light));
        assert_eq!(mode(super::DARK), Some(ThemeMode::Dark));
    }
}
