#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Builds every macOS icon file in brand/icon/macos/ from the master SVG and
the layer SVGs in .claude/skills/icon-animation/layers/.

Runs on Linux: SVG is drawn by headless Chromium (render.mjs, Playwright),
the menu bar glyph is built with shapely,
everything else is Pillow, and the .icns is written by hand (write_icns).

    python3 brand/icon/macos/tools/build.py

See ../NOTES.md for the grid, the sizes and why each file exists.
"""

from __future__ import annotations

import io
import json
import re
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont, ImageOps

HERE = Path(__file__).resolve().parent
OUT = HERE.parent  # brand/icon/macos
REPO = OUT.parents[2]
MASTER = REPO / "brand/icon/master/super-voice-icon.svg"
LAYERS = REPO / ".claude/skills/icon-animation/layers"
RENDER = HERE / "render.mjs"

# Apple's macOS app icon grid (Big Sur and later, still what a legacy .icns
# has to match on macOS 26): a 1024 canvas, an 824 tile centred on it (a 100
# margin all round), continuous ("squircle") corners of radius 185.4.
CANVAS = 1024
TILE = 824
MARGIN = (CANVAS - TILE) // 2
RADIUS = 185.4
SS = 4  # supersampling factor for the master raster and the mask

# The standard .iconset names and the .icns types they become.
ICONSET = [  # (iconset name, pixels)
    ("icon_16x16.png", 16), ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32), ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128), ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256), ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512), ("icon_512x512@2x.png", 1024),
]


# --------------------------------------------------------------------------
# Rendering


def render(jobs: list[tuple[Path, Path, int]]) -> None:
    """Draws SVG files to PNG with Chromium: [(svg, png, px), ...]."""
    args = []
    for svg, png, px in jobs:
        args += [str(svg), str(png), str(px)]
    subprocess.run(["node", str(RENDER), *args], check=True)


def unclipped(svg: str) -> str:
    """The artwork full-bleed: every rx-112 tile clip becomes a plain square,
    so the art runs to the corners and only our own mask shapes it."""
    return svg.replace('rx="112"', 'rx="0"')


def recolor(svg: str, mapping: dict[str, str]) -> str:
    for old, new in mapping.items():
        svg = svg.replace(f'stop-color="{old}"', f'stop-color="{new}"')
    return svg


# Dark appearance. Apple: base it on the light icon, keep the palette but make
# it more subdued, avoid excessive brightness, a colour background gives the
# most contrast. So: the orange ground becomes a deep ember brown, the glass
# bands keep their orange-to-coral hue but darker, the top-left light is
# dimmer, and the ink group is unchanged (its cream, olive and gold read well
# on the darker ground).
DARK = {
    "#f68a2c": "#4a2414", "#ee6c24": "#22100b",          # ground
    "#f47a2a": "#e0662a", "#d8344e": "#a3243c",          # band-near tint
    "#ee5a32": "#c4502a", "#b8203f": "#7c1630",          # band-far tint
}


def dark_svg(svg: str) -> str:
    svg = recolor(svg, DARK)
    # Dim the soft top-left light (only the `light` layer's gradient).
    svg = re.sub(r'(<radialGradient id="lg2r2light-l"[^>]*><stop offset="0" stop-color="#fff" stop-opacity=")\.34',
                 r"\g<1>.12", svg)
    return svg


# --------------------------------------------------------------------------
# The squircle


def continuous_rect(x: float, y: float, w: float, h: float, r: float) -> list[tuple[float, float]]:
    """A rounded rectangle with Apple's continuous-curvature corners, flattened
    to a polygon. Coefficients are the ones UIKit/AppKit's continuous corner
    curve uses (each corner is two cubics plus an arc-like middle), in units of
    the nominal radius r measured from the corner."""
    # One corner, as cubic segments from the top edge toward the right edge,
    # in (dx from right edge, dy from top edge) units of r.
    segs = [
        ((1.52866483, 0.0), (1.08849323, 0.0), (0.86840689, 0.0), (0.63149399, 0.07491139)),
        ((0.63149399, 0.07491139), (0.37282392, 0.16905899), (0.16905899, 0.37282392), (0.07491139, 0.63149399)),
        ((0.07491139, 0.63149399), (0.0, 0.86840689), (0.0, 1.08849323), (0.0, 1.52866483)),
    ]

    def cubic(p0, p1, p2, p3, n=48):
        pts = []
        for i in range(n + 1):
            t = i / n
            a = (1 - t) ** 3
            b = 3 * (1 - t) ** 2 * t
            c = 3 * (1 - t) * t * t
            d = t ** 3
            pts.append((a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0],
                        a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1]))
        return pts

    corner = []  # in units of r, measured inward from the top-right corner
    for s in segs:
        corner += cubic(*s)

    pts: list[tuple[float, float]] = []
    # top-right: (right - dx*r, top + dy*r)
    pts += [(x + w - dx * r, y + dy * r) for dx, dy in corner]
    # bottom-right: rotate: (right - dy*r, bottom - dx*r)
    pts += [(x + w - dy * r, y + h - dx * r) for dx, dy in corner]
    # bottom-left
    pts += [(x + dx * r, y + h - dy * r) for dx, dy in corner]
    # top-left
    pts += [(x + dy * r, y + dx * r) for dx, dy in corner]
    return pts


def squircle_mask(size: int, x: float, y: float, side: float, r: float) -> Image.Image:
    """An L mask of the continuous rounded square, antialiased by supersampling."""
    big = Image.new("L", (size * SS, size * SS), 0)
    ImageDraw.Draw(big).polygon(
        [(px * SS, py * SS) for px, py in continuous_rect(x, y, side, side, r)], fill=255)
    return big.resize((size, size), Image.Resampling.BOX)


# --------------------------------------------------------------------------
# .icns


def packbits_icns(data: bytes) -> bytes:
    """Apple's icns RLE for is32/il32 channels: a byte n < 0x80 copies the next
    n+1 bytes; n >= 0x80 repeats the next byte n-125 times (3..130)."""
    out = bytearray()
    i, n = 0, len(data)
    while i < n:
        run = 1
        while i + run < n and run < 130 and data[i + run] == data[i]:
            run += 1
        if run >= 3:
            out += bytes([run + 125, data[i]])
            i += run
            continue
        lit_start = i
        while i < n and i - lit_start < 128:
            run = 1
            while i + run < n and run < 3 and data[i + run] == data[i]:
                run += 1
            if run >= 3:
                break
            i += 1
        out += bytes([i - lit_start - 1]) + data[lit_start:i]
    return bytes(out)


def rle_rgb(im: Image.Image) -> bytes:
    r, g, b, _ = im.convert("RGBA").split()
    return b"".join(packbits_icns(c.tobytes()) for c in (r, g, b))


def png_bytes(im: Image.Image) -> bytes:
    buf = io.BytesIO()
    im.save(buf, "PNG", optimize=True)
    return buf.getvalue()


def write_icns(path: Path, images: dict[int, Image.Image]) -> list[tuple[bytes, int]]:
    """Writes an .icns with the same members iconutil makes from a full
    iconset, plus the classic RLE 16 and 32 for old Finder paths:
    is32/s8mk (16), il32/l8mk (32), and PNG ic11..ic14, ic07..ic10."""
    entries: list[tuple[bytes, bytes]] = []
    for kind, mask, px in ((b"is32", b"s8mk", 16), (b"il32", b"l8mk", 32)):
        im = images[px].convert("RGBA")
        entries.append((kind, rle_rgb(im)))
        entries.append((mask, im.split()[3].tobytes()))
    for kind, px in ((b"ic11", 32), (b"ic12", 64), (b"ic07", 128), (b"ic13", 256),
                     (b"ic08", 256), (b"ic14", 512), (b"ic09", 512), (b"ic10", 1024)):
        entries.append((kind, png_bytes(images[px])))

    toc = b"".join(k + struct.pack(">I", 8 + len(d)) for k, d in entries)
    body = b"TOC " + struct.pack(">I", 8 + len(toc)) + toc
    body += b"".join(k + struct.pack(">I", 8 + len(d)) + d for k, d in entries)
    path.write_bytes(b"icns" + struct.pack(">I", 8 + len(body)) + body)
    return [(k, 8 + len(d)) for k, d in entries]


def unpackbits_icns(data: bytes, count: int) -> tuple[bytes, int]:
    out = bytearray()
    i = 0
    while len(out) < count:
        b = data[i]
        i += 1
        if b < 0x80:
            out += data[i:i + b + 1]
            i += b + 1
        else:
            out += bytes([data[i]]) * (b - 125)
            i += 1
    return bytes(out), i


def verify_icns(path: Path, expected: dict[int, Image.Image]) -> list[str]:
    """Parses the .icns back, independently of the writer, and checks every
    member decodes to the image it was made from."""
    raw = path.read_bytes()
    assert raw[:4] == b"icns", "bad magic"
    assert struct.unpack(">I", raw[4:8])[0] == len(raw), "bad file length"
    members: dict[bytes, bytes] = {}
    off = 8
    while off < len(raw):
        kind = raw[off:off + 4]
        ln = struct.unpack(">I", raw[off + 4:off + 8])[0]
        assert ln >= 8 and off + ln <= len(raw), f"bad length for {kind!r}"
        members[kind] = raw[off + 8:off + ln]
        off += ln
    assert off == len(raw)
    report = []
    toc = members.pop(b"TOC ")
    listed = [toc[i:i + 4] for i in range(0, len(toc), 8)]
    assert listed == list(members), "TOC does not match members"
    sizes = {b"ic11": 32, b"ic12": 64, b"ic07": 128, b"ic13": 256, b"ic08": 256,
             b"ic14": 512, b"ic09": 512, b"ic10": 1024}
    for kind, px in sizes.items():
        im = Image.open(io.BytesIO(members[kind]))
        im.load()
        assert im.size == (px, px) and im.mode == "RGBA", (kind, im.size, im.mode)
        assert ImageChops.difference(im, expected[px].convert("RGBA")).getbbox() is None, kind
        report.append(f"{kind.decode()} PNG {px}x{px} RGBA ok")
    for kind, mask, px in ((b"is32", b"s8mk", 16), (b"il32", b"l8mk", 32)):
        data = members[kind]
        chans, pos = [], 0
        for _ in range(3):
            c, used = unpackbits_icns(data[pos:], px * px)
            chans.append(c)
            pos += used
        assert pos == len(data), f"{kind!r} has trailing bytes"
        im = Image.merge("RGBA", [Image.frombytes("L", (px, px), c) for c in chans]
                         + [Image.frombytes("L", (px, px), members[mask])])
        assert ImageChops.difference(im, expected[px].convert("RGBA")).getbbox() is None, kind
        report.append(f"{kind.decode()}+{mask.decode()} RLE {px}x{px} ok")
    # And Pillow's own reader, as a second opinion.
    with Image.open(path) as im:
        found = sorted(im.info["sizes"])
        im.load()  # Pillow loads the largest member
        assert im.size == (1024, 1024), im.size
        report.append(f"Pillow reads it too: {found}, largest {im.size}")
    return report


# --------------------------------------------------------------------------
# Icon composition


def tile_art(svg_text: str, work: Path, name: str) -> Image.Image:
    """Renders full-bleed art at TILE*SS px."""
    src = work / f"{name}.svg"
    src.write_text(svg_text)
    png = work / f"{name}.png"
    render([(src, png, TILE * SS)])
    return Image.open(png).convert("RGBA")


def on_grid(art: Image.Image) -> Image.Image:
    """Places full-bleed art on Apple's grid: the TILE square at MARGIN on the
    CANVAS, masked to the continuous rounded square, transparent outside.
    Returned at CANVAS*SS so every smaller size is a clean downsample."""
    big = Image.new("RGBA", (CANVAS * SS, CANVAS * SS), (0, 0, 0, 0))
    big.paste(art, (MARGIN * SS, MARGIN * SS))
    mask = squircle_mask(CANVAS * SS, MARGIN * SS, MARGIN * SS, TILE * SS, RADIUS * SS)
    a = ImageChops.multiply(big.split()[3], mask)
    big.putalpha(a)
    return big


def small_svg(svg: str) -> str:
    """The art for 16 and 32 px: the same drawing with the hairline detail
    taken out, so the mic's silhouette carries the icon. The glass bands lose
    their white rims and inner glow, and the soft top-left light goes."""
    svg = re.sub(r'stroke="url\(#lg2r2[ab]-rim\)"', 'stroke="none"', svg)
    svg = re.sub(r'fill="url\(#lg2r2[ab]-glow\)"', 'fill="none"', svg)
    svg = re.sub(r'<!-- light -->.*?(?=<!-- shadow -->)', '', svg, flags=re.S)
    return svg


def sizes_from(big: Image.Image, small: Image.Image) -> dict[int, Image.Image]:
    out = {}
    for px in (1024, 512, 256, 128, 64, 32, 16):
        im = (small if px <= 32 else big).resize((px, px), Image.Resampling.LANCZOS)
        if px <= 32:
            # A touch of sharpening keeps the mic's ink edges from going soft.
            rgb = im.convert("RGB").filter(ImageFilter.UnsharpMask(radius=0.6, percent=60, threshold=0))
            rgb.putalpha(im.split()[3])
            im = rgb
        out[px] = im
    return out


def grayscale_tinted(big: Image.Image) -> Image.Image:
    """The tinted (mono) appearance: luminance only, so the system's tint can
    colour it, with the foreground light on a near-black ground."""
    a = big.split()[3]
    g = ImageOps.grayscale(big.convert("RGB"))
    g = ImageOps.autocontrast(g, cutoff=1)
    out = Image.merge("RGBA", (g, g, g, a))
    return out


# --------------------------------------------------------------------------
# Icon Composer package


def band_asset(layer: Path) -> str:
    """A band layer, cut to the 512 icon frame and drawn at 1024, flattened as
    Apple asks: no drop-shadow filter, no layer opacity (set in icon.json)."""
    s = layer.read_text()
    s = re.sub(r'<svg [^>]*>', '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
               'viewBox="0 0 512 512" width="1024" height="1024">', s, count=1)
    s = re.sub(r'<filter .*?</filter>', '', s, flags=re.S)
    s = re.sub(r'\s*filter="url\(#[^)]*\)"', '', s)
    s = re.sub(r'\s*fill-opacity="0\.\d+"', '', s)
    return s


def layer_png(svg: Path, work: Path, name: str, extra=None) -> Image.Image:
    s = unclipped(svg.read_text())
    if extra:
        s = extra(s)
    src = work / f"{name}.svg"
    src.write_text(s)
    png = work / f"{name}.png"
    render([(src, png, 1024)])
    return Image.open(png).convert("RGBA")


def write_icon_composer(work: Path) -> None:
    pkg = OUT / "AppIcon.icon"
    if pkg.exists():
        shutil.rmtree(pkg)
    assets = pkg / "Assets"
    assets.mkdir(parents=True)

    (assets / "1-band-far.svg").write_text(band_asset(LAYERS / "band-far.svg"))
    (assets / "2-band-near.svg").write_text(band_asset(LAYERS / "band-near.svg"))
    # The ink layers and the emblem use patterns and clip paths Icon Composer
    # may not take as SVG, so they go in as lossless 1024 PNGs, as Apple
    # suggests for SVG it can't read.
    for name, src in (("3-shadow", LAYERS / "shadow/shadow-00.svg"),
                      ("4-cape", LAYERS / "cape/cape-00.svg"),
                      ("5-mic", LAYERS / "mic.svg"),
                      ("6-emblem", LAYERS / "emblem.svg")):
        layer_png(src, work, name).save(assets / f"{name}.png", optimize=True)

    def c(hexstr: str, a: float = 1.0) -> str:
        r, g, b = (int(hexstr[i:i + 2], 16) / 255 for i in (1, 3, 5))
        return f"srgb:{r:.5f},{g:.5f},{b:.5f},{a:.5f}"

    doc = {
        # The ground: the orange gradient, made by Icon Composer from one
        # colour rather than imported (Apple: prefer a fill to a background
        # image). The dark appearance gets the ember brown.
        "fill": {"automatic-gradient": c("#f27b28")},
        "fill-specializations": [
            {"appearance": "dark", "value": {"automatic-gradient": c("#3a1c10")}},
        ],
        # Front to back, as in Icon Composer's sidebar.
        "groups": [
            {
                "name": "Emblem",
                "lighting": "individual",
                "specular": True,
                "shadow": {"kind": "neutral", "opacity": 0.5},
                "translucency": {"enabled": False, "value": 0.5},
                "layers": [
                    {"name": "6-emblem", "image-name": "6-emblem.png", "glass": False},
                ],
            },
            {
                "name": "Ink",
                "lighting": "combined",
                "specular": True,
                "shadow": {"kind": "neutral", "opacity": 0.5},
                "translucency": {"enabled": False, "value": 0.5},
                "layers": [
                    {"name": "5-mic", "image-name": "5-mic.png", "glass": False},
                    {"name": "4-cape", "image-name": "4-cape.png", "glass": False},
                    {"name": "3-shadow", "image-name": "3-shadow.png", "glass": False},
                ],
            },
            {
                "name": "Glass bands",
                "lighting": "individual",
                "specular": True,
                "shadow": {"kind": "layer-color", "opacity": 0.4},
                "translucency": {"enabled": True, "value": 0.4},
                "layers": [
                    {"name": "2-band-near", "image-name": "2-band-near.svg", "glass": True, "opacity": 0.78},
                    {"name": "1-band-far", "image-name": "1-band-far.svg", "glass": True, "opacity": 0.88},
                ],
            },
        ],
        "supported-platforms": {"squares": ["macOS"]},
    }
    (pkg / "icon.json").write_text(json.dumps(doc, indent=2) + "\n")


# --------------------------------------------------------------------------
# Menu bar template


def _polys(d: str) -> list[list[tuple[float, float]]]:
    """The subpaths of an M/L/Z polyline path, as point lists."""
    out = []
    for sub in d.split("M")[1:]:
        nums = [float(x) for x in re.findall(r"-?\d+\.?\d*", sub)]
        out.append(list(zip(nums[0::2], nums[1::2])))
    return out


def menubar_glyph(px_per_unit: float | None = None, detail: str = "full"):
    """The menu bar glyph as one shapely geometry, in the mic's own frame
    (the frame of mic.svg, cape-00.svg and emblem.svg inside their
    `translate(238 290) rotate(40) scale(1.3216)`).

    A faithful simplification of the master: the desk mic's housing (mesh
    dome, seam, body with a flat bottom, the stubby side knobs), all grown by
    the master's ink outline; the cape-00 outline behind it, parted from the
    mic by a clear gap; and the S roundel on the housing as a cut-out. Cuts
    are never thinner than one device pixel (px_per_unit), so they survive
    the size they are drawn at. detail "full" cuts the roundel's ring and its S;
    "disc" cuts the roundel as one clear disc, for 1x sizes where the S can't
    resolve."""
    from shapely.geometry import Polygon, box, Point
    from shapely.ops import unary_union

    px = 1.0 / px_per_unit if px_per_unit else 0.0  # one pixel, in mic units

    import math
    dome = [(-63, -12), (-63, -87)]
    dome += [(63 * math.cos(math.radians(a)), -87 - 63 * math.sin(math.radians(a))) for a in range(180, -1, -5)]
    dome += [(63, -87), (63, -12)]
    head = Polygon(dome)
    seam = box(-63, -12, 63, 0)
    body = Polygon([(-63, 0), (63, 0), (63, 130), (58, 145), (43, 150), (-43, 150), (-58, 145), (-63, 130)])
    knobs = [box(-87, 4, -63, 46), box(63, 4, 87, 46), box(63, 86, 80, 128)]
    ink = 6  # half the master's 12-unit outline stroke
    housing = unary_union([head, seam, body, *knobs]).buffer(ink, join_style=1)

    # The seam: the master's dark band between dome and body, here a clear line.
    seam_cut = box(-120, -6 - max(5, px / 2), 120, -6 + max(5, px / 2))
    mic = housing.difference(seam_cut)

    # The S roundel, from emblem.svg: the outer ink disc and the outlined S.
    em = re.findall(r'<path d="([^"]*)"', (LAYERS / "emblem.svg").read_text())
    disc = Polygon(_polys(em[1])[0])
    s_shape = unary_union([Polygon(p) for p in _polys(em[11])])
    if detail == "full":
        ring_w = max(7, px)
        ring = disc.buffer(ring_w / 2).difference(disc.buffer(-ring_w / 2))
        mic = mic.difference(ring).difference(s_shape.buffer(max(0, px / 2 - 3)))
    else:
        # Too small for the S: the roundel is one clear disc on the housing.
        mic = mic.difference(disc)

    # The cape, cape-00's own outline, with a clear gap around the mic.
    cape_d = re.search(r'<path d="([^"]*)" fill="url\(#dskcape0-cape\)"',
                       (LAYERS / "cape/cape-00.svg").read_text()).group(1)
    cape = Polygon(_polys(cape_d)[0]).buffer(0)
    gap = max(14, px * 1.0)
    cape = cape.difference(housing.buffer(gap))
    return unary_union([cape, mic])


def _svg_path(geom) -> str:
    from shapely.geometry import Polygon
    polys = [geom] if isinstance(geom, Polygon) else list(geom.geoms)
    parts = []
    for p in polys:
        for ring in [p.exterior, *p.interiors]:
            pts = list(ring.coords)[:-1]
            parts.append("M" + "L".join(f"{x:.3f} {y:.3f}" for x, y in pts) + "Z")
    return "".join(parts)


def menubar_svg(px_box: float, scale: int = 1, detail: str | None = None) -> str:
    """The glyph turned to the icon's 45 degree heading and fitted to a
    px_box-point square, cut for drawing at `scale` pixels per point."""
    from shapely import affinity
    base = affinity.rotate(menubar_glyph(None), 45, origin=(0, 0))
    minx, miny, maxx, maxy = base.bounds
    inset = px_box * 0.03
    s = (px_box - 2 * inset) / max(maxx - minx, maxy - miny)
    if detail is None:
        detail = "full" if px_box * scale >= 32 else "disc"
    g = affinity.rotate(menubar_glyph(s * scale, detail), 45, origin=(0, 0))
    minx, miny, maxx, maxy = g.bounds
    g = affinity.affine_transform(g, [s, 0, 0, s, px_box / 2 - s * (minx + maxx) / 2, px_box / 2 - s * (miny + maxy) / 2])
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {px_box:g} {px_box:g}" width="{px_box:g}" height="{px_box:g}">
<!-- Super Voice menu bar template, {px_box:g} pt cut for @{scale}x: black and clear only; load with isTemplate = true. -->
<path fill="#000" fill-rule="evenodd" d="{_svg_path(g)}"/>
</svg>
"""


def menubar_in_icon_frame(px_per_unit: float, detail: str) -> str:
    """The glyph in the 512 icon frame, over where the master draws the mic,
    for the side-by-side blur check."""
    g = menubar_glyph(px_per_unit, detail)
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">
<g transform="rotate(5 238 290) translate(238 290) rotate(40) scale(1.3216)"><path fill="#000" fill-rule="evenodd" d="{_svg_path(g)}"/></g>
</svg>
"""


def write_menubar(work: Path) -> None:
    d = OUT / "menubar"
    if d.exists():
        shutil.rmtree(d)
    d.mkdir()
    (d / "MenuBarIconTemplate.svg").write_text(menubar_svg(18, 2))
    jobs = []
    for pt in (16, 18, 22):
        for scale, suffix in ((1, ""), (2, "@2x")):
            src = work / f"menubar-{pt}-{scale}.svg"
            src.write_text(menubar_svg(pt, scale))
            jobs.append((src, d / f"MenuBarIcon{pt}Template{suffix}.png", pt * scale))
    render(jobs)
    # Template rule: black only, the shape in alpha.
    for p in d.glob("*.png"):
        im = Image.open(p).convert("RGBA")
        black = Image.new("RGBA", im.size, (0, 0, 0, 0))
        black.putalpha(im.split()[3])
        black.save(p, optimize=True)
    # PDF (vector), 18 pt, cut for Retina: one even-odd path, no masks.
    pdf_page(d / "MenuBarIconTemplate.svg", d / "MenuBarIconTemplate.pdf", 18)
    blur_check(work)


def blur_check(work: Path) -> None:
    """Draws the master's mic, cape and emblem and the glyph in the same frame
    at the menu bar sizes, blurs both, and puts them side by side in
    menubar/blur-check.png: they should read as the same object."""
    ink = work / "ink.svg"
    parts = []
    for f in ("cape/cape-00.svg", "mic.svg", "emblem.svg"):
        s = unclipped((LAYERS / f).read_text())
        s = re.sub(r"<\?xml[^>]*>", "", s)
        parts.append(s.replace("<svg ", '<svg x="0" y="0" ', 1))
    ink.write_text('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">'
                   + "".join(parts) + "</svg>")
    jobs = [(ink, work / "ink-512.png", 512)]
    # The mic spans ~420 of the 512 frame; at 18 pt that is ~17 px.
    for px in (18, 36):
        g = work / f"glyph-{px}.svg"
        g.write_text(menubar_in_icon_frame(px / 420 * 1.3216, "full" if px >= 32 else "disc"))
        jobs.append((g, work / f"glyph-{px}.png", 512))
    render(jobs)
    master = Image.open(work / "ink-512.png").convert("RGBA")
    # The master as a template would see it: dark ink and colour both as shape.
    m_l = ImageOps.grayscale(Image.alpha_composite(Image.new("RGBA", master.size, "white"), master).convert("RGB"))
    tiles = []
    for px in (18, 36):
        glyph = Image.open(work / f"glyph-{px}.png").convert("RGBA")
        gl = Image.alpha_composite(Image.new("RGBA", glyph.size, "white"), glyph).convert("L")
        row = []
        for im in (m_l, gl):
            small = im.resize((px * 2, px * 2), Image.Resampling.LANCZOS)  # ~the size on screen
            for blur in (0, 1.2):
                b = small.filter(ImageFilter.GaussianBlur(blur)) if blur else small
                row.append(b.resize((192, 192), Image.Resampling.NEAREST))
        tiles.append(row)
    sheet = Image.new("L", (4 * 200 + 10, 2 * 200 + 40), 255)
    dr = ImageDraw.Draw(sheet)
    f = font(14)
    for c, label in enumerate(("master", "master blurred", "glyph", "glyph blurred")):
        dr.text((10 + c * 200, 8), label, fill=0, font=f)
    for r, row in enumerate(tiles):
        for c, t in enumerate(row):
            sheet.paste(t, (10 + c * 200, 30 + r * 200))
    sheet.save(OUT / "menubar" / "blur-check.png", optimize=True)


def pdf_page(svg: Path, out: Path, pt: int) -> None:
    script = f"""
let pw; try {{ pw = require('playwright'); }} catch {{ pw = require('/opt/node22/lib/node_modules/playwright'); }}
(async () => {{
  const b = await pw.chromium.launch();
  const p = await b.newPage();
  const svg = require('fs').readFileSync({json.dumps(str(svg))}, 'utf8')
    .replace(/width="[^"]*" height="[^"]*"/, 'width="{pt}pt" height="{pt}pt"');
  await p.setContent('<!doctype html><style>@page{{size:{pt}pt {pt}pt;margin:0}}html,body{{margin:0;background:transparent}}svg{{display:block}}</style>' + svg);
  await p.pdf({{ path: {json.dumps(str(out))}, width: '{pt / 72:.6f}in', height: '{pt / 72:.6f}in', printBackground: false, pageRanges: '1' }});
  await b.close();
}})();
"""
    subprocess.run(["node", "-e", script], check=True)


# --------------------------------------------------------------------------
# Preview sheet


def font(size: int):
    for f in ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
              "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf"):
        if Path(f).exists():
            return ImageFont.truetype(f, size)
    return ImageFont.load_default(size)


def dock(icon: Image.Image, dark: bool, width: int) -> Image.Image:
    """A Dock mock: wallpaper, a translucent shelf, neighbour placeholders and
    the icon at 128 px (64 pt @2x)."""
    h = 260
    bg = Image.new("RGBA", (width, h))
    top, bottom = ((210, 222, 240), (240, 214, 200)) if not dark else ((24, 28, 44), (52, 30, 40))
    dr = ImageDraw.Draw(bg)
    for y in range(h):
        t = y / h
        dr.line([(0, y), (width, y)], fill=tuple(int(top[i] * (1 - t) + bottom[i] * t) for i in range(3)) + (255,))
    n, cell = 6, 112
    shelf_w, shelf_h = n * cell + 28, 136
    sx, sy = (width - shelf_w) // 2, h - shelf_h - 24
    shelf = Image.new("RGBA", bg.size, (0, 0, 0, 0))
    ImageDraw.Draw(shelf).rounded_rectangle(
        [sx, sy, sx + shelf_w, sy + shelf_h], 44,
        fill=(255, 255, 255, 110) if not dark else (60, 60, 70, 150),
        outline=(255, 255, 255, 140) if not dark else (255, 255, 255, 50), width=2)
    bg = Image.alpha_composite(bg, shelf)
    neighbours = [(90, 150, 230), (120, 200, 120), (240, 200, 70), None, (180, 120, 220), (120, 120, 130)]
    small = icon.resize((cell, cell), Image.Resampling.LANCZOS)
    for i, col in enumerate(neighbours):
        x = sx + 14 + i * cell
        y = sy + 8
        if col is None:
            bg.alpha_composite(small, (x, y))
            ImageDraw.Draw(bg).ellipse([x + cell // 2 - 3, sy + shelf_h - 12, x + cell // 2 + 3, sy + shelf_h - 6],
                                       fill=(40, 40, 40) if not dark else (230, 230, 230))
        else:
            ph = Image.new("RGBA", (cell, cell), (0, 0, 0, 0))
            m = squircle_mask(cell, cell * 100 / 1024, cell * 100 / 1024, cell * 824 / 1024, cell * 185.4 / 1024)
            fill = Image.new("RGBA", (cell, cell), col + (255,))
            ph.paste(fill, (0, 0), m)
            bg.alpha_composite(ph, (x, y))
    return bg


def preview(sizes: dict[int, Image.Image], dark_big: Image.Image, tinted_big: Image.Image) -> None:
    W = 1600
    sheet = Image.new("RGBA", (W, 1500), (246, 244, 240, 255))
    d = ImageDraw.Draw(sheet)
    f, fb = font(18), font(26)
    d.text((40, 28), "Super Voice: macOS app icon (Apple 1024 grid: 824 tile, continuous corners r 185.4)", fill=(30, 30, 30), font=fb)

    # All sizes at 1:1, on light and dark strips.
    y = 80
    for bgc, fg in (((246, 244, 240), (40, 40, 40)), ((30, 30, 34), (220, 220, 220))):
        strip = Image.new("RGBA", (W - 80, 300), bgc + (255,))
        x = 20
        for px in (16, 32, 64, 128, 256):
            strip.alpha_composite(sizes[px], (x, 280 - px - 24))
            ImageDraw.Draw(strip).text((x, 282 - 20), f"{px}", fill=fg, font=f)
            x += px + 40
        # 512 shown at half scale would not be 1:1; show 512 cropped? Show 1024 at 256 label.
        sheet.alpha_composite(strip, (40, y))
        y += 310
    d.text((40 + 820, 100), "Each icon at its real pixel size: 16, 32, 64, 128, 256.\n16 and 32 use the simplified small-size art.\nBelow: 1024 scaled down, the dark and tinted\nappearances, the menu bar template and Docks.", fill=(60, 60, 60), font=f)

    # Default / dark / tinted at 256 each, and 1024 scaled.
    y = 720
    row = [("Default (AppIcon.icns)", sizes[1024]),
           ("Dark", dark_big.resize((1024, 1024), Image.Resampling.LANCZOS)),
           ("Tinted (grayscale for tint)", tinted_big.resize((1024, 1024), Image.Resampling.LANCZOS))]
    x = 40
    for label, im in row:
        panel = Image.new("RGBA", (300, 330), (225, 222, 216, 255) if label != "Dark" else (40, 40, 46, 255))
        panel.alpha_composite(im.resize((280, 280), Image.Resampling.LANCZOS), (10, 10))
        ImageDraw.Draw(panel).text((12, 298), label, fill=(30, 30, 30) if label != "Dark" else (230, 230, 230), font=f)
        sheet.alpha_composite(panel, (x, y))
        x += 320

    # Menu bar template on light and dark bars.
    mb = OUT / "menubar"
    x0 = 1020
    for i, (bar, fgc) in enumerate((((236, 236, 236), (0, 0, 0)), ((40, 40, 44), (255, 255, 255)))):
        barimg = Image.new("RGBA", (520, 70), bar + (255,))
        bx = 20
        for pt in (16, 18, 22):
            t = Image.open(mb / f"MenuBarIcon{pt}Template@2x.png").convert("RGBA")
            tint = Image.new("RGBA", t.size, fgc + (0,))
            tint.putalpha(t.split()[3])
            barimg.alpha_composite(tint, (bx, (70 - t.height) // 2))
            ImageDraw.Draw(barimg).text((bx + t.width + 8, 24), f"{pt}pt@2x", fill=fgc, font=f)
            bx += t.width + 110
        sheet.alpha_composite(barimg, (x0, y + 20 + i * 90))
    d.text((x0, y + 200), "Menu bar template (black + alpha; system tints it)", fill=(60, 60, 60), font=f)

    # Docks.
    y = 1080
    dl = dock(sizes[1024], False, (W - 100) // 2)
    dd = dock(dark_big.resize((1024, 1024), Image.Resampling.LANCZOS), True, (W - 100) // 2)
    sheet.alpha_composite(dl, (40, y))
    sheet.alpha_composite(dd, (60 + dl.width, y))
    d.text((40, y + dl.height + 10), "Dock, light (default icon)", fill=(60, 60, 60), font=f)
    d.text((60 + dl.width, y + dl.height + 10), "Dock, dark (dark variant)", fill=(60, 60, 60), font=f)
    sheet = sheet.crop((0, 0, W, y + dl.height + 50))
    sheet.convert("RGB").save(OUT / "preview.png", optimize=True)


# --------------------------------------------------------------------------


def main() -> int:
    work = Path(tempfile.mkdtemp(prefix="sv-macos-icon-"))
    try:
        master = MASTER.read_text()

        art = tile_art(unclipped(master), work, "default")
        big = on_grid(art)
        small = on_grid(tile_art(unclipped(small_svg(master)), work, "small"))
        sizes = sizes_from(big, small)

        iconset = OUT / "AppIcon.iconset"
        if iconset.exists():
            shutil.rmtree(iconset)
        iconset.mkdir()
        for name, px in ICONSET:
            sizes[px].save(iconset / name, optimize=True)
        # Use exactly the iconset's bytes for the .icns, so both match.
        from_disk = {px: Image.open(iconset / name).convert("RGBA") for name, px in ICONSET}
        write_icns(OUT / "AppIcon.icns", from_disk)
        for line in verify_icns(OUT / "AppIcon.icns", from_disk):
            print("  icns:", line)

        variants = OUT / "variants"
        variants.mkdir(exist_ok=True)
        dark_big = on_grid(tile_art(unclipped(dark_svg(master)), work, "dark"))
        dark_big.resize((1024, 1024), Image.Resampling.LANCZOS).save(variants / "AppIcon-dark-1024.png", optimize=True)
        # Tinted: from the dark rendition, so the ground is near black and the
        # foreground carries the light values the tint colours.
        tinted_big = grayscale_tinted(dark_big)
        tinted_big.resize((1024, 1024), Image.Resampling.LANCZOS).save(variants / "AppIcon-tinted-1024.png", optimize=True)
        # The full-bleed squares (no mask), for Xcode asset catalogs or Icon
        # Composer, which mask the icon themselves.
        art.resize((1024, 1024), Image.Resampling.LANCZOS).save(variants / "AppIcon-default-fullbleed-1024.png", optimize=True)
        dsq = tile_art(unclipped(dark_svg(master)), work, "dark-sq")
        dsq.resize((1024, 1024), Image.Resampling.LANCZOS).save(variants / "AppIcon-dark-fullbleed-1024.png", optimize=True)
        tsq = grayscale_tinted(dsq)
        tsq.resize((1024, 1024), Image.Resampling.LANCZOS).save(variants / "AppIcon-tinted-fullbleed-1024.png", optimize=True)

        write_icon_composer(work)
        write_menubar(work)
        preview(sizes, dark_big, tinted_big)
        print("wrote", OUT)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
