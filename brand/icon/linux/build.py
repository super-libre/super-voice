#!/usr/bin/env python3
"""Build the Linux (freedesktop hicolor) icon set from the master SVG.

Usage, from the repository root:

    python3 brand/icon/linux/build.py

Needs rsvg-convert (librsvg2-bin), scour and shapely (pip), and Pillow +
lxml + numpy.

What it does:

1. Flattens brand/icon/master/super-voice-icon.svg into SVG that librsvg
   (and other strict renderers) draw the same everywhere:
   - each nested <svg> layer becomes a <g> (every layer's viewBox maps 1:1
     onto its x/y/width/height, so no transform is needed, and the tile's
     own clip already cuts tighter than each layer's overflow="hidden");
   - each <feDropShadow> becomes feGaussianBlur/feOffset/feFlood/
     feComposite/feMerge, since librsvg only learned feDropShadow in 2.56;
   - SVG 2 `href` becomes `xlink:href`, which every renderer reads;
   - unused defs (patterns, gradients, clip paths) and editor cruft go,
     through scour.
2. Shrinks the 512 tile into the GNOME / Flathub square safe area: a
   416 x 416 tile (104/128 of the canvas, the square guide in GNOME's app
   icon template) centred with 48 px of transparent margin.
3. Renders hicolor PNGs at 16-512. 16, 22, 24 and 32 use a tighter margin
   (1-2 px) so the tile stays legible in panels and menus.
4. Writes the one-colour symbolic icon, traced from the master's layer
   geometry (see symbolic_path), and a preview sheet.
"""

from __future__ import annotations

import copy
import io
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np
from lxml import etree
from PIL import Image, ImageDraw, ImageFont

ICON = "super-voice-app"
ROOT = Path(__file__).resolve().parents[3]
MASTER = ROOT / "brand/icon/master/super-voice-icon.svg"
OUT = ROOT / "brand/icon/linux"
THEME = OUT / "hicolor"

SVG_NS = "http://www.w3.org/2000/svg"
XLINK_NS = "http://www.w3.org/1999/xlink"
S = f"{{{SVG_NS}}}"
XLINK_HREF = f"{{{XLINK_NS}}}href"

SIZES = [16, 22, 24, 32, 48, 64, 128, 256, 512]
# Tile inset, in output pixels, for the sizes that do not use the safe area.
SMALL_INSET = {16: 1, 22: 1, 24: 1, 32: 2}
# The safe area: a 104/128 square, centred (GNOME template square guide).
SAFE = 104 / 128


def flatten(master: Path) -> etree._ElementTree:
    tree = etree.parse(str(master))
    root = tree.getroot()

    # Nested <svg> layers -> <g>, checking each maps 1:1.
    for svg in list(root.iter(f"{S}svg")):
        if svg is root:
            continue
        x, y = float(svg.get("x", 0)), float(svg.get("y", 0))
        w, h = float(svg.get("width")), float(svg.get("height"))
        vb = [float(v) for v in svg.get("viewBox").split()]
        if vb != [x, y, w, h]:
            sys.exit(f"nested svg with a non-identity viewBox: {vb} vs {x, y, w, h}")
        svg.tag = f"{S}g"
        for attr in ("x", "y", "width", "height", "viewBox", "overflow",
                     "preserveAspectRatio"):
            svg.attrib.pop(attr, None)

    # feDropShadow -> the SVG 1.1 primitive chain.
    for ds in list(root.iter(f"{S}feDropShadow")):
        f = ds.getparent()
        f.remove(ds)
        E = lambda tag, **kw: etree.SubElement(f, f"{S}{tag}", {k.replace('_', '-'): str(v) for k, v in kw.items()})
        E("feGaussianBlur", **{"in": "SourceAlpha"}, stdDeviation=ds.get("stdDeviation", "2"))
        E("feOffset", dx=ds.get("dx", "2"), dy=ds.get("dy", "2"), result="off")
        E("feFlood", flood_color=ds.get("flood-color", "#000"),
          flood_opacity=ds.get("flood-opacity", "1"))
        E("feComposite", in2="off", operator="in", result="shadow")
        m = E("feMerge")
        etree.SubElement(m, f"{S}feMergeNode", {"in": "shadow"})
        etree.SubElement(m, f"{S}feMergeNode", {"in": "SourceGraphic"})

    # href -> xlink:href
    for el in root.iter():
        if "href" in el.attrib:
            el.set(XLINK_HREF, el.attrib.pop("href"))

    # Drop comments.
    for c in root.xpath("//comment()"):
        p = c.getparent()
        if c.tail and c.tail.strip():
            prev = c.getprevious()
            if prev is not None:
                prev.tail = (prev.tail or "") + c.tail
            else:
                p.text = (p.text or "") + c.tail
        p.remove(c)
    return tree


def place(tree: etree._ElementTree, scale: float, offset: float) -> bytes:
    """Wrap the artwork in a translate+scale group on a 512 canvas."""
    tree = copy.deepcopy(tree)
    root = tree.getroot()
    g = etree.Element(f"{S}g", transform=f"matrix({scale:g} 0 0 {scale:g} {offset:g} {offset:g})")
    for child in list(root):
        g.append(child)
    root.append(g)
    root.set("width", "512")
    root.set("height", "512")
    root.set("viewBox", "0 0 512 512")
    return etree.tostring(tree, xml_declaration=True, encoding="UTF-8")


def scour(svg: bytes) -> bytes:
    with tempfile.TemporaryDirectory() as d:
        src, dst = Path(d, "in.svg"), Path(d, "out.svg")
        src.write_bytes(svg)
        subprocess.run(
            [sys.executable, "-m", "scour.scour", "-i", str(src), "-o", str(dst),
             "--set-precision=6", "--enable-id-stripping", "--shorten-ids",
             "--remove-metadata", "--enable-comment-stripping",
             "--no-line-breaks", "--strip-xml-space", "--quiet"],
            check=True,
        )
        return dst.read_bytes()


def rsvg(svg: Path | bytes, size: int) -> Image.Image:
    data = svg.read_bytes() if isinstance(svg, Path) else svg
    out = subprocess.run(
        ["rsvg-convert", "-w", str(size), "-h", str(size), "-f", "png"],
        input=data, capture_output=True, check=True,
    ).stdout
    return Image.open(io.BytesIO(out)).convert("RGBA")


SYMBOLIC = """<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16">
<defs><style id="current-color-scheme" type="text/css">.ColorScheme-Text{color:#232629}</style></defs>
<path class="ColorScheme-Text" fill="currentColor" fill-rule="evenodd" d="{d}"/>
</svg>
"""


def symbolic_path() -> str:
    """The master's caped desk mic as a one-colour 16 px glyph.

    A simplification of the master, not a new drawing. The geometry is
    measured from the master's mic, emblem and cape layers rotated back so
    the flight axis is horizontal (512 units, x = flight direction):

    - capsule y 172..362 (half-width 95), flat bottom at x 9 with small
      rounded corners, mesh dome a half circle from x 335 to 430;
    - no seam gap: the mic is one solid body;
    - no side knobs: at panel sizes the clean capsule reads better;
    - the S roundel centred at (126, 267) on the housing, cut out as a
      round hole (an S does not fit at 16 px);
    - the cape behind the housing, fanning back past the flat bottom to a
      straight hem between its two corners,
      kept 1 px clear of the mic.

    The whole is stood upright (dome on top, cape hanging below) and
    fitted to the 16 px canvas with a 1 px margin.
    """
    from shapely import affinity
    from shapely.geometry import Point, Polygon, box
    from shapely.ops import unary_union

    cy, half = 267, 95

    def capsule():
        back = box(9 + 28, cy - half + 28, 335, cy + half - 28).buffer(28, quad_segs=8)
        dome = Point(335, cy).buffer(half, quad_segs=16)
        return unary_union([back, box(60, cy - half, 335, cy + half), dome])

    # The cape fans back from the seam, as on the master's cape-00, and ends
    # in a straight hem level with the mic's flat bottom.
    CAPE_HEM = 9  # the straight hem, level with the mic's flat bottom
    # Wider than the mic right at the seam, so the cape's sides show from the
    # seam all the way down instead of appearing only halfway down the body.
    # Same flank angle as before; the hem now ends level with the mic.
    cape = Polygon([(236, 117), (CAPE_HEM, 66), (CAPE_HEM, 468), (236, 417)])
    cape = cape.buffer(10, quad_segs=6).buffer(-10, quad_segs=6)

    def fit(geoms, anchor, size=16, margin=1.0):
        rot = [affinity.rotate(g, -90, origin=(256, 256)) for g in geoms]
        minx, miny, maxx, maxy = affinity.rotate(anchor, -90, origin=(256, 256)).bounds
        k = (size - 2 * margin) / max(maxx - minx, maxy - miny)
        cx, cy_ = (minx + maxx) / 2, (miny + maxy) / 2
        return [affinity.affine_transform(
            g, [k, 0, 0, k, size / 2 - k * cx, size / 2 - k * cy_]) for g in rot], k

    k = 0.028  # px per master unit; refined below
    for _ in range(3):
        gap = 1.0 / k  # 1 px, in master units
        solid = capsule()
        mic = solid  # no seam gap: the mic reads as one solid body
        mic = mic.difference(Point(126, cy).buffer(52, quad_segs=16))
        wedge = cape.difference(solid.buffer(gap, quad_segs=8))
        anchor = unary_union([mic, wedge])
        (mic, wedge), k = fit([mic, wedge], anchor)
    wedge = wedge.intersection(box(0, 0, 16, 16))

    def d(geom):
        polys = [geom] if geom.geom_type == "Polygon" else list(geom.geoms)
        out = []
        for poly in polys:
            for ring in [poly.exterior, *poly.interiors]:
                pts = list(ring.coords)[:-1]
                out.append("M" + "L".join(
                    f"{x:.2f} {y:.2f}".replace(".00", "") for x, y in pts) + "Z")
        return "".join(out)

    return d(mic) + d(wedge)


def main() -> None:
    flat = flatten(MASTER)

    # 1. Scalable app icon, in the safe area.
    off = 512 * (1 - SAFE) / 2
    scalable = scour(place(flat, SAFE, off))
    sc_path = THEME / "scalable/apps" / f"{ICON}.svg"
    sc_path.parent.mkdir(parents=True, exist_ok=True)
    sc_path.write_bytes(scalable)

    # Full-bleed flattened copy, only to verify the flattening against the
    # master pixel for pixel; not installed.
    full = scour(place(flat, 1.0, 0.0))

    # 2. PNGs.
    pngs: dict[int, Image.Image] = {}
    for size in SIZES:
        if size in SMALL_INSET:
            inset = SMALL_INSET[size]
            k = (size - 2 * inset) / size
            img = rsvg(scour(place(flat, k, inset * 512 / size)), size)
        else:
            img = rsvg(scalable, size)
        d = THEME / f"{size}x{size}/apps"
        d.mkdir(parents=True, exist_ok=True)
        img.save(d / f"{ICON}.png", optimize=True)
        pngs[size] = img

    # 3. Symbolic.
    sym = SYMBOLIC.replace("{d}", symbolic_path())
    sy_path = THEME / "symbolic/apps" / f"{ICON}-symbolic.svg"
    sy_path.parent.mkdir(parents=True, exist_ok=True)
    sy_path.write_text(sym)

    # 4. Verify: flattened full-bleed vs master, both through librsvg.
    a = np.asarray(rsvg(MASTER, 512), float)
    b = np.asarray(rsvg(full, 512), float)
    diff = np.abs(a - b)
    print(f"flattened vs master @512: mean {diff.mean():.4f}, max {diff.max():.0f}, "
          f"pixels off by >8: {(diff.max(axis=2) > 8).sum()}")
    # Safe-area icon vs master scaled into the same box.
    m = rsvg(MASTER, 416)
    canvas = Image.new("RGBA", (512, 512))
    canvas.alpha_composite(m, (48, 48))
    c = np.abs(np.asarray(canvas, float) - np.asarray(rsvg(scalable, 512), float))
    print(f"scalable vs master in safe area @512: mean {c.mean():.4f}, max {c.max():.0f}")

    preview(pngs, sy_path)


def preview(pngs: dict[int, Image.Image], sym_path: Path) -> None:
    W, H = 1500, 960
    sheet = Image.new("RGBA", (W, H), (246, 245, 244, 255))
    sheet.alpha_composite(Image.new("RGBA", (W, 470), (36, 31, 49, 255)), (0, 490))
    draw = ImageDraw.Draw(sheet)
    try:
        font = ImageFont.truetype("DejaVuSans.ttf", 15)
        big = ImageFont.truetype("DejaVuSans-Bold.ttf", 20)
    except OSError:
        font = big = ImageFont.load_default()
    ink, paper = (36, 31, 49), (246, 245, 244)

    def row(y_base, sizes, fg, x=24):
        for size in sizes:
            img = pngs[size]
            label = f"{size}"
            if size == 512:
                img = img.resize((256, 256), Image.LANCZOS)
                label = "512 (at 256)"
            sheet.alpha_composite(img, (x, y_base - img.height))
            draw.text((x, y_base + 8), label, fill=fg, font=font)
            x += max(img.width, 40) + 24
        return x

    draw.text((24, 16), "Super Voice hicolor app icon, every size at 1:1 (light)",
              fill=ink, font=big)
    row(330, SIZES, ink)
    draw.text((24, 380), "Transparent margin = the GNOME/Flathub safe area (416/512); "
              "16-32 px use a 1-2 px margin.", fill=ink, font=font)

    draw.text((24, 506), "Same on dark", fill=paper, font=big)
    row(680, [16, 22, 24, 32, 48, 64, 128], paper)

    draw.text((24, 730), "Symbolic icon (-symbolic.svg), recoloured as panels do",
              fill=paper, font=big)
    sym = sym_path.read_text()
    panels = [("light panel", (250, 250, 250, 255), "#2e3436"),
              ("dark panel", (30, 30, 30, 255), "#f6f5f4"),
              ("accent", (238, 108, 36, 255), "#ffffff")]
    px = 24
    for name, bg, fg in panels:
        panel = Image.new("RGBA", (420, 170), bg)
        pd = ImageDraw.Draw(panel)
        recol = sym.replace("color:#232629", f"color:{fg}")
        xx = 16
        for size in (16, 24, 32, 64):
            im = rsvg(recol.encode(), size)
            panel.alpha_composite(im, (xx, 90 - size))
            pd.text((xx, 100), str(size), fill=fg, font=font)
            xx += max(size, 30) + 30
        pd.text((16, 138), name, fill=fg, font=font)
        sheet.alpha_composite(panel, (px, 770))
        px += 440

    sheet.convert("RGB").save(OUT / "preview.png", optimize=True)


if __name__ == "__main__":
    main()
