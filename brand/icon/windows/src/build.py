#!/usr/bin/env python3
"""Build every Windows icon asset for Super Voice from the SVG sources.

    python3 brand/icon/windows/src/build.py

Needs Node with Playwright's Chromium (rendering) and Pillow (downsampling,
ICO packing). Everything is rendered at 4x and downsampled, so edges and the
master's halftone stay smooth at small sizes.

Outputs (all under brand/icon/windows/):
    super-voice.ico            app icon, 14 sizes (BMP < 256, PNG at 256)
    png/super-voice-<N>.png    each ICO frame as a PNG
    msix/Assets/*.png          MSIX / AppX logo set with scale and targetsize variants
    tray/*.ico, tray/*.png     one-colour notification-area icons, light and dark taskbar
    preview.png                contact sheet with mock taskbars and Start pins
"""
import base64
import io
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import glyphs  # noqa: E402

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.dirname(HERE)
MASTER = os.path.normpath(os.path.join(OUT, "..", "master", "super-voice-icon.svg"))
SS = 4  # supersampling factor

# Every size Windows 11 may ask for (app icon construction guidance).
ICO_SIZES = [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256]
TRAY_SIZES = [16, 20, 24, 32]
# The simplified glyph is used up to and including this size.
SMALL_MAX = 32
SCALES = [100, 125, 150, 200, 400]

# Tray glyph colours (Windows 11 taskbar text colours).
TRAY_ON_DARK = "#ffffff"
TRAY_ON_LIGHT = "#1c1c1c"


def data_uri(svg_text):
    return "data:image/svg+xml;base64," + base64.b64encode(svg_text.encode()).decode()


MASTER_URI = data_uri(open(MASTER).read())


def px(base, sc):
    """Base size at a scale factor, rounded half up as Microsoft's tables do (50 -> 63 at 125%)."""
    return int(base * sc / 100 + 0.5)


def pad_for(n):
    """Transparent padding around the tile at an n-pixel icon."""
    return max(1, round(n / 24))


def icon_html(w, h, tile, x, y, small=None):
    """A page w x h with the rounded-square icon (tile px) at (x, y).

    small: the n of a 16-32 px frame, which uses the simplified glyph.
    """
    src = data_uri(glyphs.small_svg(small, tile, x)) if small else MASTER_URI
    flt = ""
    if not small:
        # Soft key shadow, like Windows 11's own icons: down, short, low opacity.
        flt = f"filter:drop-shadow(0 {tile*0.012:.2f}px {tile*0.016:.2f}px rgba(0,0,0,.30));"
    return (
        f'<html><body style="margin:0;background:transparent">'
        f'<img src="{src}" style="position:absolute;left:{x}px;top:{y}px;'
        f'width:{tile}px;height:{tile}px;{flt}"></body></html>'
    )


def app_icon_job(n, out):
    p = pad_for(n)
    return dict(out=out, w=n, h=n, scale=SS, html=icon_html(n, n, n - 2 * p, p, p, small=n if n <= SMALL_MAX else None))


def tile_job(w, h, frac, out, lift=0.0):
    """Icon centred on a transparent tile/splash canvas, frac of the short side."""
    t = round(min(w, h) * frac)
    x = (w - t) / 2
    y = (h - t) / 2 - h * lift
    return dict(out=out, w=w, h=h, scale=SS, html=icon_html(w, h, t, x, y))


def tray_job(n, color, out):
    svg = glyphs.tray_svg(n, color)
    html = (
        f'<html><body style="margin:0;background:transparent">'
        f'<img src="{data_uri(svg)}" style="position:absolute;left:0;top:0;width:{n}px;height:{n}px">'
        f"</body></html>"
    )
    return dict(out=out, w=n, h=n, scale=SS, html=html)


def run_render(jobs):
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
        json.dump(jobs, f)
        path = f.name
    env = dict(os.environ)
    try:
        root = subprocess.check_output(["npm", "root", "-g"], text=True).strip()
        env["NODE_PATH"] = root + os.pathsep + env.get("NODE_PATH", "")
    except Exception:
        pass
    subprocess.check_call(["node", os.path.join(HERE, "render.js"), path], env=env)
    os.unlink(path)


def downsample(raw, final, w, h):
    im = Image.open(raw).convert("RGBA")
    if im.size != (w, h):
        # Resample in premultiplied alpha so transparent edges do not go dark.
        im = im.convert("RGBa").resize((w, h), Image.LANCZOS).convert("RGBA")
    os.makedirs(os.path.dirname(final), exist_ok=True)
    im.save(final, optimize=True)
    return im


# ---------- ICO writer: 32-bit BMP (DIB) entries below 256, PNG at 256 ----------

def bmp_entry(im):
    w, h = im.size
    px = im.tobytes("raw", "BGRA")
    rows = [px[y * w * 4:(y + 1) * w * 4] for y in range(h)]
    xor = b"".join(reversed(rows))  # bottom-up
    mask_stride = ((w + 31) // 32) * 4
    alpha = im.getchannel("A").tobytes()
    and_rows = []
    for y in range(h - 1, -1, -1):
        bits = bytearray(mask_stride)
        for x in range(w):
            if alpha[y * w + x] == 0:
                bits[x // 8] |= 0x80 >> (x % 8)
        and_rows.append(bytes(bits))
    header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, len(xor) + len(and_rows) * mask_stride, 0, 0, 0, 0)
    return header + xor + b"".join(and_rows)


def png_entry(im):
    b = io.BytesIO()
    im.save(b, format="PNG", optimize=True)
    return b.getvalue()


def write_ico(path, images, png_min=256, first=None):
    """images: {size: RGBA image}. Entries >= png_min are PNG, the rest BMP."""
    order = sorted(images)
    if first in images:
        order.remove(first)
        order.insert(0, first)
    blobs = [(s, png_entry(images[s]) if s >= png_min else bmp_entry(images[s])) for s in order]
    out = struct.pack("<HHH", 0, 1, len(blobs))
    offset = 6 + 16 * len(blobs)
    dirs, data = b"", b""
    for s, blob in blobs:
        dim = 0 if s >= 256 else s
        dirs += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset + len(data))
        data += blob
    with open(path, "wb") as f:
        f.write(out + dirs + data)


def tune_tray():
    """Print the sub-pixel shift per tray size with the fewest half-covered pixels."""
    tmp = tempfile.mkdtemp(prefix="sv-tune-")
    jobs, meta = [], []
    for n in TRAY_SIZES:
        for i in range(8):
            for j in range(8):
                job = tray_job(n, "#000000", os.path.join(tmp, f"{n}-{i}-{j}.png"))
                job["html"] = job["html"].replace(
                    data_uri(glyphs.tray_svg(n, "#000000")), data_uri(glyphs.tray_svg(n, "#000000", (i / 8, j / 8)))
                )
                jobs.append(job)
                meta.append((n, (i / 8, j / 8), job["out"]))
    run_render(jobs)
    best = {}
    for n, shift, path in meta:
        a = Image.open(path).convert("RGBa").resize((n, n), Image.LANCZOS).convert("RGBA").getchannel("A")
        mid = sum(1 for v in a.tobytes() if 40 < v < 215)
        if n not in best or mid < best[n][0]:
            best[n] = (mid, shift)
    shutil.rmtree(tmp)
    print("TRAY_SHIFT =", {n: s for n, (_, s) in sorted(best.items())})


def main():
    tmp = tempfile.mkdtemp(prefix="sv-win-")
    jobs, finals = [], []  # finals: (raw, final, w, h)

    def add(job, final):
        raw = os.path.join(tmp, f"{len(jobs):04d}.png")
        job["out"] = raw
        jobs.append(job)
        finals.append((raw, final, job["w"], job["h"]))

    # App icon frames
    for n in ICO_SIZES:
        add(app_icon_job(n, ""), os.path.join(OUT, "png", f"super-voice-{n}.png"))

    # MSIX: Square44x44Logo (the app list icon)
    A = os.path.join(OUT, "msix", "Assets")
    for n in ICO_SIZES:
        for alt in ("", "_altform-unplated", "_altform-lightunplated"):
            add(app_icon_job(n, ""), os.path.join(A, f"Square44x44Logo.targetsize-{n}{alt}.png"))
    for sc in SCALES:
        add(app_icon_job(px(44, sc), ""), os.path.join(A, f"Square44x44Logo.scale-{sc}.png"))
        add(app_icon_job(px(50, sc), ""), os.path.join(A, f"StoreLogo.scale-{sc}.png"))
        s71 = px(71, sc)
        add(tile_job(s71, s71, 0.62, ""), os.path.join(A, f"SmallTile.scale-{sc}.png"))
        s150 = px(150, sc)
        add(tile_job(s150, s150, 0.48, "", lift=0.05), os.path.join(A, f"Square150x150Logo.scale-{sc}.png"))
        ww, wh = px(310, sc), px(150, sc)
        add(tile_job(ww, wh, 0.48, "", lift=0.05), os.path.join(A, f"Wide310x150Logo.scale-{sc}.png"))
        s310 = px(310, sc)
        add(tile_job(s310, s310, 0.40, "", lift=0.04), os.path.join(A, f"LargeTile.scale-{sc}.png"))
        sw, sh = px(620, sc), px(300, sc)
        add(tile_job(sw, sh, 0.50, ""), os.path.join(A, f"SplashScreen.scale-{sc}.png"))

    # The master at small sizes, only for the comparison row in preview.png
    cmp_dir = os.path.join(tmp, "compare")
    for n in (16, 24, 32):
        p = pad_for(n)
        add(dict(w=n, h=n, scale=SS, html=icon_html(n, n, n - 2 * p, p, p)), os.path.join(cmp_dir, f"master-{n}.png"))

    # Tray
    T = os.path.join(OUT, "tray")
    for n in TRAY_SIZES:
        add(tray_job(n, TRAY_ON_DARK, ""), os.path.join(T, f"tray-dark-taskbar-{n}.png"))
        add(tray_job(n, TRAY_ON_LIGHT, ""), os.path.join(T, f"tray-light-taskbar-{n}.png"))

    # Splash pages are large; render them at 2x instead of 4x.
    for j in jobs:
        if j["w"] * j["h"] > 600 * 600:
            j["scale"] = 2 if j["w"] * j["h"] < 1500 * 700 else 1

    # Reference copies of the generated glyphs at 32 px, for viewing.
    with open(os.path.join(HERE, "small.svg"), "w") as f:
        f.write(glyphs.small_svg(32, 32 - 2 * pad_for(32), pad_for(32)))
    with open(os.path.join(HERE, "tray.svg"), "w") as f:
        f.write(glyphs.tray_svg(32, "#000000"))

    if os.path.isdir(os.path.join(OUT, "msix")):
        shutil.rmtree(os.path.join(OUT, "msix"))
    run_render(jobs)
    imgs = {}
    for raw, final, w, h in finals:
        imgs[final] = downsample(raw, final, w, h)

    write_ico(
        os.path.join(OUT, "super-voice.ico"),
        {n: imgs[os.path.join(OUT, "png", f"super-voice-{n}.png")] for n in ICO_SIZES},
        first=32,
    )
    for theme in ("dark", "light"):
        write_ico(
            os.path.join(T, f"super-voice-tray-{theme}-taskbar.ico"),
            {n: imgs[os.path.join(T, f"tray-{theme}-taskbar-{n}.png")] for n in TRAY_SIZES},
        )
    print(f"rendered {len(jobs)} images")

    # Contact sheet
    subprocess.check_call([sys.executable, os.path.join(HERE, "preview.py"), cmp_dir])
    shutil.rmtree(tmp)


if __name__ == "__main__":
    if "--tune-tray" in sys.argv:
        tune_tray()
    else:
        main()
