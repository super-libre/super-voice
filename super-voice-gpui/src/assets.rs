// SPDX-License-Identifier: GPL-3.0-only
//! What the app draws that isn't code: the app's icon, the Lucide icons the
//! kit doesn't embed by default, and the kit's own.

use std::borrow::Cow;

use gpui_kit::assets::icon_assets;
use gpui_kit::{
    App, AssetSource, ImageSource, ImgResourceLoader, Pixels, Resource, Result, SharedString,
    Window,
};

// The Lucide icons the app draws beyond the kit's default set.
icon_assets!(
    ExtraIcons,
    [
        AppWindow,
        AudioLines,
        BellOff,
        BookOpenText,
        Brain,
        Bug,
        Cloud,
        Download,
        FileVolume,
        KeyRound,
        Keyboard,
        LibraryBig,
        ListChecks,
        ListFilter,
        Microchip,
        NotebookPen,
        PackagePlus,
        ServerOff,
        ShieldX,
        SlidersHorizontal,
        Speaker,
        Type,
        Upload,
        UserRoundPen,
        Volume2,
        WandSparkles,
    ]
);

/// The app's icon at each size the desktop gets it, as `brand/icon/linux`
/// draws them: each size is drawn for its own pixels, small ones with a
/// tighter margin, so none of them is a scaled copy of another.
const APP_ICONS: &[(u32, &[u8])] = &[
    (
        16,
        include_bytes!("../resources/icons/hicolor/16x16/apps/super-voice-app.png"),
    ),
    (
        22,
        include_bytes!("../resources/icons/hicolor/22x22/apps/super-voice-app.png"),
    ),
    (
        24,
        include_bytes!("../resources/icons/hicolor/24x24/apps/super-voice-app.png"),
    ),
    (
        32,
        include_bytes!("../resources/icons/hicolor/32x32/apps/super-voice-app.png"),
    ),
    (
        48,
        include_bytes!("../resources/icons/hicolor/48x48/apps/super-voice-app.png"),
    ),
    (
        64,
        include_bytes!("../resources/icons/hicolor/64x64/apps/super-voice-app.png"),
    ),
    (
        128,
        include_bytes!("../resources/icons/hicolor/128x128/apps/super-voice-app.png"),
    ),
    (
        256,
        include_bytes!("../resources/icons/hicolor/256x256/apps/super-voice-app.png"),
    ),
    (
        512,
        include_bytes!("../resources/icons/hicolor/512x512/apps/super-voice-app.png"),
    ),
];

/// The asset path of the app's icon drawn `pixels` wide.
fn app_icon_path(pixels: u32) -> String {
    format!("app/super-voice-app-{pixels}.png")
}

/// Which of [`APP_ICONS`] to draw for `device_pixels` on screen: the
/// smallest that is at least that wide, so it is only ever scaled down,
/// and by as little as possible.
fn app_icon_size(device_pixels: f32) -> u32 {
    APP_ICONS
        .iter()
        .map(|&(size, _)| size)
        .find(|&size| f64::from(size) >= f64::from(device_pixels) - 0.5)
        .unwrap_or(512)
}

/// The app's icon, for `img()`, drawn `size` wide.
///
/// Drawn from the PNG made for that many device pixels, picked when it is
/// painted, so it follows the screen's scale. The scalable SVG would be
/// rasterised at 1024 px or more and shrunk on the GPU to 16 to 64, which
/// blurs it.
pub fn app_icon(size: Pixels) -> ImageSource {
    ImageSource::from(move |window: &mut Window, cx: &mut App| {
        let pixels = app_icon_size(f32::from(size) * window.scale_factor());
        let path = SharedString::from(app_icon_path(pixels));
        window.use_asset::<ImgResourceLoader>(&Resource::Embedded(path), cx)
    })
}

/// Every asset the app loads: its own first, then the kit's.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(&(_, png)) = APP_ICONS
            .iter()
            .find(|&&(size, _)| app_icon_path(size) == path)
        {
            return Ok(Some(Cow::Borrowed(png)));
        }
        if let Some(icon) = ExtraIcons.load(path)? {
            return Ok(Some(icon));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all = gpui_kit::assets::Assets.list(path)?;
        all.extend(ExtraIcons.list(path)?);
        all.extend(
            APP_ICONS
                .iter()
                .map(|&(size, _)| SharedString::from(app_icon_path(size)))
                .filter(|name| name.starts_with(path)),
        );
        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A size the desktop draws gets its own PNG; one between sizes gets the
    /// next larger, and anything past the largest gets the largest.
    #[test]
    fn the_icon_drawn_is_the_one_made_for_its_pixels() {
        assert_eq!(app_icon_size(16.), 16);
        assert_eq!(app_icon_size(32.), 32);
        assert_eq!(app_icon_size(64.), 64);
        assert_eq!(app_icon_size(20.), 22);
        assert_eq!(app_icon_size(40.), 48);
        assert_eq!(app_icon_size(80.), 128);
        assert_eq!(app_icon_size(2000.), 512);
    }

    /// Every size loads, as a PNG.
    #[test]
    fn every_icon_size_loads() {
        for &(size, _) in APP_ICONS {
            let bytes = Assets
                .load(&app_icon_path(size))
                .unwrap()
                .expect("the icon loads");
            assert!(bytes.starts_with(b"\x89PNG"), "{size} is a PNG");
        }
    }
}
