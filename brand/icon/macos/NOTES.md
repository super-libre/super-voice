# macOS icon: notes

These files are the macOS app icon, its appearances, its Icon Composer source,
and the menu bar template, all built from `brand/icon/master/super-voice-icon.svg`
and the layers in `.claude/skills/icon-animation/layers/`.

Regenerate everything with:

    python3 brand/icon/macos/tools/build.py

The build needs Node with Playwright's Chromium, Python with Pillow, and
shapely. It runs on Linux: no Xcode, `iconutil`, `actool` or `sips` is needed.

## Files and where they are used

| File | What it is | Used by |
|---|---|---|
| `AppIcon.icns` | The app icon: 16 to 1024 px, @1x and @2x, on Apple's grid. | `just bundle-macos` copies it into `Super Voice.app/Contents/Resources/AppIcon.icns` (the `macos-icon` recipe). `Info.plist` names it with `CFBundleIconFile = AppIcon`. |
| `AppIcon.iconset/` | The same ten PNGs with `iconutil`'s names. | Anyone with `iconutil`: `iconutil -c icns AppIcon.iconset` rebuilds an equivalent `.icns`. Also usable as an Xcode `AppIcon.appiconset` source. |
| `AppIcon.icon/` | The Icon Composer document (`icon.json` plus `Assets/`), with the ink drawing in three groups plus the background fill. | Not wired yet. It needs Icon Composer and `actool` on a Mac; see `AppIcon.icon.README.md` and "Wiring the Tahoe icon in later" below. |
| `AppIcon.icon.README.md` | What the package holds and what to check in Icon Composer. | People. |
| `variants/AppIcon-dark-1024.png`, `variants/AppIcon-tinted-1024.png` | The dark and tinted appearances, on the grid. | References for Icon Composer's Dark and Mono previews, and for an asset catalog's dark and tinted slots. |
| `variants/*-fullbleed-1024.png` | The default, dark and tinted art as full-bleed squares with no mask. | Asset catalogs and any tool that applies the shape itself. |
| `menubar/MenuBarIconTemplate.pdf` | The menu bar glyph, 18 pt, as one vector path. | A future `NSStatusItem` or `MenuBarExtra`: add it to the app's resources and set `isTemplate = true` (or put "Template" in the asset name). |
| `menubar/MenuBarIconTemplate.svg` | The same glyph as SVG. | An asset catalog symbol image, or another toolkit. |
| `menubar/MenuBarIcon{16,18,22}Template{,@2x}.png` | Bitmaps, each cut for its own pixel size. | Toolkits that want PNGs, such as tray libraries. |
| `menubar/blur-check.png` | The master and the glyph side by side, sharp and blurred. | The check that the glyph reads as the same object as the master. |
| `preview.png` | Every size at 1:1 on light and dark backgrounds, the three appearances, the menu bar template on light and dark bars, and a light and a dark Dock. | People. |
| `tools/build.py`, `tools/render.mjs` | The build. | Run by hand when the artwork changes. |

## Wiring

`justfile` → `macos-icon` now copies the prebuilt `AppIcon.icns`.
`super-voice-macos/app-icon.swift` is removed. The copy is simpler and more
robust than rendering at bundle time, for three reasons:

- The old renderer drew artwork onto a grey tile. The new art carries its own tile, so that renderer's premise is gone.
- `app-icon.swift` loaded the SVG with `NSImage`. AppKit's SVG support doesn't do everything this artwork needs (filters such as the bands' drop shadows, patterns, nested `<svg>` elements). Chromium does, and Chromium is what `player.html` uses.
- The small sizes use their own simplified art (16 and 32 px have no hairline band rims and no light layer), which a plain `sips` resize can't do.

The cost is that the `.icns` must be rebuilt (`tools/build.py`) and committed
whenever the master changes.

`Info.plist` is unchanged. `CFBundleIconFile = AppIcon` is right for an
`.icns`. `CFBundleIconName` only matters once the bundle has an `Assets.car`,
and building one needs `actool`.

## Research findings

### macOS 26 (Tahoe), Liquid Glass and Icon Composer

- **What macOS 26 icons are.** On iOS, iPadOS and macOS 26, an app icon is a layered Liquid Glass object. The system adds specular highlights, refraction, translucency and shadows, and offers six appearances: default, dark, clear light, clear dark, tinted light and tinted dark. If you don't provide a variant, the system generates it. The canvas is 1024 × 1024 for iPhone, iPad and Mac. ([HIG: App icons](https://developer.apple.com/design/human-interface-guidelines/app-icons))
- **Bitmap icons are segmented.** If you supply a flat bitmap instead of a layered file, the system tries to split it into a foreground and a background so it can make the dark and tinted versions. Thin lines and small detail don't get glass. (same source)
- **How to prepare layers.** Layers must be square and unmasked, because the system applies the corner shape: "Providing layers with pre-defined masking negatively impacts specular highlight effects and makes edges look jagged." Leave out your own blurs, shadows, specular highlights and glows. Prefer SVG; use PNG for raster art or SVG features Icon Composer can't read. Prefer a fill to an imported background; if you do import one, make it full-bleed and opaque. ([HIG](https://developer.apple.com/design/human-interface-guidelines/app-icons), [Creating your app icon using Icon Composer](https://developer.apple.com/documentation/xcode/creating-your-app-icon-using-icon-composer))
- **Groups.** Use at most four groups. The groups become the depth layers the system renders. Liquid Glass settings live on groups: mode (individual or combined), specular, blur, refraction, translucency and shadow. Fill, opacity and blend mode can vary by appearance and platform. ([Icon Composer docs](https://developer.apple.com/documentation/xcode/creating-your-app-icon-using-icon-composer), [WWDC25 361](https://developer.apple.com/videos/play/wwdc2025/361/))
- **The `.icon` format.** A `.icon` is a package holding `icon.json` and an `Assets/` folder. Apple doesn't publish the schema.
  - Reverse-engineered top-level keys: `fill`, `fill-specializations`, `groups`, `supported-platforms`.
  - Group keys: `name`, `layers`, `lighting`, `specular`, `blur-material`, `translucency {enabled, value}`, `shadow {kind: neutral|layer-color|none, opacity}`, `blend-mode`, `opacity`, `position`.
  - Layer keys: `name`, `image-name`, `fill`, `glass`, `opacity`, `position`.
  - Colours are strings such as `srgb:r,g,b,a`.
  - Per-appearance values go in `*-specializations` arrays of `{appearance: dark|tinted, value}`.
  - Sources: [icon-composer-template](https://github.com/peterpoliwoda/icon-composer-template), and [IconKit](https://github.com/rozd/icon-kit), which models the same files.
  - `AppIcon.icon` follows this, so it is "documented well enough" only in this community sense. It still needs opening in Icon Composer.
- **Apple's dark and tinted guidance.** Keep the icon's features the same in every appearance. Base the dark icon on the light one, more subdued, and avoid "excessively bright images". "Color backgrounds generally offer the greatest contrast in dark icons." Clear and tinted are more subdued still. ([HIG](https://developer.apple.com/design/human-interface-guidelines/app-icons))
  - The dark variant here keeps the ink group, darkens the ground to ember brown, keeps the bands orange-to-coral but deeper, and dims the light.
  - The tinted variant is grayscale on the dark rendition, so the system tint colours the light values.
  - Clear is left to the system.

### `.icon`, `Assets.car`, `.icns`, and building without Xcode

- **Xcode compiles `.icon` with `actool`.** It goes into `Assets.car`, and `CFBundleIconName` names the icon. Per Apple, adding a `.icon` replaces the asset-catalog app icon. For older systems, Xcode generates a "similar-looking" flat icon from the `.icon`. If you want your own icon on older systems, you have to keep an asset catalog instead. ([Icon Composer docs](https://developer.apple.com/documentation/xcode/creating-your-app-icon-using-icon-composer))
- **Outside Xcode, it still takes Xcode 26's `actool`.** Tauri's bundler shows how:

      actool <Icon.icon> --compile <out> --app-icon Icon --include-all-app-icons \
          --platform macosx --target-device mac --minimum-deployment-target 26.0 \
          --output-partial-info-plist <plist> ...

  It then reads the icon name back with `assetutil --info`. ([tauri-bundler icon.rs](https://docs.rs/crate/tauri-bundler/2.9.2/source/src/bundle/macos/icon.rs))
- **Shipping a Tahoe icon and your own pre-Tahoe icon together is fragile.**
  - The usual setup is `CFBundleIconName` for the `Assets.car` icon on 26, and `CFBundleIconFile` for an `.icns` on older systems.
  - The undocumented `--enable-icon-stack-fallback-generation=disabled` flag stopped working in Xcode 26.1.
  - Developers report having to mix an `Assets.car` from `actool` with a hand-built `.icns`.
  - Sources: [Apple Developer Forums 794485](https://developer.apple.com/forums/thread/794485), [mjtsai](https://mjtsai.com/blog/2025/08/08/separate-icons-for-macos-tahoe-vs-earlier/), and Electron Forge's advice to ship both files ([Electron Forge](https://www.electronforge.io/guides/create-and-add-icons)).
- **`CFBundleIconFile` vs `CFBundleIconName`.**
  - `CFBundleIconFile` names an `.icns` in `Contents/Resources`, with the extension optional. Every macOS version reads it.
  - `CFBundleIconName` names an app icon inside `Assets.car`. Without an `Assets.car` it does nothing.
  - With both set, macOS 26 prefers the asset catalog.
  - This bundle has no `Assets.car`, so only `CFBundleIconFile` applies.

### What macOS 26 does with a legacy `.icns` ("squircle jail")

- **The gray tile.** macOS 26 shrinks any icon that doesn't fill its rounded-square shape and puts it on a gray tile. ([9to5Mac](https://9to5mac.com/2025/08/08/macos-tahoe-fix-gray-box-icons/), [Daring Fireball](https://daringfireball.net/2025/08/macos_26_tahoes_dead_canary_utility_app_icons), [heise](https://heise.de/-11075561))
- **An `.icns` that matches the template escapes it.** Developers report that an `.icns` that "follows Apple's template" shows without the gray border. Semi-transparent backgrounds, and art too small for the shape, trigger it. ([Xojo forum](https://forum.xojo.com/t/how-to-create-app-icons-for-macos-26-and-maintain-compatibility-with-older-oss/86527))
- **This is why the `.icns` has these properties:**
  - The tile is fully opaque.
  - It sits exactly on Apple's 824 grid, with continuous corners.
  - It has no baked drop shadow in the margin. The pre-Tahoe template had one, but on 26 it is just extra semi-transparent pixels outside the shape, and macOS 11–15 also draw the Dock icon fine without it.
- **Still unconfirmed.** Whether macOS 26 accepts this `.icns` without the gray tile can only be confirmed on a Mac.

### The grid on the 1024 canvas

- **Shape and size.** The macOS (Big Sur and later) production template is a 1024 canvas with an 824 × 824 rounded-square tile centred on it: a 100 px transparent margin, and continuous-curvature corners of nominal radius 185.4 (22.5 % of the tile). The 824 figure for the 1024 icon also appears in [1024jp's analysis of the Big Sur icon shape](https://speakerdeck.com/1024jp/rounded-rectangle-icons-since-big-sur).
- **Where content goes.** Keep primary content inside the grid's bounding box and all content inside the outer one ([HIG](https://developer.apple.com/design/human-interface-guidelines/app-icons), [Apple Design Resources](https://developer.apple.com/design/resources/)).
- **The 2025 grid is different.** In Icon Composer the layers fill the whole 1024 square and the system applies the (slightly rounder) shape. The `.icon` assets are therefore full-bleed 1024 squares, while the `.icns` is on the 824 grid.
- **How this build draws the corners.** `build.py` draws the corner as UIKit/AppKit's continuous-corner Bézier: the curve starts 1.5287 × r from each corner. The coefficients are the widely used reconstruction of `UIBezierPath`'s continuous corners. To compare against the real shape, overlay Apple's template on a Mac.

### `.icns` sizes and the file format

- **The ten iconset images.** `icon_16x16`, `icon_16x16@2x` (32), `icon_32x32`, `icon_32x32@2x` (64), `icon_128x128`, `icon_128x128@2x` (256), `icon_256x256`, `icon_256x256@2x` (512), `icon_512x512`, `icon_512x512@2x` (1024). ([Firefox's macOS icon docs](https://firefox-source-docs.mozilla.org/browser/branding/docs/UpdatingMacIcons.html))
- **The file structure.** An `.icns` starts with `icns` and a big-endian total length, then holds elements, each `type, length, data`. An optional `TOC ` lists the elements. ([Wikipedia: Apple Icon Image format](https://en.wikipedia.org/wiki/Apple_Icon_Image_format))
- **The PNG types:**
  - `ic11` = 16@2x
  - `ic12` = 32@2x
  - `ic07` = 128
  - `ic13` = 128@2x
  - `ic08` = 256
  - `ic14` = 256@2x
  - `ic09` = 512
  - `ic10` = 512@2x (1024)
- **16 and 32 at 1x.** The PNG types `icp4` and `icp5` "do not display properly" inside app bundles and can come out scrambled on 10.15.7 and later. This file therefore uses the classic RLE types: `is32` plus `s8mk` for 16, and `il32` plus `l8mk` for 32.
- **Building without Apple's tools.**
  - Pillow can write `.icns`, but it leaves out 16 and 32 at 1x and resizes one image into every size.
  - `png2icns` (libicns) isn't installed here.
  - `build.py` therefore has its own small writer (`write_icns`): a `TOC `, `is32`/`s8mk`, `il32`/`l8mk`, and PNG `ic11`, `ic12`, `ic07`, `ic13`, `ic08`, `ic14`, `ic09`, `ic10`.
- **Verification.** `verify_icns` parses the file back independently and checks:
  - the magic and the total length;
  - that every element's length adds up and the TOC matches the elements;
  - that each PNG member is pixel-identical to the iconset PNG;
  - that the RLE channels decode to exactly the 16 and 32 images.

  Pillow's own reader then opens it and finds all ten sizes, with 1024 as the largest. All of this passed.

### Menu bar (status item) template

- **Use a template image.** Apple: use a symbol or icon in black and clear. The system recolours the black for light and dark menu bars and for the selected state. The menu bar is 24 pt tall. ([HIG: The menu bar](https://developer.apple.com/design/human-interface-guidelines/the-menu-bar))
- **Size and format.** In practice the glyph is about 16–18 pt, with 22 pt the largest that fits. Use a PDF or a 1x/2x PNG pair with `isTemplate = true`; template images may hold only black and alpha. ([Alastair's Place](https://alastairs-place.net/blog/2013/07/23/nsstatusitem-what-size-should-your-icon-be/), [Qt's Cocoa tray code](https://code.qt.io/cgit/qt/qtbase.git/tree/src/plugins/platforms/cocoa/qcocoasystemtrayicon.mm?h=5.14), [cocoa-dev](https://lists.apple.com/archives/cocoa-dev/2020/Jun/msg00047.html))
- **What the glyph is.** It is a faithful simplification of the master, not a generic microphone. It is built geometrically from the layers' own shapes in the mic's frame, at the master's 45° heading:
  - the desk mic's housing: mesh dome on top, the seam (a clear line), the body with its flat bottom, and the stubby side knobs, grown by the master's 12-unit ink outline;
  - the `cape-00` outline behind it, parted from the housing by a clear gap;
  - the S roundel from `emblem.svg`, as a cut-out. Where the size allows (2x), it is a clear ring with the S cut from the disc. At 1x the roundel is a clear disc.
- **Cuts are never thinner than one device pixel**, and every size is cut for its own pixel grid.
- **`menubar/blur-check.png`** shows the master's mic, cape and emblem next to the glyph, sharp and blurred, at the menu bar sizes. They read as the same object.

### Pre-Tahoe macOS

- **macOS 11–15** show the `.icns` as is. There is no system mask, so the icon's own shape and margin are what people see. That is why the `.icns` carries the squircle and the transparent margin itself.
- **macOS 10.x** used free-form icons and shows this one the same way. (The bundle needs macOS 14 anyway: `LSMinimumSystemVersion`.)

### Small-size legibility

- **Apple's advice.** Prefer simple, filled shapes; avoid thin lines and fine detail that disappear at small sizes. Let the system add effects rather than baking them in. ([HIG](https://developer.apple.com/design/human-interface-guidelines/app-icons))
- **What `build.py` does:**
  - The 16 and 32 px members use a simplified rendition: no white band rims, no inner glow, no soft light. The orange ground and the mic's ink silhouette carry the icon.
  - They get a light unsharp mask after downsampling.
  - Everything is downsampled from a 4× supersampled render rather than resized from 1024.
- **Further work.** A hand-tuned, pixel-aligned 16 px would still improve on this.

## What can only be finished on a Mac

1. **Check the `.icns` in the Dock and Finder on macOS 26.** Confirm there is no gray tile ("squircle jail"). Run `iconutil -c iconset AppIcon.icns` to confirm Apple's tool reads every member.
2. **Open `AppIcon.icon` in Icon Composer.** Check the group order, the fill, and the dark and mono appearances; see `AppIcon.icon.README.md`.
3. **Wire the Tahoe icon in later:**
   1. Run `actool brand/icon/macos/AppIcon.icon --compile <Resources> --app-icon AppIcon --platform macosx --target-device mac --minimum-deployment-target 14.0 --output-partial-info-plist <plist>` with Xcode 26 or later.
   2. Copy the resulting `Assets.car` into `Contents/Resources`.
   3. Add `CFBundleIconName = AppIcon` to `Info.plist`, keeping `CFBundleIconFile = AppIcon` for macOS 14–15.
   4. Check on both systems which icon each one shows. Reports differ by Xcode version.
4. **Check the continuous-corner shape** against Apple's macOS template in Apple Design Resources.
5. **Add the menu bar item.** When the app gets a status item or `MenuBarExtra`, add `MenuBarIconTemplate.pdf` to its resources and check it on light, dark and tinted menu bars and in the highlighted state.
