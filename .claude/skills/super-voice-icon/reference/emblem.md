# Super Voice "S" emblem: reference spec

> Research and design spec for the S emblem ("Voice crest"). The shipped emblem is `../layers/emblem.svg` (the pop roundel variant). The original working files this note mentions (`src/`, `paths/`, `emblem.svg`, `wrapped.svg`) are not kept in the repo; `images/emblem.png` and `images/emblem-wrapped.png` show the results.


Original design for the caped-mic icon. Everything is drawn as paths, with no font. The geometry comes from `src/geom.py`, and `src/build.py` regenerates every file in this folder. Run them with the venv in `work/` (`work/venv/bin/python src/build.py` from `work/`, or copy them there).

Files:
- `emblem.svg` / `.png`: the master "Voice crest" flat and bevelled, the letter construction, three alternative containers (roundel, hex plate, no container), and the master in ink, glass and pop colours with its 64 px reduction.
- `wrapped.svg` / `.png`: the emblem wrapped on the stage mic and the desk mic, in the `translate(248 268) rotate(35) scale(1.18)` frame, in all three styles.
- `flat-vs-wrap.png`: the same emblem pasted flat and wrapped on the handle.
- `test-64.png`, `test-32.png`: the 64 px and 32 px detail levels on both mics in all three styles, zoomed with nearest-neighbour pixels (the `*-native.png` files are actual size).
- `paths/`: **reusable layered path data**. `emblem-flat-lod{512,64,32}.svg` holds the flat emblem in emblem units. `wrapped-stage-lod*.svg` and `wrapped-desk-lod*.svg` are already wrapped into the local mic coordinates, so you can paste them inside your icon group. Each layer has an `id`: keyline, border, inkey, field, rule_o, rule_i, letter_key, letter.
- `tiles/`: single 512 tiles per mic, style and detail level.

## 1. What the current badges do (before)

| Icon | Badge | Size (local u) | Fraction of body width | Construction |
|---|---|---|---|---|
| Pop stage (bs-pop2) | ellipse at y = -16 | 54 × 72 (+8 stroke) | 0.64 of the handle | single black ring, monoline stroked S |
| Ink stage (bs-ink2) | circle r 34, squashed to 0.86 | 58 × 68 | 0.70 | single cream ring, zig-zag polyline S |
| Pop desk (bs-pop3) | black ellipse on the mesh, y = -76 | 62 × 82 | 0.49 of the body | flat, no ring |
| Ink desk (bs-ink3) | rounded rect on the mesh | 66 × 60 | 0.52 | single ring |
| Glass desk (bs-glass3) | circle r 34 | 68 | 0.54 | rim gradient |

All of them are completely flat: they ignore the curvature of the handle or dome. Each has a single outline, and the S is a thin stroked line rather than a heavy display letter. They read as a "label" or "button", not as a chest emblem.

## 2. Superhero emblem conventions (research summary)

- **One bold mark, centred on the chest**: a letter or a simple symbol, sized to dominate the torso (roughly a third to a half of the chest width). TV Tropes "Chest Insignia"; Creative Bloq's best superhero logos.
- **Containers**: a shield, an inverted triangle or pentagon (Superman's, which began as a police-badge-like triangle in 1938 and settled about 1940–45), a circle (Flash's bolt in a circle, 1956; the X-Men's X in a circle), an oval (Batman's 1960s–80s yellow oval), a star (Captain Marvel's eight-point star), or no container (Batman's later bare bat, Spider-Man's spider). Wikipedia "Superman logo"; Fabrik Brands logo histories.
- **The letter is custom-drawn, not typeset.** Heroes' letters are heavy display forms with flared or cut terminals and strong contrast. Superman's S was "created entirely to be in the logo" and gained serifs by 1939.
- **Two colours plus a dark keyline**: field colour A, letter colour B, and a border band in B so the letter's colour frames the field (yellow/red, red/yellow, white/black). A dark outer keyline separates the emblem from the suit.
- **Double outline**: an outer keyline, a coloured border band, then an inner keyline or hairline rule before the field. This is what makes a mark read as an *emblem* rather than a sticker.
- **Real costumes**: emblems are separate pieces. They are 3D-printed or sculpted masters cast in rubber or latex, bevelled EVA foam (cosplay guides recommend a 45° edge over a blunt 90° one), appliqué, or embossed metal. On camera this reads as a raised plate with a **bevelled edge** (one lit facet on the light side, one shaded), a **thin thickness edge** on the shadow side, and a **cast shadow** on the suit. CGTrader emblem replicas; Kamui Cosplay; Plexi / Popverse; Dremel.
- **On a curved body the emblem wraps.** Its horizontal lines follow the same ellipses as the body's cross-sections (a label on a bottle follows the rim ellipses), its sides are foreshortened toward the silhouette, and its shading follows the cylinder: dark at both edges with a highlight strip on the lit side. Proko and Artyfactory on cylinder ellipses; PTC IsoDraw "wrapping around a cylinder"; Adobe community threads on cylinder wraps (Envelope Distort mesh 3×1 or Arc warp).
- **Small sizes**: use one or two solid colours and one or two bold shapes; turn thin strokes into fills; merge detail; check that the mark passes at the smallest size and set the minimum above it; squint at 32 px. Provide a reduced variant. Cursa scalability checks; wildandfreetools favicon contrast; Apple HIG (1999) small-icon advice; IBM app-icon sizes.

## 3. The design

### 3.1 Letterform: a custom geometric heavy S
The S is built on a centreline of straight runs joined by large fillets, then thickened.
- The letter box is **52 × 64 u inside the crest** (w/h 0.81). The stroke weight is **t = 13.6 u (0.21 h)**, and at 64 px the letter goes to **68 × 56, t = 16.5 (0.24 h)**.
- **The terminals are cut on parallel 24° slants**, the outer edge longer, so the tips point outward and down like a beak. The cut is point-symmetric, so the top and bottom terminals mirror each other through the centre. The slant echoes the climbing motion. The apertures stay open, so the letter never closes into an 8.
- Optical corrections: the **upper bowl is 6% narrower** than the lower one, and the **spine is a gentle diagonal** falling from left to right, sitting 1.2% of h above centre.
- The corners are almost semicircular fillets (radius equal to half the bowl's height), which give a squarish athletic or racing letter rather than a calligraphic one.
- Optional **inline stripe**, at 512 px only: a 0.2 t line in the field colour along the centreline. Drop it below 256 px.

### 3.2 Master container: the "Voice crest"
An **upright squircle** (superellipse n = 3.4, 86 × 100 u) with a **short speech-bubble tail at the lower left**. It is the product's own idea: a hero crest that is also a speech bubble. On the mic the tail points back toward the cape, so it doubles as a motion streak. It has no point at the bottom and no flat-topped shoulders, so it never reads as a heraldic or Superman-type shield.

**Double-outline construction** (offsets from the crest edge C, in emblem units):

| Layer | Offset | Colour | 512 | 64 | 32 |
|---|---|---|---|---|---|
| keyline | C + 4.5 (32 px: + 6) | K dark | ✓ | ✓ | ✓ |
| border band | C → C − 6.5 | B (letter colour) | ✓ | ✓ (8 wide) | — |
| inner keyline | C − 6.5 → C − 9 | K | ✓ | — | — |
| field | inside C − 9 | A | ✓ | ✓ | ✓ (fills C) |
| hairline rule | C − 12.5 → C − 14.1 | B | ✓ (≥ 256) | — | — |
| letter keyline | S + 3 | K | ✓ | ✓ | — |
| letter | S | B | ✓ | ✓ heavier | ✓ 62 × 74, t 20.5 |

So the master emblem is 95 × 106 u including the keyline and tail, in emblem units.

### 3.3 Bevel and raised version (on top of the flat layers)
The light comes from screen upper-left, which is **local (−0.94, −0.34)** inside `rotate(35)`. That matches the handle's highlight strip and the ball's glint.
1. **Cast shadow**: the keyline shape offset by (+3.5, +6), black at 22–28%.
2. **Thickness edge**: the keyline shape offset 3.2 u away from the light, in K. Also offset the letter keyline 2.4 u.
3. **Border and letter bevel, b = 3 u**: fill the shape with B_dark, then add the lit crescent `shape − translate(shape, −LIGHT·b)` in B_lit, then the face `shape.buffer(−0.85 b)` in B.
4. **Recessed field**: a crescent `field − translate(field, −LIGHT·3.2)` in A_dark on the light-facing inner edge, so the field sits below the border.
5. **One glint** on the top bowl of the letter (white at 55%).
6. **Wrapped only**: the cylinder shading overlay (section 5.3), clipped to the keyline.
Below 128 px, drop steps 3–6 and keep at most the cast shadow.

### 3.4 Alternatives (in `emblem.svg`)
- **B. Roundel**: a circle r 48 with the same double outline. The most classic and the most compact, and the safest at 32 px. It is generic: a letter in a circle is a convention, not anyone's mark.
- **C. Hex plate**: an elongated octagon (a hexagon with the top and bottom points flattened), 94 × 100. It reads as a "badge plate", a bit industrial.
- **D. No container**: the S alone (88 × 70, t 18.5) with a sticker cut of keyline, border and inner keyline. This gives the biggest letter for the space and is good for the desk dome. Its "emblem" read is weaker, though.

## 4. Colour pairing per style

| Style | K (keylines) | B (letter + border) | A (field) | Bevel | Notes |
|---|---|---|---|---|---|
| **Pop** | #141414 | #ffe14a yellow | #ff5a36 coral-red | B_lit #fff8c9, B_dark #d49a10, A_dark #b8361b at 55% | Classic hero pairing. On the lime tile it works as is. On the olive-gold tile (#d2c62a), keep the red field; yellow on olive needs the black keyline. Optional inline stripe in A, plus off-register or Ben-Day dots in the B_dark facets. |
| **Ink** | #171614 | #fdf8ec paper | #171614 ink | lit = white; dark facets = 35° hatching (`#hatch` pattern, 4 u pitch, 1.6 u lines) | Two-colour engraving: the double outline is paper border plus paper hairline on an ink field. On the red tile the tile red must not be used inside the emblem; keep it two-colour. |
| **Glass** | #1d2204 at 55% | gradient white → #e9f5a8 → #a7bb38 (rim); letter white → #cfe36a | gradient #4b5510 → #1f2504 | lit crescent white at 85%; letter shadow black at 35% | The keylines become translucent dark rims and the inner rule is white at 90%. **At ≤ 64 px switch to opaque values**: field #20260a, letter #ffffff, keyline #141804. The translucent version collapses at 32 px. |

## 5. Size and placement

The current badges are about 72–82 u tall. **The new emblem is 1.35–1.5× that linearly, and about 2× in area.**

### Stage mic (earlier handheld stage-mic studies; handle radius r(y) from its handle curve)
- Centre at **y0 = +5**, with emblem scale **s = 1.0**. The keyline spans y −50 → +61, just under the collar (the collar's stroke ends at about −53).
- The handle radius is 43.5 at y = −50, 40 at y = 5 and 36 at y = 56.
- The flat emblem is 102 u wide including the tail (about 46 u of arc to the right of centre and 54 u to the left), wrapped over **±62–75° of the handle**.
- **Projected width 75 u = 0.94 of the handle's width at the emblem centre** (border band 71 u = 0.89). Projected height 111 u, which is 1.39× the pop badge (80 u with stroke) and 1.5× the ink badge.
- Drop the optional on/off switch: the emblem covers it.

### Desk mic (desk-mic.md geometry: cylinder R 63, dome above y = −87)
- Centre at **y0 = −76**, scale **s = 1.08**. The top crosses onto the dome; the keyline ends at −130 projected (−134 before the dome compresses it) and its bottom at −14, just touching the seam top. Use y0 = −78 if you want a gap.
- **Projected width 96 u = 0.77 of the body width** (border 90 u = 0.71). Projected height 117 u, 1.3× the pop badge (90 u with stroke).
- **The housing (solid part) is not recommended**: the mute button (y 36) and knob (y 94) leave no room. If the controls are dropped, centre the emblem at y 70 with s 1.0 on the plain cylinder.

### Legibility (see `test-64.png`, `test-32.png`)
- **64 px** (1 u = 0.148 px): the emblem is about 16 px tall and the letter stroke about 2.4 px. All three styles read with the 64 px detail level. The 512 px level also survives at 64 for pop and ink.
- **32 px** (1 u = 0.074 px): the emblem is about 8 px tall. Only the **32 px level** reads as an "S": a solid field, a 6 u keyline and a letter weight of 20.5 u ≈ 1.5 px. Pop and ink read clearly. Glass reads only with the opaque small-size values. The mic outlines in the test tiles are at their 512 weight; the designers' own small-size rules for the mic still apply.

## 6. Wrap recipe

Flat emblem point (x, y) in emblem units → local mic point (X, Y). `s` is the emblem scale, `y0` the emblem centre, and `k` the minor/major ratio of the body's cross-section ellipses (0.14 matches the curved ball and collar seams. Use **k = 0** if that icon draws its seams as straight lines; then only the horizontal foreshortening remains).

### 6.1 Cylinder or tapered handle (stage mic)
```
u = s·x                      # arc length round the body
v = s·y                      # arc length along the body
Yl = y0 + v
r  = r(Yl)                   # local radius: handle curve M-44 -60C-42 -10 -31 120 -27 166 (right edge)
θ  = clamp(u / r, ±1.52)     # radians; the emblem uses up to ±75°
X  = r·sin θ
Y  = Yl + k·r·(cos θ − 1)    # horizontal lines become ellipse arcs; centre stays at y0
```

### 6.2 Dome plus cylinder (desk mic, R = 63, dome equator y = −87)
```
Yl = y0 + s·y
if Yl ≥ −87:  Y' = Yl,  r = R
else:         φ = (−87 − Yl)/R,  Y' = −87 − R·sin φ,  r = R·cos φ   # bend the vertical arc over the dome
θ  = clamp(s·x / R, ±1.52)                                         # angle from the BASE radius ...
X  = r·sin θ                                                       # ... so the decal tapers with the dome
Y  = Y' + k·r·(cos θ − 1)
```
Densify the paths first (segments of 1.5 u or less), map every vertex, then simplify (tolerance 0.12 u). `map_geom()` in `src/geom.py` does exactly this, and the files in `paths/` are its output.

### 6.3 Doing it by hand in SVG or Illustrator (no script)
1. **Horizontal scale gradient (5 bands)**: slice the flat emblem into 5 vertical bands of equal arc length. Scale each band horizontally by the cylinder's slope, cos θ at the band centre: for the stage handle (±70° total) about **0.56, 0.88, 1.0, 0.88, 0.56** (band centres at 0°, ±28°, ±56°). Then butt the bands together. On a cylinder the projected x of a band edge is r·sin(edge angle).
2. **Curved baselines**: shift each band down by k·r·(1 − cos θ) relative to the centre band, so the outer bands rise toward the silhouette (about 3 u on the handle and about 5 u on the desk body). Or in Illustrator, use Envelope Distort › Make with Mesh, 1 row × 4–5 columns, pulling the outer columns in and up. Use Effect › Warp › Arc only for the gentle baseline curve (bend about 8–10%), never for the horizontal compression.
3. **Dome (desk)**: additionally scale the bands above y = −87 vertically by cos φ and horizontally by cos φ (where φ is the band's height on the dome), so the top of the crest narrows and flattens toward the top of the dome.
4. **Shading**: overlay a horizontal `linearGradient` in local coordinates (`gradientUnits="userSpaceOnUse" x1="−r" x2="r"`) with these stops: black 42% → black 8% at 0.14 → **white 38% at 0.27 (the highlight strip, about −0.45 r)** → transparent at 0.36 → black 4% at 0.72 → black 50% at 1. Clip it to the keyline. On the desk dome, add a radial darkening around (0, −87), r 63, from 55% → 100% at 0 → 35% black.
5. Keep the light direction fixed in local coordinates (−0.94, −0.34) for the bevel crescents. Compute them flat and wrap them with the rest, so they bend with the surface.

## 7. Legal boundary: what this design avoids

Designers should **not** reintroduce any of these:
- **A five-sided "diamond" shield** (a flat top, angled shoulders and a bottom point) with an S inside. This is the Superman S-shield, a registered trademark that DC actively enforces. DC has sued over a shirt that only swapped the "S" for "dad" inside the shield.
- **Any pointed-bottom shield or inverted triangle** around an S. Superman's early (1938–40) shields were inverted triangles. The Voice crest is deliberately rounded at the bottom, and its tail is off to the side.
- **Superman's S shape**: the swooping S with a hooked top terminal and a fish-tail lower serif, letter ends that run into the border, and the specific red S on a yellow field with a red border, especially all together. Our S is geometric and squarish, has parallel slanted cuts, and floats clear of the border with a keyline gap.
- **A lightning bolt in a circle** (the Flash, 1956; also the 1930s fascist "flash and circle" symbol) and a bolt on the chest generally (Shazam / Captain Marvel of Fawcett). Keep any speed streaks off the emblem.
- **An eight-point star** (Captain Marvel), **a horizontal yellow oval with a black silhouette** (the 1960s–80s Batman), **an X in a circle**, and a spider or bat glyph.
- Also avoid **red-on-yellow pentagon + S** colour lockups even without the shape. Our default pop pairing is a yellow letter on a coral field, inside a speech-bubble crest.

This note records design caution, not legal advice. The crest-with-a-speech-tail and the cut-terminal S are original to this project.

## 8. Sources

- Superman logo history and shapes: https://en.wikipedia.org/wiki/Superman_logo ; https://fabrikbrands.com/branding-matters/logofile/superman-logo-history-superman-symbol/ ; https://www.logodesignlove.com/superman-symbol-evolution ; https://comicbook.com/comics/news/every-superman-logo-through-the-decades-ranked/
- Superhero emblem design overviews: https://www.creativebloq.com/design/logos-icons/the-best-superhero-logos-of-all-time ; https://www.creativebloq.com/inspiration/superhero-logos ; https://tvtropes.org/pmwiki/pmwiki.php/Main/ChestInsignia ; https://creativemarket.com/blog/superhero-badges-logo-design
- Flash and circle: https://en.wikipedia.org/wiki/Flash_and_circle ; https://hyperborea.org/flash/logos.html ; Captain Marvel star: https://fabrikbrands.com/captain-marvel-logo-history-captain-marvel-symbol/ ; https://www.looper.com/414643/the-real-reason-captain-marvel-wears-a-star-on-her-chest
- Trademark enforcement: https://www.americanbar.org/groups/litigation/committees/intellectual-property/practice/2015/dc-comics-trademark-police/ ; https://www.worldipreview.com/news/superman-victory-for-dc-comics-7265 ; https://theconversation.com/who-owns-superman-the-man-of-steel-fights-trademark-law-14625 ; https://www.cll.com/newsroom-news-173231
- Costume construction (raised, bevelled and cast emblems): https://cgtrader.com/3d-models/art/signs-logos/batman-batfleck-chest-symbol-emblem ; https://www.kamuicosplay.com/2017/10/09/foamcos/ ; https://www.thepopverse.com/cosplay-texture-foam-eva-armor-plexi ; https://www.dremel.com/us/en/projects/how-to/how-to-make-a-foam-cosplay-helmet ; https://makezine.com/article/craft/working-with-pva-foam-for-costume-construction
- Wrapping on cylinders: https://community.adobe.com/t5/illustrator-discussions/map-a-logo-to-3d-cylinder-in-illustrator-without-getting-mangled/m-p/14928112 ; https://community.adobe.com/t5/illustrator-discussions/best-way-to-apply-logo-to-drink-tumbler/m-p/9329646 ; https://community.adobe.com/t5/illustrator-discussions/distort-to-create-appearance-of-cylinder/m-p/8695195 ; https://support.ptc.com/help/isodraw/r7.4/en/isodraw_top_toc/palette_projection_wrapping_around_cylinder_uref.html ; https://www.proko.com/course-lesson/how-to-draw-cylinders-and-ellipses ; https://www.artyfactory.com/perspective_drawing/perspective_7.html
- Torso foreshortening: https://creativebloq.com/how-to/how-to-draw-a-torso ; https://dev.publishnews.com.br/draw-chest
- Small-size legibility: https://cursa.app/en/page/logo-design-foundations-scalability-and-legibility-checks ; https://wildandfreetools.com/blog/favicon-design-contrast-readability ; https://ibm.com/design/language/iconography/app-icons/usage ; https://dev.to/alphashark/what-makes-a-logo-work-at-16x16-pixels-c4i ; https://www.viget.com/articles/responsive-logos-part-1-tips-for-adapting-logos-for-small-screens

Caveats: several emblem-history sources are fan or branding blogs, and the dates in them are approximate. The cosplay results cover bevelling and raised detail in general rather than emblems specifically. No source gives a formula for comic-artist emblem foreshortening, so the wrap recipe here is derived from cylinder and sphere geometry.
