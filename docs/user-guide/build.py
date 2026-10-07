#!/usr/bin/env python3
"""Builds the user guide: assembles the HTML parts, draws the keyboards from
the app's own board definitions, and prints the result to PDF with Chrome.

Usage: docs/user-guide/build.py [output.pdf]
"""

import math
import os
import pathlib
import re
import shutil
import subprocess
import sys

try:
    import tomllib
except ModuleNotFoundError:
    # The board definitions are TOML, which Python reads from 3.11 on. The
    # Mac's own python3 is older, so hand over to a newer one if there is one.
    for name in ("python3.13", "python3.12", "python3.11"):
        newer = shutil.which(name) or next(
            (p for p in (f"/opt/homebrew/bin/{name}", f"/usr/local/bin/{name}") if os.path.exists(p)),
            None,
        )
        if newer:
            os.execv(newer, [newer, *sys.argv])
    sys.exit("build.py needs Python 3.11 or newer (for tomllib); none was found.")

HERE = pathlib.Path(__file__).parent
ROOT = HERE.parent.parent
BOARDS = ROOT / "crates/kc-boards/boards"
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

LEGENDS = {
    "ESC": "Esc", "TAB": "Tab", "RET": "Enter", "BSPC": "Bksp", "DEL": "Del", "SPACE": "Space",
    "LSHFT": "Shift", "RSHFT": "Shift", "LCTRL": "Ctrl", "LALT": "Alt", "LGUI": "Gui",
    "CAPS": "Caps", "EQUAL": "=", "MINUS": "-", "BSLH": "\\", "SEMI": ";", "SQT": "'",
    "COMMA": ",", "DOT": ".", "FSLH": "/", "LBKT": "[", "RBKT": "]", "GRAVE": "`",
}


def legend(name):
    if name in LEGENDS:
        return LEGENDS[name]
    if len(name) == 2 and name[0] == "N" and name[1].isdigit():
        return name[1]
    return name


def corners(key):
    w, h, x, y, rot, rx, ry = key
    pts = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
    if rot:
        a = math.radians(rot / 100)
        s, c = math.sin(a), math.cos(a)
        pts = [(rx + (px - rx) * c - (py - ry) * s, ry + (px - rx) * s + (py - ry) * c) for px, py in pts]
    return pts


def keyboard_svg(board_file, width=640, legends=True, highlight=(), colors=None, links=(), dim=()):
    """An SVG drawing of a board's default layout.

    highlight: key positions outlined in the accent color.
    colors: {position: fill} for lighting illustrations.
    links: lists of positions joined by a line, as combos are drawn.
    dim: positions drawn faded, as transparent keys are.
    """
    board = tomllib.loads((BOARDS / board_file).read_text())
    layout = next(l for l in board["layouts"] if l["id"] == board["default_layout"])
    keys = layout["keys"]
    names = board.get("starter_keys", [])
    all_pts = [p for k in keys for p in corners(k)]
    x0, y0 = min(p[0] for p in all_pts), min(p[1] for p in all_pts)
    x1, y1 = max(p[0] for p in all_pts), max(p[1] for p in all_pts)
    scale = width / (x1 - x0)
    height = (y1 - y0) * scale
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.0f} {height:.0f}" class="kbd-svg">']
    centers = []
    for i, key in enumerate(keys):
        w, h, x, y, rot, rx, ry = key
        inset = 4
        pts = corners((w - 2 * inset, h - 2 * inset, x + inset, y + inset, rot, rx, ry))
        sp = [((px - x0) * scale, (py - y0) * scale) for px, py in pts]
        cx = sum(p[0] for p in sp) / 4
        cy = sum(p[1] for p in sp) / 4
        centers.append((cx, cy))
        fill = (colors or {}).get(i, "#eef1f6")
        stroke, sw = ("#3b5bdb", 2.4) if i in highlight else ("#c3cad6", 1)
        opacity = ' opacity="0.45"' if i in dim else ""
        path = " ".join(f"{px:.1f},{py:.1f}" for px, py in sp)
        out.append(f'<polygon points="{path}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}" stroke-linejoin="round"{opacity}/>')
        name = names[i] if i < len(names) else ""
        if legends and name:
            text_fill = "#ffffff" if colors and i in colors and colors[i] not in ("#eef1f6",) else "#394150"
            size = max(6.5, scale * 24)
            label = legend(name).replace("&", "&amp;").replace("<", "&lt;")
            out.append(f'<text x="{cx:.1f}" y="{cy + size * 0.35:.1f}" font-size="{size:.1f}" text-anchor="middle" fill="{text_fill}"{opacity}>{label}</text>')
    for group in links:
        pts = " ".join(f"{centers[i][0]:.1f},{centers[i][1]:.1f}" for i in group)
        out.append(f'<polyline points="{pts}" fill="none" stroke="#3b5bdb" stroke-width="3" stroke-linecap="round"/>')
        for i in group:
            out.append(f'<circle cx="{centers[i][0]:.1f}" cy="{centers[i][1]:.1f}" r="4" fill="#3b5bdb"/>')
    out.append("</svg>")
    return "\n".join(out)


def table(match):
    """Marks a table's heading row so it repeats on every page the table
    spans, and keeps short tables in one piece."""
    attrs, rows = match.group(1), match.group(2)
    head = re.match(r"\s*(<tr>\s*<th.*?</tr>)", rows, flags=re.S)
    if head:
        rows = rows[head.end():]
        heading = head.group(1) if re.search(r"<th[^>]*>[^<]", head.group(1)) else ""
    else:
        heading = ""
    if rows.count("<tr") <= 6:
        attrs += ' style="break-inside: avoid"'
    thead = f"<thead>{heading}</thead>" if heading else ""
    return f"<table{attrs}>{thead}<tbody>{rows}</tbody></table>"


def main():
    out = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else HERE / "Keyboard Curator User Guide.pdf"
    body = "".join((HERE / name).read_text() for name in sorted(p.name for p in HERE.glob("part-*.html")))
    rainbow = ["#e5484d", "#f79a3e", "#e9c43f", "#46a758", "#3e8ed0", "#8e4ec6"]
    go60_rows = {i: rainbow[(i // 12) % 6] for i in range(48)}
    go60_rows.update({i: rainbow[4] for i in range(48, 54)})
    go60_rows.update({i: rainbow[5] for i in range(54, 60)})
    figures = {
        "IMPRINT": keyboard_svg("cyboard-imprint.toml"),
        "GO60": keyboard_svg("moergo-go60.toml"),
        "GO60_SMALL": keyboard_svg("moergo-go60.toml", width=420),
        "GO60_COMBO": keyboard_svg("moergo-go60.toml", highlight=(31, 32), links=[(31, 32)]),
        "GO60_HRM": keyboard_svg("moergo-go60.toml", highlight=tuple(i for i in range(60) if (i % 12 >= 6 and i < 48) or i in (51, 52, 53, 57, 58, 59))),
        "GO60_LIT": keyboard_svg("moergo-go60.toml", colors=go60_rows),
        "GO60_SELECT": keyboard_svg("moergo-go60.toml", width=420, highlight=(25,)),
        "GLOVE80": keyboard_svg("moergo-glove80.toml"),
        "DEFY": keyboard_svg("dygma-defy.toml"),
    }
    for name, svg in figures.items():
        body = body.replace("{{" + name + "}}", svg)
    body = re.sub(r"<table([^>]*)>(.*?)</table>", table, body, flags=re.S)
    html = (HERE / "template.html").read_text().replace("{{BODY}}", body)
    built = HERE / "guide.html"
    built.write_text(html)
    subprocess.run(
        [CHROME, "--headless=new", "--disable-gpu", "--no-pdf-header-footer",
         f"--print-to-pdf={out}", built.as_uri()],
        check=True, capture_output=True,
    )
    print(out)


if __name__ == "__main__":
    main()
