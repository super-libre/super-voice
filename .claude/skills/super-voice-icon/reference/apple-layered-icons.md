# Glass background layers for Coral Whip: research and layer plan

> Research on how Apple platforms treat layered icons (Icon Composer, Liquid Glass, what moves and what does not).


Labels used below: **[Apple]** means confirmed in Apple documentation or a WWDC transcript. **[Reported]** means third-party reporting. **[Inferred]** means my reading, not stated anywhere.

## 1. How macOS handles icon animation

**Short answer.** In macOS 26 the layers of a Liquid Glass app icon do **not** move or show parallax against each other. They don't react to hover, click, pointer movement or window movement, in the Dock or anywhere else I could find. Apple renders the layers into one static-looking glass object with highlights, translucency, refraction and shadows. The only "live" lighting Apple describes is on the **iOS Home Screen** (gyro-driven edge light), and reporting says iOS 27 removed even that. The Dock's own motion is unchanged: icons bounce when an app opens and magnify under the pointer. Both are user settings and are the same for every icon.

### 1.1 macOS 26 (Tahoe)
- **[Apple]** Icons on iOS, iPadOS, macOS and watchOS have "a background layer and one or more foreground layers that coalesce to create dimensionality. These icons take on Liquid Glass attributes like specular highlights, refraction, and translucency." The HIG's only motion language for these four platforms is "the system applies visual effects that respond to the environment and people's interactions". It names no pointer, hover or parallax behaviour for macOS. (HIG App icons)
- **[Apple]** WWDC25 "Say hello to the new look of app icons": the only motion statement is iOS-specific. "the material comes alive on your iOS home screen. Based on gyro input, you can see light moving on the edge of the icon." For macOS it only says existing icons "receive the new material appearance". Nothing about layers moving.
- **[Apple]** The Newsroom (June 2025) says Liquid Glass in general "uses real-time rendering and dynamically reacts to movement with specular highlights". That is the material in general (controls and UI), not a claim about Mac Dock icons.
- **[Apple]** WWDC26 session 8012 describes 2026 rendering changes: a "sharper", "crisper" specular with dark parts, refraction picked up from system glass, reduced translucency and ring shadows. It says nothing about icons responding to pointer, hover or motion on any platform.
- **[Apple]** Dock animations come from the user's Desktop & Dock settings: "Animate opening applications" ("Make icons bounce when you open apps") and Magnification ("Magnify icons when you move the pointer over them"). They are system animations of the whole icon, not of its layers.
- **[Inferred]** Finder, Launchpad/Apps, Spotlight and the app switcher draw the same rendered icon. I found no source describing layer motion in any of them. I didn't find hands-on reports that the Mac Dock's specular shifts with the pointer, and Apple doesn't claim it.

### 1.2 Other platforms
- **iOS / iPadOS [Apple]:** gyro-driven light moves along the icon edge on the Home Screen (WWDC25 session 220). **[Reported]** MacRumors (June 2026) says iOS 27 beta 1 removed the gyroscopic specular effect. Edge highlights now sit fixed at the top and bottom, and icons gained more distinct glass layers. I found no Apple source for a press or long-press effect specific to icon layers.
- **visionOS [Apple]:** an icon is a background plus one or two layers, "producing a three-dimensional object that subtly expands when people view it", with system shadows between layers and an embossed alpha. WWDC25 session 303 describes the gaze "ramp" hover, where the icon starts scaling on look. This is real layer depth and motion. (HIG; WWDC23 10076; WWDC25 303)
- **tvOS [Apple]:** "between two and five layers". On focus the icon "elevates to the foreground in response to someone's finger movement on their remote, and gently sways while the surface illuminates… The separation between layers and the use of transparency produce a feeling of depth during the parallax effect." This is the only platform with true layer parallax. Its layers are built in an Xcode image stack, not in Icon Composer. (HIG)

### 1.3 What a developer controls
- **Icon Composer [Apple]:**
  - **Layers** get Liquid Glass automatically, and the Effects toggle can turn it off. Each layer also has Color settings (Fill: Automatic, None, Solid or Gradient; Opacity; Blend Mode) and Composition settings (position, scale, visibility).
  - **Groups** have Liquid Glass Mode (Individual or Combined) plus Specular (Off, Automatic, Inside or Outside), Blur, Refraction, Translucency and Shadow (neutral or chromatic, per WWDC25 session 361).
  - Settings can vary per appearance (default, dark and mono, with clear and tinted made from mono) and per platform.
  - **None of these controls motion.** Before OS 27, Inside or Outside specular simply shows, and Refraction "has no visible effect".
  - (Creating your app icon using Icon Composer; WWDC25 session 361)
- **Group limit [Apple]:** "organize the layers… into a maximum of four groups to reduce complexity. The groups become the layers in the app icon image the platform renders." WWDC25 session 361 adds: "By default, it'll always be one, but you can go all the way up to four."
- **Artwork rules [Apple]:**
  - Remove blurs, shadows, specular, opacity, translucency and background gradients from the layer art; the system draws those.
  - Avoid feathered edges and prefer crisp, flat shapes.
  - Layers are square and unmasked: "Providing layers with pre-defined masking negatively impacts specular highlight effects and makes edges look jagged."
  - Use SVG, with text converted to outlines.
  - Icon Composer supports solid and gradient backgrounds natively.
- **Runtime Dock APIs [Apple]:**
  - `NSDockTile` lets an app badge the tile (`badgeLabel`), show or hide the app badge, or draw the tile itself with a custom `contentView` and `display()`. An app can therefore animate its Dock tile by redrawing the content view.
  - The default tile shows `NSApplication.applicationIconImage`, which an app can set at runtime.
  - `requestUserAttention(_:)` starts an attention request. That this request bounces the icon is common knowledge (**[Inferred]**); the doc page I read doesn't state it.
  - A custom view or image replaces the system-rendered Liquid Glass icon while it's shown. **[Inferred]**

### 1.4 WWDC25 and WWDC26 on dynamic lighting
- **WWDC25 [Apple]:** the icons are "lit from within", and the iOS Home Screen has gyro-driven edge light. "softer light-to-dark gradients harmonize best with the direction of light"; rounder corners let "light… seamlessly travel on the edges".
- **WWDC26 [Apple]:** the Design guide says Icon Composer can "preview with dynamic lighting effects". Session 8012 covers the sharper specular, refraction, reduced translucency and ring shadows. Opening last year's file applies the new rendering "no artwork changes or recompile required". Clear-mode glass deliberately "does not respond to the [transparency] slider".

**What this means for the user's idea.** Two glass background layers will look great on macOS as a *still* layered object: two sheets of glass with highlights and shadow between them. They will not slide when the icon is clicked. Real layer motion exists only on tvOS (focus parallax) and visionOS (gaze expand). On iOS, light only moved along the icon edges, and reportedly not in iOS 27.

## 2. The glass background layers
In each `split-N/` folder:
- `bg-base.svg`: the orange tile ground (#f68a2c → #ee6c24).
- `bg-glass-a.svg`: the middle orange-coral glass sheet.
- `bg-glass-b.svg`: the deep coral-crimson sheet.
- `light.svg`: an optional top-left light.

Each sheet has a translucent diagonal tint, a soft inner glow on its lit edge, a thin bright rim and a faint crimson drop shadow, so depth shows between them.

The splits:
- **split-1, Two planes:** two big rounded squares tilted -10°, stacked toward the lower right. It is the calmest and reads most like the original gradient.
- **split-2, Diagonal sweep:** two wide rounded bands at 45°, running parallel to the mic's climb.
- **split-3, Two discs:** two large circles rising from the lower-right corner. It is the softest, and the most like Game Center.

These preview files are clipped to rx 112 and carry baked tints, rims and shadows **for preview only**. For Icon Composer:
- Export the same shapes unclipped and flat: solid fill, no rim, no glow, no shadow.
- Set the colours, opacity and Liquid Glass (specular, translucency, shadow) in Icon Composer.
- Use Icon Composer's own gradient background instead of `bg-base.svg`.
- Drop `light.svg`, since the system supplies lighting.

## 3. Recommended Icon Composer structure (4 groups, back to front)
1. **Background** (not a group): Icon Composer gradient fill, #f27a22 → #d8344e, or solid #f68a2c if glass B carries the coral.
2. **Group 1, "Glass A"**: layer `01-glass-a`. Liquid Glass on, Translucency medium, Specular Automatic, neutral Shadow.
3. **Group 2, "Glass B"**: layer `02-glass-b`. Liquid Glass on, Translucency a little higher, so A shows through where they overlap.
4. **Group 3, "Hero"**: layers `03-shadow` (Liquid Glass off, or drop it and use the group Shadow), `04-cape` and `05-mic`. Mode Combined, so the mic and cape read as one object with one specular rim.
5. **Group 4, "Badge"**: layer `06-badge` (the S). Liquid Glass on with Specular Inside, so the crest gets its own crisp highlight.

Alternative if the ink linework looks muddy under glass: turn Liquid Glass off on the ink layers. Apple notes the system doesn't apply glass to "text and thin lines" in auto-segmented icons, and lets you opt layers out. In that case only the two background sheets and the badge are glass.

## 4. Sources
- HIG, App icons: https://developer.apple.com/design/human-interface-guidelines/app-icons
- Creating your app icon using Icon Composer: https://developer.apple.com/documentation/xcode/creating-your-app-icon-using-icon-composer
- Icon Composer: https://developer.apple.com/icon-composer/
- WWDC25 220, Say hello to the new look of app icons: https://developer.apple.com/videos/play/wwdc2025/220/
- WWDC25 361, Create icons with Icon Composer: https://developer.apple.com/videos/play/wwdc2025/361/
- WWDC26 Design guide: https://developer.apple.com/wwdc26/guides/design/
- WWDC26 8012 (icon rendering updates): https://developer.apple.com/videos/play/wwdc2026/8012/
- Apple Newsroom, new software design (June 2025): https://www.apple.com/newsroom/2025/06/apple-introduces-a-delightful-and-elegant-new-software-design/
- Apple Support, Desktop & Dock settings: https://support.apple.com/guide/mac-help/change-desktop-dock-settings-mchlp1119/mac
- NSDockTile: https://developer.apple.com/documentation/appkit/nsdocktile
- requestUserAttention(_:): https://developer.apple.com/documentation/appkit/nsapplication/requestuserattention(_:)
- WWDC25 303, Design hover interactions for visionOS: https://developer.apple.com/videos/play/wwdc2025/303/
- WWDC23 10076, Design for spatial user interfaces: https://developer.apple.com/videos/play/wwdc2023/10076/
- [Reported] MacRumors, iOS 27 revamps app icons: https://macrumors.com/2026/06/16/ios-27-revamps-app-icons
