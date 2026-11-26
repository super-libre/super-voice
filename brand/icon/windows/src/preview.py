#!/usr/bin/env python3
"""Render brand/icon/windows/preview.png from the built assets (run by build.py)."""
import base64
import json
import os
import subprocess
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.dirname(HERE)
A = os.path.join(OUT, "msix", "Assets")
ICO_SIZES = [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256]


def uri(path):
    return "data:image/png;base64," + base64.b64encode(open(path, "rb").read()).decode()


def app(n):
    return uri(os.path.join(OUT, "png", f"super-voice-{n}.png"))


def tray(theme, n):
    return uri(os.path.join(OUT, "tray", f"tray-{theme}-taskbar-{n}.png"))


LIGHT = dict(bg="#f3f3f3", panel="#fbfbfb", fg="#1b1b1b", muted="#5f5f5f", ghost="#d4d4d4", bar="#eeeeee")
DARK = dict(bg="#202020", panel="#2c2c2c", fg="#ffffff", muted="#a8a8a8", ghost="#4a4a4a", bar="#1c1c1c")


def ghost(px, c):
    r = round(px * 0.22)
    return f'<span style="display:inline-block;width:{px}px;height:{px}px;border-radius:{r}px;background:{c}"></span>'


def taskbar(th, theme, scale):
    s = scale
    icon = 24 * s
    h = 48 * s
    cell = 40 * s
    items = []
    for i in range(5):
        if i == 2:
            items.append(
                f'<span style="position:relative;display:inline-flex;align-items:center;justify-content:center;'
                f'width:{cell}px;height:{cell}px;border-radius:{4*s}px;background:{th["ghost"]}55">'
                f'<img src="{app(icon)}" width={icon} height={icon}>'
                f'<span style="position:absolute;bottom:{1*s}px;left:50%;transform:translateX(-50%);width:{16*s}px;'
                f'height:{3*s}px;border-radius:{2*s}px;background:#0067c0"></span></span>'
            )
        else:
            items.append(
                f'<span style="display:inline-flex;align-items:center;justify-content:center;width:{cell}px;height:{cell}px">'
                f'{ghost(icon, th["ghost"])}</span>'
            )
    t = 16 * s
    tray_img = f'<img src="{tray(theme, t)}" width={t} height={t}>' if t in (16, 32) else ""
    return (
        f'<div style="display:flex;align-items:center;height:{h}px;background:{th["bar"]};'
        f'border-top:1px solid {th["ghost"]};padding:0 {12*s}px;gap:{4*s}px;font:{12*s}px Segoe UI,sans-serif;color:{th["fg"]}">'
        f'<span style="flex:1"></span>{"".join(items)}<span style="flex:1"></span>'
        f'<span style="display:inline-flex;gap:{8*s}px;align-items:center">'
        f'{ghost(t, th["ghost"])}{tray_img}{ghost(t, th["ghost"])}'
        f'<span style="text-align:right;line-height:1.25">10:42<br>26/11/2026</span></span></div>'
    )


def start(th, scale):
    s = scale
    icon = 32 * s
    names = ["Mail", "Photos", "Super Voice", "Settings", "Files", "Music"]
    cells = []
    for i, nm in enumerate(names):
        img = f'<img src="{app(icon)}" width={icon} height={icon}>' if nm == "Super Voice" else ghost(icon, th["ghost"])
        cells.append(
            f'<div style="width:{96*s}px;height:{84*s}px;display:flex;flex-direction:column;align-items:center;'
            f'justify-content:center;gap:{8*s}px;font:{12*s}px Segoe UI,sans-serif;color:{th["fg"]}">{img}<span>{nm}</span></div>'
        )
    return (
        f'<div style="background:{th["panel"]};border:1px solid {th["ghost"]};border-radius:{8*s}px;padding:{16*s}px;'
        f'display:inline-block"><div style="font:600 {14*s}px Segoe UI,sans-serif;color:{th["fg"]};margin-bottom:{8*s}px">Pinned</div>'
        f'<div style="display:grid;grid-template-columns:repeat(6,{96*s}px)">{"".join(cells)}</div></div>'
    )


def section(title, body):
    return f'<h2 style="font:600 18px Segoe UI,sans-serif;margin:28px 0 10px">{title}</h2>{body}'


def main():
    row = lambda th: (
        f'<div style="display:flex;align-items:flex-end;gap:18px;padding:16px;background:{th["bg"]};border-radius:8px;color:{th["muted"]};font:11px sans-serif">'
        + "".join(
            f'<div style="display:flex;flex-direction:column;align-items:center;gap:6px"><img src="{app(n)}" width={n} height={n}>{n}</div>'
            for n in ICO_SIZES
        )
        + "</div>"
    )
    zoom = lambda th: (
        f'<div style="display:flex;align-items:flex-end;gap:18px;padding:16px;background:{th["bg"]};border-radius:8px;color:{th["muted"]};font:11px sans-serif">'
        + "".join(
            f'<div style="display:flex;flex-direction:column;align-items:center;gap:6px"><img src="{app(n)}" width={n*6} height={n*6} style="image-rendering:pixelated">{n} px at 600%</div>'
            for n in (16, 20, 24, 32, 40)
        )
        + "</div>"
    )
    trays = "".join(
        f'<div style="display:flex;align-items:flex-end;gap:18px;padding:16px;background:{th["bar"]};border-radius:8px;color:{th["muted"]};font:11px sans-serif;margin-bottom:8px">'
        + "".join(
            f'<div style="display:flex;flex-direction:column;align-items:center;gap:6px"><img src="{tray(theme, n)}" width={n} height={n}>{n}</div>'
            for n in (16, 20, 24, 32)
        )
        + "".join(
            f'<div style="display:flex;flex-direction:column;align-items:center;gap:6px"><img src="{tray(theme, n)}" width={n*6} height={n*6} style="image-rendering:pixelated">{n} px at 600%</div>'
            for n in (16, 24, 32)
        )
        + f'<span style="margin-left:auto">{theme} taskbar</span></div>'
        for theme, th in (("light", LIGHT), ("dark", DARK))
    )
    tiles = (
        '<div style="display:flex;gap:6px;align-items:flex-start;flex-wrap:wrap;background:#0063b1;padding:12px;border-radius:8px">'
        + "".join(
            f'<img src="{uri(os.path.join(A, f))}" width={w} height={h} style="background:#0078d4">'
            for f, w, h in (
                ("SmallTile.scale-100.png", 71, 71),
                ("Square150x150Logo.scale-100.png", 150, 150),
                ("Wide310x150Logo.scale-100.png", 310, 150),
                ("LargeTile.scale-100.png", 310, 310),
                ("StoreLogo.scale-100.png", 50, 50),
                ("SplashScreen.scale-100.png", 310, 150),
            )
        )
        + '</div><p style="font:12px sans-serif;color:#555">Windows 10 tiles on an accent plate (tiles are transparent, so the plate is the accent colour; Windows 11 does not show tiles). Last: SplashScreen at 50%.</p>'
    )
    html = (
        '<html><body style="margin:0;padding:24px 32px;background:#ffffff;width:1500px;font-family:Segoe UI,sans-serif;color:#1b1b1b">'
        '<h1 style="font:700 26px Segoe UI,sans-serif;margin:0">Super Voice for Windows: icon preview</h1>'
        '<p style="font:13px sans-serif;color:#555;margin:6px 0 0">Every frame of super-voice.ico at 1:1. 16 to 32 px use the simplified glyph; 36 px and up use the master artwork.</p>'
        + section("super-voice.ico, every size, light and dark", row(LIGHT) + "<div style='height:8px'></div>" + row(DARK))
        + section("Small sizes, magnified", zoom(LIGHT))
        + section("Taskbar at 100% (24 px) and tray (16 px)", taskbar(LIGHT, "light", 1) + "<div style='height:8px'></div>" + taskbar(DARK, "dark", 1))
        + section("Taskbar at 200% (48 px) and tray (32 px)", taskbar(LIGHT, "light", 2) + "<div style='height:8px'></div>" + taskbar(DARK, "dark", 2))
        + section("Start pins at 100% (32 px) and 200% (64 px)",
                  f'<div style="display:flex;gap:16px;flex-wrap:wrap">{start(LIGHT,1)}{start(DARK,1)}</div><div style="height:12px"></div>'
                  f'<div style="display:flex;gap:16px;flex-wrap:wrap;background:#e9e9e9;padding:12px;border-radius:8px">{start(DARK,2)}</div>')
        + section("Notification-area icons", trays)
        + section("MSIX tiles and logos", tiles)
        + "</body></html>"
    )
    with tempfile.TemporaryDirectory() as d:
        jobs = os.path.join(d, "jobs.json")
        # Height is generous; the page is cropped to its content below.
        json.dump([dict(out=os.path.join(d, "p.png"), w=1564, h=3600, scale=1, html=html)], open(jobs, "w"))
        env = dict(os.environ)
        env["NODE_PATH"] = subprocess.check_output(["npm", "root", "-g"], text=True).strip()
        subprocess.check_call(["node", os.path.join(HERE, "render.js"), jobs], env=env)
        from PIL import Image

        im = Image.open(os.path.join(d, "p.png")).convert("RGBA")
        bg = Image.new("RGBA", im.size, (255, 255, 255, 255))
        bg.alpha_composite(im)
        # crop trailing white
        px = bg.convert("L").point(lambda v: 255 if v < 250 else 0)
        box = px.getbbox()
        bg = bg.crop((0, 0, im.width, min(im.height, box[3] + 32)))
        bg.convert("RGB").save(os.path.join(OUT, "preview.png"), optimize=True)
    print("wrote preview.png")


if __name__ == "__main__":
    main()
