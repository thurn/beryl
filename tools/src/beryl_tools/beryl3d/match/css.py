"""The web interface's computed CSS values, parsed into the structures the element records hold
(records.py), so that two values that look the same compare equal field by field, and written
back as short CSS-like text for a reader.

dump.mjs writes `box-shadow`, `filter`, `background-image` and `mask-image` as WebKit computes
them, and `normalized` turns them into:

- a shadow, of `box-shadow` or of a `drop-shadow` filter: `{"inset", "x", "y", "blur",
  "spread", "color"}`, lengths in points and the color `[r, g, b, a]`;
- an image layer: `{"kind": "url", "url"}`, or a gradient `{"kind", "repeating", "shape",
  "stops"}` with `kind` "linear", "radial" or "conic". A linear gradient's `shape` is its angle
  in degrees (`to right` is 90), a radial or conic one's is its first argument with its spaces
  collapsed, or null. Each stop is `{"color", "at"}`: `at` is null or a list of positions, a
  length in points as a number or a percentage as text such as "42%". A color hint (a position
  between two stops, without a color) is a stop whose color is null;
- a filter function: `{"fn": "drop-shadow", "shadow"}`, or `{"fn", "value"}` with the value a
  number (`brightness(80%)` is 0.8, `blur(4px)` is 4, `hue-rotate(90deg)` is 90) or, when it is
  none of those, its text.

Each of the four properties is "none" or a list of these.
"""

from __future__ import annotations

import re
from collections.abc import Mapping, Sequence

Value = object

# The properties dump.mjs writes as WebKit's text, which `normalized` parses.
PARSED = ("background-image", "box-shadow", "filter", "mask-image")

# `to <side>` as the angle CSS gives it (CSS Images 3, "linear-gradient() syntax").
SIDES: Mapping[str, float] = {
    "to top": 0.0,
    "to right": 90.0,
    "to bottom": 180.0,
    "to left": 270.0,
    "to top right": 45.0,
    "to right top": 45.0,
    "to bottom right": 135.0,
    "to right bottom": 135.0,
    "to bottom left": 225.0,
    "to left bottom": 225.0,
    "to top left": 315.0,
    "to left top": 315.0,
}
# A linear gradient without an angle runs from top to bottom.
DEFAULT_ANGLE = 180.0
COLOR: re.Pattern[str] = re.compile(
    r"(?:rgba?|color|hsla?|oklch|oklab|lab|lch)\([^()]*\)|transparent"
)
NUMBER: re.Pattern[str] = re.compile(r"^(-?[\d.]+(?:e[+-]?\d+)?)([a-z%]*)$")


def commas(text: str) -> list[str]:
    """`text` split at the commas that are not inside parentheses, each part trimmed."""
    out: list[str] = []
    depth = 0
    start = 0
    for i, character in enumerate(text):
        if character == "(":
            depth += 1
        elif character == ")":
            depth -= 1
        elif character == "," and depth == 0:
            out.append(text[start:i].strip())
            start = i + 1
    out.append(text[start:].strip())
    return [part for part in out if part]


def rounded(value: float, places: int = 2) -> float:
    return round(value, places) + 0.0


def color(text: str) -> list[float] | None:
    """A computed color as `[r, g, b, a]`, channels from 0 to 255 and the alpha from 0 to 1, as
    dump.mjs writes the colors it reads itself; None for a form WebKit does not compute to.
    """
    text = text.strip()
    if text == "transparent":
        return [0, 0, 0, 0]
    rgb = re.fullmatch(r"rgba?\(([^()]*)\)", text)
    if rgb:
        parts = [float(p) for p in re.split(r"[\s,/]+", rgb[1].strip()) if p]
        alpha = parts[3] if len(parts) > 3 else 1.0
        return [*(round(p) for p in parts[:3]), rounded(alpha, 3)]
    srgb = re.fullmatch(r"color\(srgb ([^()]*)\)", text)
    if srgb:
        parts = [float(p) for p in re.split(r"[\s/]+", srgb[1].strip()) if p]
        alpha = parts[3] if len(parts) > 3 else 1.0
        return [*(round(p * 255) for p in parts[:3]), rounded(alpha, 3)]
    return None


def split_color(text: str) -> tuple[list[float] | None, str]:
    """The color at the start or the end of `text`, and the rest of `text` around it."""
    found = COLOR.search(text)
    if found is None:
        return None, text.strip()
    rest = (text[: found.start()] + " " + text[found.end() :]).strip()
    return color(found[0]), " ".join(rest.split())


def length(text: str) -> float:
    """A computed length in points (WebKit computes every length to `px`)."""
    found = NUMBER.match(text)
    return rounded(float(found[1])) if found else 0.0


def shadow(text: str) -> dict[str, Value]:
    """One layer of `box-shadow`, or a `drop-shadow` filter's argument."""
    inset = bool(re.search(r"\binset\b", text))
    found, rest = split_color(re.sub(r"\binset\b", " ", text))
    x, y, blur, spread = ([length(t) for t in rest.split()] + [0.0] * 4)[:4]
    return {
        "inset": inset,
        "x": x,
        "y": y,
        "blur": blur,
        "spread": spread,
        "color": found if found is not None else [0, 0, 0, 1],
    }


def position(text: str) -> Value:
    """A gradient stop's position: a percentage as text, anything else as a length."""
    return text if text.endswith("%") else length(text)


def gradient_shape(kind: str, text: str | None) -> Value:
    if kind == "linear":
        if text is None:
            return DEFAULT_ANGLE
        if text in SIDES:
            return SIDES[text]
        found = NUMBER.match(text)
        if found and found[2] in ("deg", ""):
            return rounded(float(found[1]) % 360)
        if found and found[2] == "turn":
            return rounded(float(found[1]) * 360 % 360)
    return text


def image(text: str) -> dict[str, Value]:
    """One layer of `background-image` or `mask-image`."""
    found = re.fullmatch(r"([a-z-]+)\((.*)\)", text.strip(), re.S)
    if not found:
        return {"kind": "unknown", "text": text}
    function, inner = found[1].removeprefix("-webkit-"), found[2]
    if function == "url":
        return {"kind": "url", "url": inner.strip().strip("\"'")}
    repeating = function.startswith("repeating-")
    kind = function.removeprefix("repeating-").removesuffix("-gradient")
    parts = commas(inner)
    shape: str | None = None
    # The first argument is the gradient's shape unless it is a stop, which always has a color.
    if parts and split_color(parts[0])[0] is None:
        shape = " ".join(parts.pop(0).split())
    stops: list[dict[str, Value]] = []
    for part in parts:
        stop_color, rest = split_color(part)
        stops.append(
            {
                "color": stop_color,
                "at": [position(p) for p in rest.split()] if rest else None,
            }
        )
    return {
        "kind": kind,
        "repeating": repeating,
        "shape": gradient_shape(kind, shape),
        "stops": stops,
    }


def filter_value(text: str) -> Value:
    found = NUMBER.match(text.strip())
    if not found:
        return " ".join(text.split())
    number, unit = float(found[1]), found[2]
    return rounded(number / 100 if unit == "%" else number, 4)


def filters(text: str) -> list[dict[str, Value]]:
    """A `filter` list, each function in order."""
    out: list[dict[str, Value]] = []
    for found in re.finditer(r"([a-z-]+)\(((?:[^()]|\([^()]*\))*)\)", text):
        function, argument = found[1], found[2]
        if function == "drop-shadow":
            out.append({"fn": function, "shadow": shadow(argument)})
        else:
            out.append({"fn": function, "value": filter_value(argument)})
    return out


def parsed(name: str, text: str) -> Value:
    """`text`, the computed value of `name` (one of `PARSED`), as the records hold it."""
    if text.strip() in ("none", ""):
        return "none"
    if name == "box-shadow":
        return [shadow(part) for part in commas(text)]
    if name == "filter":
        return filters(text)
    # A layer of `none` draws nothing, as in `radial-gradient(...), none`.
    layers = [image(part) for part in commas(text) if part != "none"]
    return layers or "none"


def normalized(style: Mapping[str, Value]) -> dict[str, Value]:
    """`style` with each of `PARSED` that is still WebKit's text parsed."""
    out = dict(style)
    for name in PARSED:
        value = out.get(name)
        if isinstance(value, str):
            out[name] = parsed(name, value)
    return out


def number(value: float) -> str:
    return f"{value:.2f}".rstrip("0").rstrip(".") if value % 1 else f"{value:.0f}"


def color_text(value: Sequence[object]) -> str:
    r, g, b, a = (float(str(v)) for v in value)
    return f"rgba({number(r)},{number(g)},{number(b)},{number(a)})"


def text(value: Value, is_color: bool = False) -> str:
    """`value`, a record's style value, as short CSS-like text for a reader. `is_color` says
    that a list of four numbers is a color, as the values of `support.COLORS` are."""
    if isinstance(value, bool):
        return str(value).lower()
    if isinstance(value, (int, float)):
        return number(float(value))
    if isinstance(value, str):
        return value
    if value is None:
        return "null"
    if isinstance(value, Mapping):
        return mapping_text(value)
    if isinstance(value, Sequence):
        numbers = all(isinstance(v, (int, float)) for v in value)
        if is_color and numbers and len(value) == 4:
            return color_text(value)
        joiner = " " if numbers else ", "
        return joiner.join(text(v, is_color) for v in value) if value else "[]"
    return repr(value)


def mapping_text(value: Mapping[str, Value]) -> str:
    if "inset" in value:
        lengths = " ".join(text(value[k]) for k in ("x", "y", "blur", "spread"))
        inset = " inset" if value["inset"] else ""
        return f"{text(value['color'], True)} {lengths}{inset}"
    if value.get("kind") == "url":
        url = str(value.get("url"))
        return f"url({url[:40]}…)" if len(url) > 40 else f"url({url})"
    if "stops" in value:
        stops = value["stops"]
        assert isinstance(stops, list)
        shape = value.get("shape")
        parts = [] if shape is None else [text(shape)]
        if value.get("kind") == "linear" and isinstance(shape, (int, float)):
            parts = [f"{text(shape)}deg"]
        for stop in stops:
            assert isinstance(stop, Mapping)
            words = [] if stop.get("color") is None else [text(stop["color"], True)]
            at = stop.get("at")
            words += [text(p) for p in at] if isinstance(at, list) else []
            parts.append(" ".join(words))
        prefix = "repeating-" if value.get("repeating") else ""
        return f"{prefix}{value.get('kind')}-gradient({', '.join(parts)})"
    if "fn" in value:
        inner = value.get("shadow", value.get("value"))
        return f"{value['fn']}({text(inner)})"
    return ", ".join(f"{k}: {text(v)}" for k, v in value.items())
