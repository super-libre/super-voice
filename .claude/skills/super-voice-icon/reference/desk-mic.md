# Desk USB microphone body: drawing spec

> Anatomy research for the desk USB microphone the icon uses. `reference.svg`/`.png` mentioned below is kept as `images/desk-mic.png`. The shipped drawing is `../layers/mic.svg`.


The subject is the microphone body only: mesh half, solid half, seam, front details and pivot points. The yoke and base are left out, but the pivots are marked.
See `reference.svg` / `reference.png` in this folder. The geometry is generic and has no brand marks.

## 1. Real proportions (unit B = body height, top of dome to base of body)

Body dimensions from makers' and retailers' specs. All of these are side-address or desk USB/podcast mics.

| Model | Body H × Ø (mm) | H : W |
|---|---|---|
| RØDE NT-USB | 184 × 62 (depth 50) | 3.0 |
| Razer Seiren X | 184 × 50 | 3.7 (slim cylinder) |
| Fifine K669 | 185 × ~66 (124 wide with yoke knobs) | 2.8 |
| Razer Seiren V2 Pro | 152 × 58 | 2.6 |
| Shure MV7/MV7+ (end-address, yoke) | 169.5 × 66.5; 90 across the knurled yoke knobs | 2.55 |
| Blue Yeti, complete with stand | 295 tall, 120 wide across the yoke knobs, 125 deep | – |

Working numbers:
- **Height to width: 2.5–3.0 for real mics.** Use **about 2.4 for the icon** (width 0.42 B), because a chunkier body reads better small.
- **Shape:** a cylinder with a **fully round (semicircular) top dome**, dome radius = half the width. The bottom is flat or only slightly domed, with small corner radii of about 0.07 B. In front view it is a **pill that is round on top and square-ish at the bottom**, not a symmetric capsule.
- **Mesh portion:** the top **40–50%** of the height; use 0.46 B. Smaller "stick" models (Seiren X, NT-USB Mini) run down to about 35–40%.
- **Seam:** a band about **0.03–0.05 B** tall, usually a dark ring, a chrome ring or a step in diameter. Sometimes it is only a change of colour or material. Use a dark band 0.04 B tall that sits **1–2% proud** of the body width.
- **Solid portion:** the lower **about 0.50 B**.
- **Pivot / mounting points:** on the two sides, **centred about 0.40–0.50 B above the base**. That is the body's vertical centre, usually **just below the seam on the solid part**; use 0.42 B. Knob figures come from the MV7, whose knobs add 90 − 66.5 = 23.5 mm in total, so **each side sticks out about 0.07 B**; knob diameter is about 0.12–0.16 B. With no stand, show a short **stub of 0.04 B by 0.09 B** or a round boss/recess there.
- **Front controls** (on the solid part, centred):
  - mute button with LED just under the seam, at about 0.38 B above the base, diameter about 0.10 B;
  - gain/volume knob lower, at about 0.19 B above the base, diameter about 0.13 B.

  The badge sits on the mesh front (Yeti style) or just under the seam. The headphone jack and USB port are on the **underside or back**, so they are not visible from the front.

### Icon geometry (local frame = Bold Stage frame)

Group transform `translate(248 268) rotate(35) scale(1.18)`. Body centred at the origin, +y towards the base. **B = 300 u**, width 126 u.

```
body outline   M-63 -87A63 63 0 0 1 63 -87V130Q63 150 43 150H-43Q-63 150 -63 130Z
mesh (top)     M-63 -12V-87A63 63 0 0 1 63 -87V-12Z             y -150..-12  (0.46B)
seam band      M-65 -12H65V0H-65Z                                y -12..0     (0.04B)
solid (lower)  M-63 0H63V130Q63 150 43 150H-43Q-63 150 -63 130Z  y 0..150     (0.50B)
pivot stubs    M-63 10H-71Q-75 10 -75 14V34Q-75 38 -71 38H-63Z  (and mirrored)  centre y=24
pivot knobs    M-63 2H-80Q-86 2 -86 8V40Q-86 46 -80 46H-63Z     (optional variant; mirrored)
capsule shadow ellipse cx=0 cy=-70 rx=44 ry=62, clipped to the mesh
badge          rect x=-22 y=-74 w=44 h=22 rx=6   (on the mesh)
mute + LED     circle cy=36 r=15 (LED dot r=7)
gain knob      circle cy=92 r=19, pointer line
highlight      M-40 6V140, stroke 10, white 45%
```
In this frame the old Bold Stage mic spanned -202..184; the body spans -150..150. That leaves room for the cape at the base end, and the cape can attach at the pivots (y ≈ 24, x ≈ ±63–86). That is a natural "shoulder".

Three-quarter view: draw the seam, the base and the top of the solid part as ellipse arcs with ry ≈ 14 (0.047 B). Shift the controls about 26 u towards the turned side and squash them 0.8× horizontally. Draw one pivot stub as a trapezoid on the turned side; the other hides behind the body (see the reference).

## 2. Parts, ordered by importance for recognition

1. **Two-tone split:** a light or patterned mesh top over a solid, usually darker or smooth, bottom, split near the middle. It is the single strongest cue. It separates this mic from a stick mic (ball on a handle) and from a walkie-talkie or torch.
2. **Round dome top with a flat base:** an asymmetric pill. A symmetric capsule reads as a pill or a battery.
3. **Seam band:** the dark ring between the two halves. It is the "join" that makes it an assembly.
4. **Side pivot points** at the body's middle, just under the seam. Two little knobs or stubs read as "this mounts in a yoke", even with no yoke drawn. **Use the knob variant if the icon must read as a desk mic at 32 px.**
5. **Mesh texture:** perforated metal (round holes in a hex or offset grid) or woven wire. The capsule shows through as a darker oval in the upper middle of the mesh.
6. Front mute button with a coloured LED (red or green): one round coloured dot under the seam.
7. Gain/volume knob: a round knob with a pointer line, lower on the solid part.
8. Badge: a small plate on the mesh front (on this icon it can carry the "S").

### Silhouette variants (pick one)
- **A. Tall pill on a yoke** (Yeti, K669, AKG/HyperX-style, NT-USB): dome top, about 0.45 mesh, H:W about 2.5–3. **Recommended.**
- **B. Squat cylinder** (Yeti Nano, JLab Talk, NT-USB Mini): H:W about 1.7–2.2, with a larger mesh share and fatter knobs. It reads as cute and toy-like.
- **C. Slim stick** (Seiren X, Seiren V2): H:W about 3.5. Mesh is the top 35–40%, and it often sits in a shock mount rather than a yoke.
- **D. Rectangular / ribbon style** (ribbon and side-address condensers: MXL, Golden Age R1, Sony C-37A, AT3525 on yokes): a rounded rectangle or lozenge with **horizontal slots** or a mesh window on the front face, and the yoke pivots at the exact middle. It is retro and distinctive, but drifts towards a radio.

## 3. Simplification by render size

Scale: 1 local u = 1.18 px at 512, 0.148 px at 64, 0.074 px at 32. The body is about 354 px long at 512, about 44 px at 64 and about 22 px at 32.

- **512:**
  - Draw everything: the mesh pattern (holes about 9 u apart, about 14 across), the capsule shadow, the seam band, the stubs or knobs, the LED, the knob with its pointer, the badge and a long highlight on the solid part.
  - Ink style: hatch the solid part along its length, and stipple or crosshatch the mesh.
  - Glass style: frosted mesh with a darker core, and the solid part clear.
- **64:**
  - Show the mesh as a flat light tone, or 3–4 rows of dots at most, so fine perforation doesn't turn to moiré. Keep the seam band (12 u ≈ 1.8 px; thicken it to 18 u).
  - Keep the LED as one coloured dot and drop the knob, or keep only one of the two controls.
  - Make the pivot stubs about 3 px.
  - Drop the badge text; the badge becomes a light chip.
- **32:**
  - The body is about 22 × 9 px. Keep only the silhouette, the two tones (mesh light, solid dark) and the dark seam.
  - Use the **knob variant**: knobs of 1.5–2 px that stick out sideways are what separate it from a phone, battery or walkie-talkie.
  - Show the LED only as a 2 px coloured dot if contrast allows.
  - Drop the badge, the mesh pattern and the highlight details.

## 4. Drawing rules

1. **Dome top, flat bottom.** The top is a full semicircle of radius W/2, and the bottom has only small corner radii of about 0.07 B.
2. **Height about 2.4–3× width.** Not stubby, unless you deliberately choose variant B.
3. **Split near the middle:** mesh 0.40–0.50 B on top, solid about 0.50 B below, with a contrasting seam band of 0.04 B between them.
4. **The mesh is lighter and textured; the solid part is smooth and darker.** Never draw both the same value.
5. **Pivots sit at the vertical middle, on the sides, just under the seam.** Never put them at the base or on the dome. They are stubby round bosses, not handles.
6. **Controls are centred on the front of the solid part.** The button with its LED goes right under the seam and the knob below it. Ports are not visible from the front.
7. **Show depth on the mesh.** Use a darker capsule oval in the upper middle, so it reads as a hollow grille and not a dotted sticker.
8. **In the 35° flight pose**, the dome leads up and to the right. Attach the cape at the pivots or the seam, and keep the controls facing the viewer.

## 5. Sources

- RØDE NT-USB datasheet (184 × 62 × 50 mm): https://edge.rode.com/pdf/products/95/NT-USB_Datasheet_FA.pdf ; NT-USB Mini (141.9 tall, 54.5 wide): https://edge.rode.com/pdf/products/96/NT-USB_Mini_ds_V06.pdf
- Razer Seiren X (50 mm Ø × 184 mm, base 90 mm): https://mysupport.razer.com/app/answers/detail/a_id/2625 ; Seiren V2 Pro (152 × 58.2 mm without stand, 203 mm with it): https://www.tomshardware.com/reviews/razer-seiren-v2-pro
- Shure MV7 spec sheet (169.5 mm long, 90 mm across the yoke with its knurled knobs) and B&H (body Ø 66.5): https://www.fullcompass.com/common/files/89157-MV7Specsheet.pdf ; https://www.bhphotovideo.com/c/product/1808586-REG/shure_mv7_k_podcast_microphone.html
- Blue/Logitech Yeti (12 × 12.5 × 29.5 cm in its stand; detachable base, 5/8" thread): https://www.logitechg.com/en-us/products/streaming-gear/yeti.html ; https://www.bhphotovideo.com/c/product/1103930-REG/blue_836213002070_yeti_usb_microphone_black.html
- Yeti front layout (mute button with LED below the logo, front volume knob, rear gain knob, headphone jack on the underside): https://www.manualslib.com/manual/837882/Blue-Yeti.html?page=5 ; https://www.popsci.com/?p=371582 ; https://ats.emory.edu/_includes/documents/studios/manuals/Yeti%20Manual.pdf
- Yeti X (metal chassis, steel grille, combined smart knob): https://www.ifixit.com/Device/Logitech_YETI_X ; https://lanoc.org/review/audio/blue-yeti-x?start=1
- Yeti Nano (21.1 × 9.6 × 10.9 cm with stand): https://www.logitechg.com/en-us/products/streaming-gear/yeti-nano-usb-microphone.html
- Fifine K669 (12.4 × 6.6 × 18.5 cm): https://www.streamtechreviews.com/blog/k669bvst669vsk670 ; https://www.desertcart.com.ar/products/36133412-fifine-usb-podcast-condenser-microphone-k669
- JLab Talk (about 13.5 × 7.0 × 5.3 cm): https://help.jlab.com/pages/talk-tech-specs
- HyperX QuadCast S (25 × 10.3 × 12.9 cm with its stand; mic 255 g): https://hp.com/us-en/shop/pdp/hyperx-quadcast-s-usb-microphone-%28white-grey%29-rgb-lighting
- Side-address ribbons and condensers on yokes (MXL UR-1, Golden Age R1, Sony C-37A, AT3525, Shure PG27-USB): https://recordinghacks.com/microphones/tag/yoke-mount
- Caveat: no maker publishes the mesh/solid split or the knob size directly. The 0.46 B mesh share, the 0.04 B seam and the pivot height of 0.42 B come from product photos of Yeti, K669, NT-USB and QuadCast-class mics, together with the MV7 knob figures. Treat them as ±10%.
