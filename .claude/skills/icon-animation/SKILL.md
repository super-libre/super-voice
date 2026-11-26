---
name: icon-animation
description: How to animate the layered Super Voice app icon (the caped desk microphone flying through glass bands). Use whenever the icon should move anywhere, such as the macOS Dock tile while the app listens or speaks, a splash or onboarding screen, the settings app, the COSMIC applet, the website or README, or a video, or when adding a new animated state, exporting the icon, or changing its layers.
---

# Super Voice icon animation

The icon is a stack of eight layers on a 512 × 512 rounded square (corner radius 112). It reads as a microphone in a cape flying up and to the right through glass. Everything needed to draw and animate it lives in this folder:

| File | What it is |
|---|---|
| `animation.json` | The single source of truth: every layer, its order, canvas and motion. Read it, don't copy numbers out of this page. |
| `layers/` | The layer SVGs. `cape/` and `shadow/` hold 24 frames each (`cape-00.svg` … `cape-23.svg`). |
| `player.html` | A reference player that plays `animation.json` exactly. Use it to check any port against. |
| `preview.png` | The still icon (the rest frame) at 512 px. |

## The layers, back to front

| # | Layer | Motion | Why |
|---|---|---|---|
| 0 | `ground` | none | The orange tile. It never moves, so the icon always has a stable edge. |
| 1 | `band-far` | scrolls, slow | Deeper coral glass band, further away. |
| 2 | `band-near` | scrolls, faster | Wide glass band, closer. |
| 3 | `light` | none | Soft top-left light over the glass. |
| 4 | `shadow` | 24 frames | The hard ink shadow of the cape and mic, locked to the cape frames. |
| 5 | `cape` | 24 frames | The olive cape, billowing. |
| 6 | `mic` | none | The desk USB microphone. |
| 7 | `emblem` | none | The pop roundel S on the mic body. |

The flight heading is 45° toward the upper right. All ink layers are already drawn at that angle, so place them at (0, 0) at full size with no extra transform.

## The motion rules

These rules are what make it look right. Keep them when you add states or port the animation.

1. **The background is still; things above it move more the closer they are to the mic.** Depth comes from relative speed: `band-near` moves faster than `band-far`, and the ground does not move at all.
2. **Things stream past opposite the flight.** The bands scroll along `(-0.7071, 0.7071)`, toward the lower left, straight behind the mic. Never scroll along the length of a band: that looks like a barber pole, not flight.
3. **Scrolling is seamless and linear.** Each band layer is one band repeated every `period` units along the scroll direction, drawn on a large `-512 -512 1536 1536` canvas so no edge ever enters the tile. Translate it by exactly one period over `duration_ms`, linearly, forever. Eased scrolling stutters at the loop point.
4. **The two bands use different periods (340 and 400)**, so they never settle into one visible repeat.
5. **The cape is the fastest thing on screen.** It cycles 24 frames at 20 fps (one billow every 1.2 s). The bands take 4–5 s per period. If you change speeds, keep the cape fastest, then the near band, then the far band.
6. **The shadow frames are locked to the cape frames.** Drive both from one clock and always show the same frame index.
7. **The mic and emblem never move relative to each other.** The S is wrapped onto the housing and turns with the body. If the mic moves, move all four ink layers (shadow, cape, mic, emblem) as one group.
8. **The rest frame is the icon.** Frame 0 of every layer with no scroll offset is the still app icon. Animations start from it and can stop on it.
9. **Respect reduced motion.** When the system asks for reduced motion, show the rest frame and do not scroll or cycle frames.

## How to play it

Scale everything by `size / 512`. For each layer, place an image at its `viewBox` origin and size, then clip the whole stack to the rounded square.

- **`static`:** draw it once.
- **`scroll`:** translate by `direction × period` (in icon units, times the scale) over `duration_ms`, linear, repeating forever.
- **`frames`:** show frame `floor(elapsed_s × fps) mod frame_count`. Preload or pre-render every frame so swaps never flicker.

`player.html` is a complete, short example of exactly this, written with the Web Animations API. To try it, serve this folder (`python3 -m http.server -d .claude/skills/icon-animation 8000`) and open `player.html`. Add `?size=64` for a small preview or `?still` for the rest frame.

## Where the icon can animate

Be precise with people about what each platform can do.

- **macOS Dock tile.** The system app icon cannot animate. macOS 26 draws the layered icon as one still glass object, and no layer moves on hover, click or launch. The app itself can animate its Dock tile while it runs, through `NSDockTile`: set a custom `contentView` that draws the stack and call `display()` on each tick. Pre-render the 24 cape frames and the bands at the tile size once and composite, instead of rasterising SVG every frame. Good moments: while listening (`voice record`) and while speaking (`voice speak`). Return to the rest frame when idle.
- **SwiftUI (`super-voice-macos/`).** Stack the layers in a `ZStack` clipped to a `RoundedRectangle(cornerRadius: size * 112 / 512, style: .continuous)`. Drive the scroll offsets and the frame index from one `TimelineView(.animation)` date. Check `accessibilityReduceMotion`.
- **Settings app (libcosmic / iced).** Draw the layers with the `svg` widget in a stack and drive offsets and the frame index from a time subscription (`iced::time::every`), redrawing only while the animation is visible.
- **Web, README and docs.** Use `player.html` as is, or export a short loop (see below). GitHub READMEs don't run scripts, so use an exported GIF, APNG or WebP there.
- **Linux app icon (hicolor), installer and tray.** These are static. Use `preview.png` or the rest-frame layers. The tray at 16–22 px needs a separate one-colour symbol, not this artwork.

## Exporting a loop (video, GIF, WebP)

The full loop that returns exactly to the start is the least common multiple of the three cycles: cape 1.2 s, near band 4.0 s, far band 5.2 s, which is 312 s. That's too long, so for a short export:

- Render 4.0 s at 20 fps (80 frames). The cape and the near band both loop perfectly in 4.0 s.
- Accept the far band's small jump at the loop point, or make an export-only copy of `band-far` with `duration_ms` 4000, so all three repeat every 4 s.

Use Playwright with `player.html` and screenshot the `#icon` element at fixed times, or render the layers directly.

## Changing the artwork

- **Keep the stack aligned.** Every layer is drawn in the same 512 frame.
- **Keep the ink group (shadow, cape, mic, emblem) at the 45° flight heading.** It carries a baked `rotate(5 238 290)` that lines the original drawings up with the bands.
- **Keep new band layers periodic.** If you add or redraw a band, it must be identical after a shift of exactly one `period` along the scroll direction. Prove it by rendering shift 0 and shift `period` and comparing pixels.
- **Keep 24 cape frames that loop.** If you redraw the cape, keep 24 frames that step evenly, including from frame 23 back to frame 0, with a matching shadow frame for each.
- **Keep the S original.** The emblem must not imitate any existing superhero emblem: no five-sided diamond shield and no swooping hooked S.
- **Update `animation.json` and `preview.png`** whenever you change timing, order or files, and re-check with `player.html`.
- **For the system macOS icon (Icon Composer),** use the rest frame only and split it into at most four groups: ground, the two bands, the ink group, then the emblem. Supply flat, unclipped layers, because Icon Composer adds the glass, highlights and shadows itself.
