// SPDX-License-Identifier: GPL-3.0-only
//! What the app draws that isn't code: the app's icon, the Lucide icons the
//! kit doesn't embed by default, and the kit's own.

use std::borrow::Cow;

use gpui_kit::assets::icon_assets;
use gpui_kit::{AssetSource, Result, SharedString};

// The Lucide icons the app draws beyond the kit's default set.
icon_assets!(
    ExtraIcons,
    [
        AppWindow,
        AudioLines,
        BookOpenText,
        Brain,
        Bug,
        Cloud,
        Download,
        FileVolume,
        KeyRound,
        Keyboard,
        LibraryBig,
        ListFilter,
        Microchip,
        NotebookPen,
        PackagePlus,
        ServerOff,
        ShieldX,
        SlidersHorizontal,
        Upload,
        Speaker,
        Type,
        UserRoundPen,
        Volume2,
        WandSparkles,
    ]
);

/// The app's icon, as `img()` finds it: the desktop entry's, in colour.
pub const APP_ICON: &str = "app/super-voice-app.svg";
const APP_ICON_SVG: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/apps/super-voice-app.svg");

/// Every asset the app loads: its own first, then the kit's.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == APP_ICON {
            return Ok(Some(Cow::Borrowed(APP_ICON_SVG)));
        }
        if let Some(icon) = ExtraIcons.load(path)? {
            return Ok(Some(icon));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all = gpui_kit::assets::Assets.list(path)?;
        all.extend(ExtraIcons.list(path)?);
        if APP_ICON.starts_with(path) {
            all.push(APP_ICON.into());
        }
        Ok(all)
    }
}
