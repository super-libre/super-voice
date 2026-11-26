# AppIcon.icon: the Icon Composer source

`AppIcon.icon/` is a hand-written Icon Composer document for the macOS 26
(Tahoe) Liquid Glass app icon. Icon Composer can't run on Linux, so nobody has
opened it yet. This README sits beside the package, not inside it, so that
Icon Composer and `actool` never see a file they don't expect.

Apple doesn't publish the `icon.json` schema. The keys here follow the
reverse-engineered reference at
<https://github.com/peterpoliwoda/icon-composer-template>. Treat the file as a
well-prepared starting point and check it on a Mac.

## What's inside

| Group (front to back) | Layers | Settings in `icon.json` |
|---|---|---|
| Emblem | `6-emblem.png`, the pop roundel S | Liquid Glass off on the layer, neutral shadow |
| Ink | `5-mic.png`, `4-cape.png`, `3-shadow.png` (cape frame 0, the rest frame) | lighting combined, Liquid Glass off on the layers, so the inked drawing stays crisp |
| Glass bands | `2-band-near.svg` (opacity 0.78), `1-band-far.svg` (opacity 0.88) | Liquid Glass on, translucency 0.4, chromatic (layer-color) shadow |
| Background (the document fill) | none | `automatic-gradient` from orange `#f27b28`; a dark specialization of ember brown `#3a1c10` |

This is the grouping the icon-animation skill recommends: ground, bands, ink,
then emblem. That makes three groups plus the fill, within Apple's limit of
four groups.

The layers follow Apple's preparation rules:

- All layers are full-bleed squares, 1024 × 1024, on the 512 icon frame scaled ×2. None has a mask or rounded corners, because the system applies the shape.
- The ground is not imported. Icon Composer draws the fill.
- The `light` layer (a soft top-left highlight) is left out, because Liquid Glass adds its own specular highlight.
- The bands' drop-shadow filter and layer opacity are removed. Opacity is set in `icon.json` instead.
- The ink layers and the emblem are lossless PNGs, not SVGs. They use patterns and clip paths that Icon Composer may not import. Apple says to use PNG for SVG features it doesn't support. The vector originals are in `.claude/skills/icon-animation/layers/`.
- `tools/build.py` regenerates everything here. Edits made in Icon Composer are lost if you run it again, so copy them back into `build.py` (`write_icon_composer`) or stop regenerating the package.

## Still to do on a Mac (Xcode 26 or later, or the standalone Icon Composer)

1. **Open it.** Open `AppIcon.icon` in Icon Composer. If it won't open, make a new document, drag the three folders' worth of files from `Assets/` in, and group them as in the table above.
2. **Check the order.** Emblem should be on top and the glass bands at the back. If the order is reversed, the `groups` array is back-to-front in the real format: reverse it in `build.py`.
3. **Check the fill.** Compare the default fill with `variants/AppIcon-default-fullbleed-1024.png`. If the automatic gradient is off, set a two-stop gradient from `#f68a2c` (top left) to `#ee6c24` (bottom right).
4. **Check each appearance:** Default, Dark, and Mono (clear light, clear dark, tinted light, tinted dark). Compare Dark and Tinted with `variants/AppIcon-dark-1024.png` and `variants/AppIcon-tinted-1024.png`. In Mono, the ink group may need its fill set to automatic or white so the mic doesn't go flat grey.
5. **Decide on the comic shadow.** Decide whether to keep the hard comic shadow (`3-shadow.png`). Icon Composer adds its own shadow at group level, so hide the layer if the two together look doubled.
6. **Try Liquid Glass on the ink.** Try `glass: true` on the mic. If the specular edge reads well on the black outline, keep it.
7. **Save and compile.** Save, then compile `Assets.car` (see `NOTES.md`, "Wiring the Tahoe icon in later").
