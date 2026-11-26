# Super Voice icon rubric

> The rubric an icon-critic agent used to score icon candidates: run new artwork through these tests before shipping it.


Each criterion is scored 1–5 (1 = fails, 3 = acceptable, 5 = excellent). Total is out of 40.
A **keep** needs a total of 30 or more, with no score below 3 on Recognition, Silhouette or S-match.
An **iterate** is 22–29, or a strong idea with one blocking flaw. A **drop** is below 22, or an idea that can't be saved.

| # | Criterion | 1 means… | 5 means… |
|---|-----------|----------|----------|
| R | **Recognition at 32 px** | At 32 px (and in a 3 px blur at 128 px) it turns into a coloured blob. You can't name the object. | At 32 px you can still say "a mic, flying", and the 16 px greyscale version still has one clear shape. |
| Si | **Silhouette and figure–ground** | The hero merges with its background: similar values, busy texture, or nothing separates it. | The hero is one solid shape with a clear outline. It holds up as a black-and-white threshold, and the background stays quiet. |
| C | **Concept clarity (voice + Super)** | It needs a caption. The mic or the "super" (cape, flight, S) is missing or unclear. | Voice and Super are read at once: a big mic, and a cape and flight that obviously belong to it. |
| S | **S perspective-match** | The S is a flat sticker. It doesn't match the angle, foreshortening or curvature of the body it sits on. | The S shares the body's tilt, foreshortening and wrap. It reads as printed on the surface, and its light matches. |
| P | **Craft and polish** | Line weights are inconsistent, there are tangents or awkward overlaps, the light source is muddy, or it's cramped against the tile edge. | One consistent light source and perspective, controlled detail, optical centring and good margins. Ready for Icon Composer layers. |
| D | **Distinctiveness (Dock test)** | Its shape and colour are generic. It looks like a podcast, recorder or Voice Memos clone. | You can pick it out of a Dock of 10 icons in under a second. The colour and shape combination is its own. |
| B | **Brand fit** | Off palette, or the accent fights the base colour. | Olive-gold or lime with an orange or coral accent. The palette is confident and also works in dark, tinted and clear looks. |
| A | **Appeal and personality** | Bland or clip-art. | It has charm, energy or wit. You'd want it as a sticker. If it's a character, its expression reads at 64 px. |

### Added after the user's later preferences

| # | Criterion | 1 means… | 5 means… |
|---|-----------|----------|----------|
| LG | **Faithful to the macOS 26 Liquid Glass language** (glass sets only) | Skeuomorphic illustration: mesh grilles, painted chrome, painted highlights, speed streaks, scenes. | Built from 3–5 simple primitives, as frosted translucent layers whose colours blend where they overlap, on a gradient tile lit from the top, with almost no texture. Reference: the original mockups in `logo-research.html` (Glass Capsule, Duet Glass, Equalizer, Sound Orb, Live Trace) and the macOS 26 gallery in `gal/`. |

- The glass totals are out of 45. In `review.json`, `comparable_total_of_40` rescales them to 40 so they can be ranked against ink and pop.
- **The mic must belong on a desktop.** Allowed: a USB desk mic on a small stand, a broadcast or podcast mic on a boom arm, a desk condenser in a shock mount, a headset boom, a gooseneck, or a clip-on or webcam-top mic. Not allowed: a stage ball-grille handheld, a 1950s chrome mic, a ribbon mic on a yoke, a candlestick, a shotgun mic or a horn. A wrong mic costs **−2 Brand fit**, and a borderline one (a pencil condenser, or a ring-mounted capsule) costs −1. Every entry in `review.json` records `mic_type` and `desktop_mic`.
- **No faces in Pop Art.** pop2's faces are a no-go, so no pop2 icon can be a keep. Its poses, gloves, staging and colour are still assessed.
- A **keep** now needs `comparable_total_of_40` ≥ 30, plus the gates on Recognition, Silhouette and S-match. After these preference changes, nothing reviewed so far meets that.

The user's weighting comes on top of the scores: **a big, bold mic as the clear hero** (they preferred round 5 over the looser round 6), and the S on the same angle as the mic body. When totals tie, the bigger and clearer hero wins.

## Tests used
- **512 px**: craft and detail.
- **256, 128, 64 and 32 px** on mid-grey, plus 32 px on dark and light: recognition and contrast.
- **32 px upscaled with nearest-neighbour**: how many pixels the mic actually gets.
- **128 px with `filter: blur(3px)`**: the squint test. Does one hero blob survive?
- **16 px greyscale**: the tray-size proxy. Does the value structure survive without colour?
- **48 px luminance threshold** (black and white at 50%): silhouette and figure–ground.
- **Dock test**: the 64 px finalists on a mid-grey bar between real macOS 26 icons.

## Research notes and sources

1. **Apple HIG, App icons (macOS 26 / Liquid Glass)**. macOS icons are a background layer plus one or more foreground layers. The system applies specular highlights, refraction, translucency and shadows, so artwork shouldn't bake in its own highlights, shadows between layers, bevels, blurs or glows. You supply square layers and the system masks them to the rounded rectangle. Icons must work in default, dark, clear and tinted looks. Implication: mockups with painted gloss and painted drop shadows will be doubled by the system, so separate hero, cape and background into flat layers.
   https://developer.apple.com/design/human-interface-guidelines/app-icons and https://developer.apple.com/icon-composer/ and WWDC25 session 220 https://developer.apple.com/videos/play/wwdc2025/220/
2. **Liquid Glass in practice**. Flarup: Liquid Glass "is going to have huge implications for app icon design"; Icon Composer controls the effect per layer except the base. iOS 27 / macOS 27 reacted to complaints about blurry glass by putting more of the glass into the artwork itself, and Rogue Amoeba praised the "sharper" icons with "superfluous Liquid Glass removed". Implication: glass belongs on one or two big shapes, not everywhere.
   https://www.flarup.email/p/through-the-liquid-glass and https://macrumors.com/2026/06/16/ios-27-revamps-app-icons and https://weblog.rogueamoeba.com/2026/06/26/free-the-icons/
3. **The squircle**. macOS 26 forces every icon into the squircle and shrinks older free-form icons into a grey tile. Rogue Amoeba and Gruber call this a loss of distinctiveness. Implication: our only way to stand out is the silhouette inside the tile and the colour of the tile, so the hero must break up the square's interior boldly.
   https://weblog.rogueamoeba.com/2026/06/26/free-the-icons/ and https://daringfireball.net/2026/07/eliminate_app_icon_squircle_jail
4. **Tahoe critiques**. Gruber called the Tahoe utility icons "objectively terrible": generic glyphs on same-y tiles erased their identity. TidBITS notes that busy detail under glass "gets messy". Implication: don't be a glyph on a gradient tile.
   https://daringfireball.net/2025/08/macos_26_tahoes_dead_canary_utility_app_icons and https://tidbits.com/2025/11/05/tahoes-terrible-icons-another-take/
5. **GNOME HIG, App icons**. Simple, geometric, buildable from basic shapes. Every app also needs a **symbolic** variant, a 16×16 monochrome SVG with 2 px strokes that the system recolours. The HIG discourages mascots and unrelated metaphors. Implication: COSMIC/GNOME want a quieter, simpler mark, and the tray icon must be a separate one-colour glyph (mic + cape), not the shrunk app icon.
   https://developer.gnome.org/hig/guidelines/app-icons.html and https://developer.gnome.org/hig/guidelines/ui-icons.html
6. **Tray icons**. KDE: a tray icon should be monochrome, with colour only to signal state; at 16 px keep 2 px margins and align to the pixel grid.
   https://develop.kde.org/hig/platform/tray/ and https://develop.kde.org/hig/icons/monochrome
7. **Michael Flarup (App Icon Book / Applypixels)**. Five aspects: scalability, recognisability, consistency, uniqueness, and not being a logo. Focus on **one single object**. Make it work on many backgrounds, and test it on the device. Uniqueness depends on "what EVERYONE ELSE is doing", so test it among neighbours. "Icons are not logos." Avoid text.
   https://www.smashingmagazine.com/2017/01/designing-better-app-icons/ and https://applypixels.com/blog/how-to-design-better-app-icons and https://blog.adobe.com/en/publish/2015/07/23/flarup-app-icon
8. **Squint and silhouette tests**. Blur or squint until detail goes. If a distinct silhouette or a clear internal split survives, the icon works small; if it turns into a blob, simplify. Check the solid-black silhouette, and look at it beside other icons.
   https://dev.to/alphashark/what-makes-a-logo-work-at-16x16-pixels-c4i and https://cursa.app/en/page/logo-design-foundations-scalability-and-legibility-checks and https://thisisglance.com/learning-centre/what-makes-app-icons-instantly-recognisable-to-users
9. **Optical weight and centring**. Weight follows filled area and colour, not the bounding box. Asymmetric shapes such as a play triangle, or a mic with a cape trailing one way, need nudging off mathematical centre. The blur test is also a check of weight balance.
   https://blog.icons8.com/articles/why-great-designers-break-alignment-rules/ and https://medium.com/design-bridges/optical-effects-9fca82b4cd9a
10. **Light, perspective and contrast**. One light source and one perspective across the whole icon. Microsoft: at least half the icon should pass 3:1 contrast on both light and dark backgrounds. elementary OS adds a thin, semi-transparent outline so icons read on any background. Make background layers lower in contrast than the foreground so the hero comes forward.
    https://learn.microsoft.com/windows/apps/design/iconography/app-icon-design and https://docs.elementary.io/hig/reference/iconography and https://www.skillshare.com/en/blog/10-fundamentals-of-effective-icon-design/
11. **Mascot and character appeal**. The eyes and mouth carry most of the personality, so get them right first. Eyebrow angle sets the mood: angled down reads as determined, raised reads as friendly. A mascot has to stay recognisable at small sizes, because scaling down strips detail. Keep the face and body sending the same message.
    https://sosfactory.com/how-to-design-a-mascot-logo and https://adobe.com/creativecloud/design/discover/mascot-logo-design.html
