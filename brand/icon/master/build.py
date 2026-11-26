#!/usr/bin/env python3
"""Flatten the icon's layers into one master SVG at their rest frame.

Reads .claude/skills/super-voice-icon/animation.json and its layers, and
writes brand/icon/master/super-voice-icon.svg: every layer at frame 0 with
no scroll offset, stacked as nested SVGs and clipped to the 512 rounded
square (radius 112). Every platform's icon files are built from this file.
"""
import json
import re
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
SKILL = REPO / ".claude" / "skills" / "super-voice-icon"
OUT = Path(__file__).resolve().parent / "super-voice-icon.svg"


def main() -> None:
    spec = json.loads((SKILL / "animation.json").read_text())
    parts = []
    for layer in spec["layers"]:
        rel = layer.get("file") or layer["frames"].replace("{nn}", "00")
        text = re.sub(r"<\?xml[^>]*>", "", (SKILL / rel).read_text()).strip()
        vx, vy, vw, vh = layer["viewBox"]
        text = re.sub(
            r"<svg\b[^>]*>",
            f'<svg x="{vx}" y="{vy}" width="{vw}" height="{vh}" '
            f'viewBox="{vx} {vy} {vw} {vh}" overflow="hidden">',
            text, count=1)
        parts.append(f"<!-- {layer['id']} -->\n{text}")
    OUT.write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
        'viewBox="0 0 512 512" width="512" height="512">\n'
        '<defs><clipPath id="sv-master-tile"><rect width="512" height="512" rx="112"/></clipPath></defs>\n'
        '<g clip-path="url(#sv-master-tile)">\n' + "\n".join(parts) + "\n</g>\n</svg>\n")
    print(f"wrote {OUT.relative_to(REPO)}")


if __name__ == "__main__":
    main()
