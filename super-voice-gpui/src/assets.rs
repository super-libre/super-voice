// SPDX-License-Identifier: GPL-3.0-only
//! What the app draws that isn't code: the app's icon, and the Phosphor
//! icons (src/icons.rs), its own and the ones drawn in place of the kit's.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

/// The app's icon, as `img()` finds it: the desktop entry's, in colour.
pub const APP_ICON: &str = "app/super-voice-app.svg";
const APP_ICON_SVG: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/apps/super-voice-app.svg");

/// Every asset the app loads. The kit's own Lucide icons are never served:
/// each path the kit asks for gets its Phosphor icon.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == APP_ICON {
            return Ok(Some(Cow::Borrowed(APP_ICON_SVG)));
        }
        Ok(crate::icons::svg(path).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(std::iter::once(APP_ICON)
            .chain(crate::icons::paths())
            .filter(|name| name.starts_with(path))
            .map(Into::into)
            .collect())
    }
}
