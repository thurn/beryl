"""The plot `just b3d match motion` writes for each pair of moving elements: its box's center x
and y, its scale and its opacity over the span, the web's and Beryl 3D's tracks overlaid, as an
SVG picture that plot.mjs turns into a PNG. The Beryl 3D line is dashed, so that where the two
agree both still show.

Scale is the element's width over the web element's width in the last frame it is on screen
(or Beryl 3D's, if the web's never is), so that both sides share one measure and a card drawn
larger in Beryl 3D shows as a higher line, not as the same one.
"""

from __future__ import annotations

from collections.abc import Callable, Sequence
from html import escape

from .tracks import Sample, Track

WIDTH = 720
PANEL = 150
MARGIN_LEFT = 70
MARGIN_RIGHT = 20
TOP = 46
GAP = 26
# The two sides' colors, and the page's: a light page, so that the PNG reads on any screen.
WEB = "#2563eb"
B3D = "#ea580c"
INK = "#1f2937"
RULE = "#d1d5db"
PAGE = "#ffffff"
OPACITY = "opacity"


def reference_width(web: Track, b3d: Track) -> float:
    for track in (web, b3d):
        for s in reversed(track):
            if s is not None and s.width > 0:
                return s.width
    return 1.0


def number(value: float) -> str:
    return f"{value:.3g}" if abs(value) < 10 else f"{value:.0f}"


def panel(
    top: float,
    title: str,
    times: Sequence[float],
    lines: Sequence[tuple[str, Sequence[float | None]]],
) -> list[str]:
    """One panel's SVG elements: a title, its value range on the left, and each side's line."""
    values = [v for _, line in lines for v in line if v is not None]
    low: float = min(values) if values else 0.0
    high: float = max(values) if values else 1.0
    if title == OPACITY:
        # Opacity is always drawn on its whole range, so that a flat line reads as its value.
        low, high = min(low, 0.0), max(high, 1.0)
    if high - low < 1e-6:
        low, high = low - 0.5, high + 0.5
    span: float = (times[-1] - times[0]) or 1.0
    right: float = WIDTH - MARGIN_RIGHT

    def x(t: float) -> float:
        return MARGIN_LEFT + (t - times[0]) / span * (right - MARGIN_LEFT)

    def y(v: float) -> float:
        return top + PANEL - (v - low) / (high - low) * PANEL

    out = [
        f'<rect x="{MARGIN_LEFT}" y="{top}" width="{right - MARGIN_LEFT}" height="{PANEL}" '
        f'fill="none" stroke="{RULE}"/>',
        f'<text x="{MARGIN_LEFT}" y="{top - 6}" class="title">{escape(title)}</text>',
        f'<text x="{MARGIN_LEFT - 6}" y="{top + 10}" class="axis" text-anchor="end">'
        f"{number(high)}</text>",
        f'<text x="{MARGIN_LEFT - 6}" y="{top + PANEL}" class="axis" text-anchor="end">'
        f"{number(low)}</text>",
    ]
    for (color, line), dashes in zip(lines, ("", ' stroke-dasharray="6 4"')):
        # A side's line breaks where its element is not on screen.
        runs: list[list[str]] = [[]]
        for t, v in zip(times, line):
            if v is None:
                runs.append([])
            else:
                runs[-1].append(f"{x(t):.1f},{y(v):.1f}")
        for run in runs:
            if len(run) == 1:
                cx, cy = run[0].split(",")
                out.append(f'<circle cx="{cx}" cy="{cy}" r="2" fill="{color}"/>')
            elif run:
                out.append(
                    f'<polyline points="{" ".join(run)}" fill="none" stroke="{color}" '
                    f'stroke-width="2"{dashes}/>'
                )
    return out


def svg(title: str, times: Sequence[float], web: Track, b3d: Track) -> str:
    """The plot of one pair, `times` in ms from the span's start."""
    reference = reference_width(web, b3d)
    measures: list[tuple[str, Callable[[Sample], float]]] = [
        ("x of the center, points", lambda s: s.center()[0]),
        ("y of the center, points", lambda s: s.center()[1]),
        ("scale (width / the web's last width)", lambda s: s.width / reference),
        (OPACITY, lambda s: s.opacity),
    ]
    height = TOP + len(measures) * (PANEL + GAP) + 24
    body: list[str] = [
        f'<text x="{MARGIN_LEFT}" y="20" class="head">{escape(title)}</text>',
        f'<text x="{WIDTH - MARGIN_RIGHT}" y="20" class="axis" text-anchor="end">'
        f'<tspan fill="{WEB}">web</tspan>  <tspan fill="{B3D}">Beryl 3D (dashed)</tspan></text>',
    ]
    for n, (name, measure) in enumerate(measures):
        lines = [
            (color, [None if s is None else measure(s) for s in track])
            for color, track in ((WEB, web), (B3D, b3d))
        ]
        body += panel(TOP + 14 + n * (PANEL + GAP), name, times, lines)
    bottom = height - 8
    body += [
        f'<text x="{MARGIN_LEFT}" y="{bottom}" class="axis">{number(times[0])} ms</text>',
        f'<text x="{WIDTH - MARGIN_RIGHT}" y="{bottom}" class="axis" text-anchor="end">'
        f"{number(times[-1])} ms</text>",
    ]
    style = (
        f"text{{font-family:-apple-system,Helvetica,sans-serif;fill:{INK}}}"
        ".head{font-size:14px;font-weight:600}.title{font-size:12px}.axis{font-size:11px}"
    )
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" '
        f'viewBox="0 0 {WIDTH} {height}"><style>{style}</style>'
        f'<rect width="100%" height="100%" fill="{PAGE}"/>' + "".join(body) + "</svg>\n"
    )
