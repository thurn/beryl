"""The element records that `web.json` and `b3d.json` hold, and a check that a document of them
keeps the contract every `match` subcommand reads (docs/beryl-3d.md, "Matching the web
interface's look"):

    {"target", "scale", "elements": [{"key", "role", "name", "object", "box", "z", "parent",
                                      "style", "text"}]}

Beryl 3D's records also hold `place`, where on the table the element lies (`hand.yours`,
`stack`), or empty for the flat layer; the web's do not.

`box` is `[x, y, width, height]` in points, `z` the paint order, `parent` the index of the
parent element or null, `style` CSS property names to normalized values, and `text` a list of
lines `{"string", "family", "size", "weight", "color", "baseline", "box"}`. A color is
`[r, g, b, a]` with channels from 0 to 255 and the alpha from 0 to 1.
"""

from __future__ import annotations

from collections.abc import Mapping, Sequence

# The style properties both sides write, in the contract's order.
STYLE = (
    "background-color",
    "background-image",
    "border-width",
    "border-color",
    "border-radius",
    "box-shadow",
    "filter",
    "opacity",
    "color",
    "font-family",
    "font-size",
    "font-weight",
    "mask-image",
    "transform",
)
# Properties an element with no text of its own may leave out: Beryl 3D writes them only for
# the flat layer's elements, whose type it knows.
TYPE: frozenset[str] = frozenset({"color", "font-family", "font-size", "font-weight"})
ELEMENT = ("key", "role", "name", "object", "box", "z", "parent", "style", "text")
LINE = ("string", "family", "size", "weight", "color", "baseline", "box")


def number(value: object) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def is_box(value: object) -> bool:
    return isinstance(value, list) and len(value) == 4 and all(map(number, value))


def is_color(value: object) -> bool:
    if not (isinstance(value, list) and len(value) == 4 and all(map(number, value))):
        return False
    *channels, alpha = value
    return all(0 <= c <= 255 for c in channels) and 0 <= alpha <= 1


def style_faults(where: str, style: Mapping[str, object]) -> list[str]:
    faults = [f"{where}: unknown style {name!r}" for name in style if name not in STYLE]
    faults += [
        f"{where}: style lacks {name!r}"
        for name in STYLE
        if name not in style and name not in TYPE
    ]
    for name in ("background-color", "color"):
        value = style.get(name)
        if value is not None and not is_color(value):
            faults.append(f"{where}: {name} {value!r} is not [r, g, b, a]")
    for name in ("opacity", "font-size", "font-weight"):
        value = style.get(name)
        if value is not None and not number(value):
            faults.append(f"{where}: {name} {value!r} is not a number")
    return faults


def line_faults(where: str, line: object) -> list[str]:
    if not isinstance(line, Mapping):
        return [f"{where}: a line is not an object"]
    faults = [f"{where}: line lacks {key!r}" for key in LINE if key not in line]
    if not isinstance(line.get("string"), str):
        faults.append(f"{where}: a line's string is not text")
    if "box" in line and not is_box(line["box"]):
        faults.append(f"{where}: line box {line['box']!r} is not [x, y, width, height]")
    if line.get("color") is not None and not is_color(line["color"]):
        faults.append(f"{where}: line color {line['color']!r} is not [r, g, b, a]")
    if "baseline" in line and not number(line["baseline"]):
        faults.append(f"{where}: baseline {line['baseline']!r} is not a number")
    return faults


def faults(document: object) -> list[str]:
    """Each way `document` breaks the record contract, as one line each; none when it keeps
    it."""
    if not isinstance(document, Mapping):
        return ["the document is not an object"]
    out = [
        f"lacks {key!r}"
        for key in ("target", "scale", "elements")
        if key not in document
    ]
    elements = document.get("elements", [])
    if not isinstance(elements, Sequence) or isinstance(elements, str):
        return [*out, "elements is not a list"]
    for index, element in enumerate(elements):
        where = f"element {index}"
        if not isinstance(element, Mapping):
            out.append(f"{where}: not an object")
            continue
        out += [f"{where}: lacks {key!r}" for key in ELEMENT if key not in element]
        if "box" in element and not is_box(element["box"]):
            out.append(f"{where}: box {element['box']!r} is not [x, y, width, height]")
        if "z" in element and not isinstance(element["z"], int):
            out.append(f"{where}: z {element['z']!r} is not a whole number")
        parent = element.get("parent")
        if parent is not None and not (isinstance(parent, int) and 0 <= parent < index):
            out.append(f"{where}: parent {parent!r} is not an earlier element")
        style = element.get("style")
        if isinstance(style, Mapping):
            out += style_faults(where, style)
        elif "style" in element:
            out.append(f"{where}: style is not an object")
        text = element.get("text")
        if isinstance(text, list):
            for line in text:
                out += line_faults(where, line)
        elif "text" in element:
            out.append(f"{where}: text is not a list")
    return out
