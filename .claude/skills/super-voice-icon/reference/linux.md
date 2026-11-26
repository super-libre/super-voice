# Super Voice Linux icon set

> Research for the Linux icon files. File paths in this note are relative to `brand/icon/linux/`, where the generated files and the build script live.
> **Updated since this research:** the symbolic icon is no longer traced separately. It is the shared small icon, drawn by `menubar_svg` in `brand/icon/macos/tools/build.py` (cut for 2x so the S shows on HiDPI panels). Sections below that describe its earlier geometry are history.


The Linux (freedesktop hicolor) icons for the app, built from
`brand/icon/master/super-voice-icon.svg`: the caped desk USB mic flying at
45° through orange-to-coral glass bands, with the pop roundel S.

To rebuild everything here after the master changes, run
`python3 brand/icon/linux/build.py` from the repository root. It needs
`rsvg-convert` (librsvg2-bin), `scour` and `shapely` (pip), Pillow, lxml and numpy. Then copy
`brand/icon/linux/hicolor/.` over `super-voice-gpui/resources/icons/hicolor/`.

## Files and where they're installed

| File (under `hicolor/`) | Installed to (prefix `/usr/local`) | Used by |
|---|---|---|
| `scalable/apps/super-voice-app.svg` | `share/icons/hicolor/scalable/apps/` | GTK/GNOME, COSMIC, the app's About page (`include_bytes!`), notifications |
| `{16,22,24,32,48,64,128,256,512}x…/apps/super-voice-app.png` | `share/icons/hicolor/<N>x<N>/apps/` | Qt/KDE (QtSvg can't draw the filters and gradients), AppStream's 48/64/128 cache, anything that loads a pixmap |
| `symbolic/apps/super-voice-app-symbolic.svg` | `share/icons/hicolor/symbolic/apps/` | panels, trays, GNOME's small and high-contrast contexts, KDE `-symbolic` lookups |
| `preview.png` (next to this file) | not installed | every size on light and dark, and the symbolic icon on panels |
| `symbolic-check.png` | not installed | the master and the symbolic glyph, sharp and blurred, at 16/24/32 px |
| `build.py`, `super-engine-installer-icons.patch` | not installed | the build, and the installer change (see "Wiring") |

The icon name stays `super-voice-app`. It's the desktop entry's `Icon=`, the
metainfo's `<icon type="stock">`, the daemon's notification `app_icon`, and the
name the installer, `uninstall.sh` and the justfile all use.

## Design decisions

**Tile with a transparent margin, not full bleed.** The master tile fills the
whole 512 canvas. Flathub's quality guidelines say to "avoid filling the entire
canvas", and suggest putting a custom-style icon "in one of the grid shapes,
e.g. a square". GNOME says app icons "shouldn't fill this space" and points to
the template's guides. GNOME's App Icon Preview template has a square guide
about 104 px wide on its 128 px canvas, so the tile is now 416 × 416
(104/128), centred with 48 px of transparent margin. It keeps the master's
corner shape (radius 112/512 of the tile), so it matches the macOS tile
(824/1024 ≈ 80.5%, against 81.25% here) and still reads as the brand. Only
16, 22, 24 and 32 px use a 1–2 px margin instead, because the safe-area
margin would waste 3–6 px of a panel icon.

**Shadows.** The two soft shadows under the glass bands, and the ink shadow
behind the mic, are inside the tile and part of the artwork. Nothing is drawn
outside the silhouette, which is what Flathub's "no baked-in shadows or glows"
and GNOME's "do not draw shadows outside the main silhouette" are about. The
shell adds its own drop shadow.

**Contrast.** The tile is a mid-saturation orange with a black ink outline
inside, so it holds up on both light and dark backgrounds. You can see both
in `preview.png`.

**Symbolic icon.** This is the master simplified, not a new drawing. `build.py`
measures the geometry from the master's mic, emblem and cape layers. It
rotates them back to a horizontal flight axis, rebuilds the shapes with
shapely, then turns them to the 45° heading and fits them to the 16 px canvas
with a 1 px margin. The glyph has the same desk-mic capsule (flat bottom,
rounded mesh dome), the seam band as a 1 px gap, the stubby side knobs at
mid-length, the S roundel on the housing as a round cut-out, and the cape
wedge behind, held 1 px clear of the mic. A letter S doesn't fit at 16 px,
so the roundel is a plain hole. The brief asked for no S; the roundel hole
keeps the housing recognisable instead.

`symbolic-check.png` shows the check: at 16, 24 and 32 px, the master, its ink
silhouette, the glyph, and both blurred side by side. The blurred pair should
read as the same object.

The glyph is one `<path>` (`fill-rule="evenodd"`, `fill="currentColor"`,
`class="ColorScheme-Text"`, with a `current-color-scheme` stylesheet), so:
- GTK and libcosmic recolour it to the foreground, because they override
  `fill`;
- KDE Plasma recolours it through the `ColorScheme-Text` class;
- anything else draws it in `#232629`.

A diagonal shape can't sit fully on the pixel grid. It is cleanest at 16,
32 and 64 px, the sizes GNOME recommends for symbolics. At 16 px it is dense.
Hand-hinting a 16 px PNG is the next step if that matters.

The small PNGs (16–32 px) are not hand-simplified. They are the master
itself, rendered with a tighter margin, so they are the same icon by
construction.

## SVG cleaned for librsvg

The master renders correctly in librsvg 2.58, but it uses three things that
older or stricter renderers don't handle. `build.py` rewrites them:

| In the master | Problem | In the shipped SVG |
|---|---|---|
| 9 nested `<svg>` layers with `overflow="hidden"` | Each viewBox maps 1:1, so they only add viewport clipping, and some renderers (QtSvg) handle nested `<svg>` badly | plain `<g>`. The tile's rounded `clipPath` already clips tighter |
| 2 × `<feDropShadow>` | librsvg only added it in 2.55.91/2.56 (2023). Ubuntu 22.04's 2.52, for example, would fail the filter, and the band wouldn't draw | `feGaussianBlur` + `feOffset` + `feFlood` + `feComposite` + `feMerge`, which is the same result in SVG 1.1 primitives |
| SVG 2 `href` on `<use>` | Older librsvg and Qt only read `xlink:href` | `xlink:href` |
| 6 unused `<pattern>`, unused gradients and clips, comments | dead weight | removed with scour |

The shipped SVG has no `<mask>`, `<image>`, `<text>`, CSS or external
references. It is 42 KB, down from 75 KB, has about 470 elements, and makes
about 20 `url(#…)` references, well under librsvg's limits of 1,000,000
loaded elements and 500,000 resolved references (`rsvg/src/limits.rs`).

## How it was checked

- The flattened full-bleed SVG against the master, both through
  `rsvg-convert` 2.58 at 512 px: mean difference 0.0004 per channel, max 7/255,
  and no pixel off by more than 8. The shipped safe-area SVG against the
  master scaled to 416 and inset by 48: mean 0.08 (only resampling).
- The shipped SVG through Inkscape 1.x against librsvg, composited on white:
  mean 0.44/255, p99 5/255. They look the same.
- `xmllint --noout` on both SVGs. `desktop-file-validate` on the desktop
  entry, and `appstreamcli validate` on the metainfo.
- `appstreamcli compose` over a staged `/usr` tree. It resolves
  `<icon type="stock">super-voice-app</icon>` and caches 48, 64, 64@2, 128 and
  128@2 PNGs with no hints.
- `gtk-update-icon-cache -f -t` over a staged hicolor tree with all 11 files.
  The system `hicolor/index.theme` lists every directory used here, including
  `22x22/apps`, `512x512/apps` and `symbolic/apps`.

## Wiring

- `super-voice-gpui/resources/icons/hicolor/` now holds the full set. The
  release workflows already copy `super-voice-gpui/resources/*` into the
  tarball, so the PNGs and the symbolic icon ship with no workflow change.
- `justfile`: `install-app` installs the SVG, every PNG size and the symbolic
  icon, then runs `gtk-update-icon-cache -f -t` on the theme root as before.
  `uninstall-app` removes them all. The new variables are
  `app_icon_theme_src` and `app_icon_sizes`. `app_icon_src` is unchanged.
- `uninstall.sh` removes the PNG sizes and the symbolic icon too, and its
  system cache refresh now passes `-t`. `/usr/local/share/icons/hicolor` has
  no `index.theme`, so without `-t` that refresh did nothing.
- `super-voice-install`: what gets installed is decided in the
  `super-engine-installer` crate, in the super-engine repository, pinned by
  rev. `super-engine-installer-icons.patch`, applied to rev `b565b31`, adds the
  PNG sizes and the symbolic icon as optional manifest entries: each is
  installed if the release carries it and skipped if not, so older releases
  still install. Retired apps also get those files removed. The patch adds a
  unit test, and `cargo test -p super-engine-installer` passes (138 tests).
  Until that lands and the pin is bumped, the release installer installs only
  the SVG, which still works everywhere GTK and libcosmic draw icons.
- The `.desktop` and metainfo files need no change. `Icon=super-voice-app`
  is a bare theme name with no path or extension, as the spec asks, and the
  metainfo already has `<icon type="stock">super-voice-app</icon>`.

### App ID

The metainfo `<id>` and the window's Wayland `app_id` are
`org.superlibre.SuperVoice`. The desktop file and the icon are
`super-voice-app`. They're linked by `<launchable type="desktop-id">` and
by `StartupWMClass=org.superlibre.SuperVoice`, which is valid for a native
install. Flatpak/Flathub would require both the desktop file and the icon to
be named after the app ID (`org.superlibre.SuperVoice.desktop`,
`org.superlibre.SuperVoice.svg`, `org.superlibre.SuperVoice-symbolic.svg`). The
rename also lets the shells match the window by `app_id` without
`StartupWMClass`. That change touches the installer, the daemon's
notification icon, the portal registration and the macOS recipe, so it isn't
made here. `build.py`'s `ICON` constant is the only place to change for the
icon files.

## Checking it on each desktop

After `just install-app`, or after running the installer:

- **Any desktop:** `ls /usr/local/share/icons/hicolor/*/apps/super-voice-app*`
  should list 11 files. `gtk4-icon-browser` (gtk-4-examples) or
  `gtk3-icon-browser` can look up `super-voice-app` and
  `super-voice-app-symbolic`.
- **COSMIC:** open the App Library and the launcher (`Super`). The install
  recipe restarts `cosmic-app-library` and `cosmic-launcher`; if the icon is
  stale, log out and back in. Launch the app and check the dock icon. It
  matches through `StartupWMClass` because the window's app_id is
  `org.superlibre.SuperVoice`. Settings → Appearance → Icons: try a non-default
  icon theme. hicolor is the fallback, so the icon should stay the same.
- **GNOME:** Activities overview and the dash, plus Alt+Tab and the
  notification from `voice record` when the daemon reports an error. Settings
  → Accessibility → High Contrast should not lose the icon. With the app
  open, `looking-glass` → Windows should show the app as Super Voice, not a
  generic window.
- **KDE Plasma:** Kickoff, the task manager at small and large sizes, and
  the system settings for Notifications. At panel sizes Plasma uses the
  22–48 px PNGs. `kiconfinder6 super-voice-app` (kf6 tools) prints the path it
  resolves, which should be the PNG or SVG under `/usr/local`.
- **AppStream / software centres:**
  `appstreamcli validate super-voice-gpui/resources/super-voice-app.metainfo.xml`,
  and `appstreamcli compose` as above to check the icon is found and cached.

## Research and sources

- **freedesktop Icon Theme Specification.** `hicolor` is the required
  fallback theme and is searched last. Apps should install "minimally … a 48x48
  icon in the hicolor theme", plus `scalable/apps` SVG for every size. PNG is
  the recommended bitmap format and SVG support is optional for
  implementations, which is why the PNGs are shipped next to the SVG.
  https://specifications.freedesktop.org/icon-theme/latest/
- **Flatpak conventions.** Icons are named after the app ID, are PNG or SVG,
  are square, go in `hicolor/<size>/apps` or `scalable/apps`, and are at most
  512 × 512. The desktop file is named `<app-id>.desktop` with
  `Icon=<app-id>`. https://docs.flatpak.org/en/latest/conventions.html
- **Flathub requirements.** An SVG, or at least a 256 px PNG.
  https://docs.flathub.org/docs/for-app-authors/requirements
- **Flathub quality guidelines (app icon).** Square; a reasonable
  footprint (don't fill the canvas or touch its edges); good contrast on
  light and dark; no baked-in shadows or glows; recognisable at small sizes;
  custom styles may sit in a grid shape such as a square.
  https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines/quality-guidelines
- **GNOME HIG, app icons.** A 128 px canvas that the icon shouldn't fill;
  follow the template's guides and baseline; shadows only inside the
  silhouette; every app should ship a symbolic variant.
  https://developer.gnome.org/hig/guidelines/app-icons.html
  Template: https://gitlab.gnome.org/World/design/app-icon-preview
  (`data/resources/templates/empty_project.svg`, with a square guide of about
  104 px on 128).
- **GNOME HIG, UI icons (symbolic).** 16 × 16 SVG, also used at 32, 64 and
  128 px; monochrome and recoloured in code; 2 px strokes; aligned to the
  pixel grid; orthogonal view, no perspective; it must still read when
  inverted. https://developer.gnome.org/hig/guidelines/ui-icons.html
- **KDE HIG, application icons.** Always the colorful style, with a baseline
  size of 48 px, a distinct silhouette and an apt metaphor.
  https://develop.kde.org/hig/icons/colorful/application/
  Monochrome recolouring uses the `current-color-scheme` stylesheet and the
  `ColorScheme-Text` class, and Plasma 6 requests `-symbolic` names.
  https://develop.kde.org/docs/plasma/theme/theme-colors
- **COSMIC / libcosmic.** I found no separate COSMIC icon guideline.
  libcosmic apps install into hicolor (`scalable/apps/<APP_ID>.svg`) and
  load icons by name from the freedesktop theme, with `-symbolic` names
  recoloured to the theme foreground. COSMIC's launcher, App Library and
  dock resolve the desktop entry's `Icon=` through the same theme lookup.
  The Fedora Discourse thread on COSMIC/GNOME/KDE icons points to the
  Flathub guidance.
  https://github.com/edfloreshz/cosmic-app-template ,
  https://discussion.fedoraproject.org/t/application-icon-logo-not-displaying-in-cosmic-gnome-and-kde-flatpak-prep/196435
- **AppStream.** `<icon type="stock">` names a theme icon with no path or
  extension. `appstreamcli compose` caches 48, 64 and 128 px (and @2)
  icons from the theme, as verified above. The metainfo `<id>` must be
  reverse-DNS, and `<launchable type="desktop-id">` links it to the desktop
  file. https://www.freedesktop.org/software/appstream/docs/chap-Metadata.html ,
  https://www.freedesktop.org/software/appstream/docs/sect-Metadata-Application.html
- **librsvg.** Supported features
  (https://gitlab.gnome.org/GNOME/librsvg/-/blob/main/devel-docs/features.rst),
  `feDropShadow` since 2.55.91 (`NEWS`, #743), SVG 2 `href` beside
  `xlink:href` (#568), and load limits in `rsvg/src/limits.rs`.
- **GTK symbolic recolouring and the `-symbolic` suffix.**
  https://docs.gtk.org/gtk4/gtk4-encode-symbolic-svg.html
