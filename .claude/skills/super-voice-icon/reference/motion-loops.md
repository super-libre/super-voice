# Endless flight: research, diagnosis, and the three new loops

> Research behind the background loop. The shipped loop is the "Diagonal sweep, turned, endless" option described in `../animation.json` (two glass bands scrolling behind the mic); the streak variants studied here were alternatives.


## Diagnosis: why split-2r-loop looks wrong
1. **The stripes run the wrong way.** Speed lines are drawn *parallel* to the direction of travel, behind the moving object. In split-2r-loop the bands are perpendicular to the flight, so they read as a striped surface (a barber pole or a conveyor belt) sliding diagonally, not as air rushing past.
2. **The bands are hard-edged and identical.** Each layer repeats the same full-length band at a fixed spacing. The eye locks onto the regular pattern, and the motion looks mechanical. Real speed lines are short, varied in length, and stretch and fade in and out.
3. **It is too dense and too busy.** Three overlapping layers, with similar shapes at similar opacity, fill the whole tile. That competes with the mic and cape, which already have strong ink detail and a fluttering cape.
4. **It is too fast for big shapes.** At 60–180 units/s, large surfaces moving across a small tile become the main event. A background should support the hero, not overshadow it.

## Research findings
- **Direction.** Motion lines are "the abstract lines that appear behind a moving object or person, parallel to its direction of movement" (Wikipedia, Motion lines). Readers infer direction from the lines' placement, opposite the side the lines sit on. In manga these are *ryūsen* (流線, "flow lines"), and background speed lines also run parallel. Japanese With Anime; Wikipedia.
- **Parallax.** Background layers move slower than the focal layer. Common ratios are about 1/3 and 2/3 of the main speed (Tuts+ parallax tutorial), and two or three layers are enough (Moon Patrol used three). To make it loop, wrap each layer on its own period instead of building one long strip. Tuts+; Wikipedia, Parallax scrolling; JSLegend infinite parallax.
- **Streaks that stretch and fade.** Motion-design tutorials build speed lines from tapered strokes whose length is animated (trim path "end"), with blur or glow. For a loop, the first and last frames must be identical, and each streak must be fully transparent at its own start and end. Motion Design School; PremiumBeat; SVGator rocket tutorial; Adobe Community loop thread.
- **Easing.** Linear easing suits continuous loops such as marquees, spins and drifts. Ease-in-out at a loop's boundary makes a visible pulse at every seam. Inside a single streak's life, fades on opacity give the soft in and out without breaking the steady flow. web.dev, The basics of easing; MDN.
- **Restraint at small sizes.** Keep generous negative space and don't clutter. A constantly looping icon works best with low density or a pause. Apple's HIG says to add motion "purposefully… Don't add motion for the sake of adding motion", and to respect Reduce Motion by cutting "automatic and repetitive animations, including… peripheral motion". GitLab motion guidelines; Lordicon best practices; Apple HIG, Motion and Accessibility; Apple Reduced Motion evaluation criteria.
- **Icons.** As established in `notes.md`, macOS app icons don't animate their layers. This loop is for the web page or app UI, not for the Dock icon. The Dock-icon route would be a custom `NSDockTile` content view.

## The three variants

Every variant uses linear motion toward (-1, +1)/√2, opposite the flight. All of them pass the seamless-loop check: the t = 0 and t = one-cycle frames differ by a mean of 0.007/255. The remaining maximum of about 22 is only anti-aliasing.

| | Technique | Cycle | What moves |
|---|---|---|---|
| **loop-a Speed streaks** | self-contained SMIL SVG (`streaks.svg`) | 2.4 s | 11 glass streaks *parallel* to the flight, cream and lime, with a bright head and a fading tail. Each slides toward the lower left while it stretches (45% → 100% → 70% of its length) and fades (0 → peak → 0). Durations are cycle/1, cycle/2 and cycle/3, with golden-ratio phase offsets so no two streaks pulse together. |
| **loop-b Soft parallax** | static periodic layers, played by the page with WAAPI | 12 s | 2 layers of soft frosted capsules, long axis along the flight. `soft-far.svg` has P = 300 over 12 s (25 u/s, faint). `soft-near.svg` has P = 450 over 6 s (75 u/s, larger, one lime capsule). Each layer is a linear `translate` from [0,0] to `translate_to` with infinite iterations. |
| **loop-c Glass wind** | self-contained SMIL SVG (`wind.svg`) | 4.8 s | 1 slow soft glass layer drifting one period (340 u) per cycle, plus 6 sparse streaks with durations of cycle/2 to cycle/4. |

**Why some variants are SMIL.** Per-streak timing, stretch and fade can't be expressed as a single translate per layer, so loop-a and loop-c are self-contained animated SVGs: the page just shows them, through `<img>` or inline. loop-b is fully described in its `loop.json` for the Web Animations API. Every `loop.json` includes a `reduce_motion` note.

## Recommendation
**loop-c Glass wind.** The streaks carry the "forward" read, because they run parallel to the flight. The single slow glass layer gives Liquid Glass depth without the busyness of three layers. Six streaks stay readable without clutter, and the 4.8 s cycle is calm.

If you want more pace, use loop-a. If you want calm, decorative motion, use loop-b.

## Sources
- Motion lines (Wikipedia): https://en.wikipedia.org/wiki/Motion_lines
- Speedlines: depicting motion in motionless pictures (SIGGRAPH history): https://history.siggraph.org/learning/speedlines-depicting-motion-in-motionless-pictures
- Clip Studio, Speed lines and Focus lines: https://help.clip-studio.com/en-us/manual_en/540_comic/Speed_lines_and_Focus_lines.htm
- Motion lines in anime/manga (Japanese With Anime): https://www.japanesewithanime.com/2020/03/motion-lines.html
- Parallax scrolling (Wikipedia): https://en.wikipedia.org/wiki/Parallax_scrolling
- Tuts+, Parallax scrolling for 2D games: https://code.tutsplus.com/parallax-scrolling-a-simple-effective-way-to-add-depth-to-a-2d-game--cms-21510t
- Infinite parallax in JavaScript: https://jslegenddev.substack.com/p/how-to-implement-infinite-parallax
- Motion Design School, Speed lines in After Effects: https://motiondesign.school/speed-lines-in-after-effects-tutorial/
- PremiumBeat, Anime speed lines in After Effects: https://www.premiumbeat.com/blog/create-anime-speed-lines-after-effects/
- SVGator, Animated rocket with speed lines: https://svgator.com/blog/how-to-create-animated-rocket
- Adobe Community, seamlessly looping line shapes: https://community.adobe.com/t5/after-effects/how-to-seamlessly-loop-a-series-of-line-shapes/m-p/11828001
- LottieFiles speed-line loops: https://lottiefiles.com/free-animations/speed-lines
- web.dev, The basics of easing: https://web.dev/articles/the-basics-of-easing
- Apple HIG, Motion: https://developer.apple.com/design/human-interface-guidelines/motion
- Apple, Reduced Motion evaluation criteria: https://developer.apple.com/help/app-store-connect/manage-app-accessibility/reduced-motion-evaluation-criteria/
- GitLab motion guidelines: https://design.gitlab.com/brand-design/motion
- Lordicon best practices: https://lordicon.com/docs/best-practices
