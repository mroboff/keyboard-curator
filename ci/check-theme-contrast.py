#!/usr/bin/env python3
"""Checks every theme's text colors against their backgrounds.

Reads the theme files under crates/kc-app/assets/themes and computes the
WCAG 2.1 contrast ratio for each pair of text and background the app draws:
the toolkit's foreground, muted, primary, secondary, popover, success,
warning and danger pairs, and the look's keycap, system key and layer key
legends. Body text should reach 4.5:1 and large or bold text and controls
3:1 (WCAG AA). The report lists every pair below 4.5 and fails on any pair
below 3.

Usage: ci/check-theme-contrast.py [--all]   (--all prints every pair)
"""

import json
import pathlib
import sys

THEMES = pathlib.Path(__file__).resolve().parent.parent / "crates/kc-app/assets/themes"

# (text, background, what it is) in the toolkit's color names.
TOOLKIT_PAIRS = [
    ("foreground", "background", "text on the window"),
    ("muted.foreground", "background", "help text on the window"),
    ("muted.foreground", "muted.background", "help text on a muted panel"),
    ("foreground", "muted.background", "text on a muted panel"),
    ("foreground", "secondary.background", "text on a secondary surface"),
    ("secondary.foreground", "secondary.background", "secondary button"),
    ("primary.foreground", "primary.background", "primary button"),
    ("accent.foreground", "accent.background", "accented item"),
    ("popover.foreground", "popover.background", "menu or popover"),
    ("success.foreground", "success.background", "success badge"),
    ("warning.foreground", "warning.background", "warning badge"),
    ("danger.foreground", "danger.background", "danger badge"),
]

# (text, background, what it is) in the look's color names.
LOOK_PAIRS = [
    ("key_text", "key", "keycap legend"),
    ("system_text", "system_key", "system key legend"),
    ("layer_text", "layer_key", "layer key legend"),
    ("accent_text", "plinth", "accent text on the plinth"),
]


def channel(value):
    value = value / 255
    return value / 12.92 if value <= 0.03928 else ((value + 0.055) / 1.055) ** 2.4


def parse(color):
    """`#RRGGBB` or `#RRGGBBAA`, as (r, g, b, alpha in 0..1)."""
    hex_digits = color.lstrip("#")
    if len(hex_digits) not in (6, 8):
        return None
    rgb = tuple(int(hex_digits[i : i + 2], 16) for i in (0, 2, 4))
    alpha = int(hex_digits[6:8], 16) / 255 if len(hex_digits) == 8 else 1.0
    return (*rgb, alpha)


def over(color, backdrop):
    """A translucent color composited over an opaque backdrop."""
    r, g, b, alpha = color
    return tuple(round(c * alpha + d * (1 - alpha)) for c, d in zip((r, g, b), backdrop))


def luminance(rgb):
    r, g, b = (channel(c) for c in rgb)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    la, lb = luminance(a), luminance(b)
    light, dark = max(la, lb), min(la, lb)
    return (light + 0.05) / (dark + 0.05)


def main():
    show_all = "--all" in sys.argv[1:]
    worst = 21.0
    failures = 0
    for path in sorted(THEMES.glob("*.json")):
        theme = json.loads(path.read_text())
        for mode in ("light", "dark"):
            if mode not in theme:
                continue
            colors = theme[mode]["toolkit"]["colors"]
            look = theme[mode]["look"]
            # Translucent surfaces sit on the window, and the window itself
            # on the desktop, which the glass themes let through: black or
            # white is the worst case for them.
            desktop = (0, 0, 0) if mode == "dark" else (255, 255, 255)
            window = over(parse(colors["background"]), desktop)
            pairs = [(colors.get(t), colors.get(b), what) for t, b, what in TOOLKIT_PAIRS]
            pairs += [(look.get(t), look.get(b), what) for t, b, what in LOOK_PAIRS]
            for text, background, what in pairs:
                if text is None or background is None:
                    continue
                t, b = parse(text), parse(background)
                if t is None or b is None:
                    continue
                b = over(b, window)
                t = over(t, b)
                ratio = contrast(t, b)
                worst = min(worst, ratio)
                grade = "AA" if ratio >= 4.5 else ("large" if ratio >= 3 else "FAIL")
                if show_all or ratio < 4.5:
                    print(f"{theme['name']:<12} {mode:<5} {ratio:5.2f}  {grade:<5} {what} ({text} on {background})")
                if ratio < 3:
                    failures += 1
    print(f"lowest ratio {worst:.2f}; {failures} pair(s) below 3:1")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
