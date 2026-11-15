import json
import re
from collections.abc import Mapping, Sequence
from pathlib import Path

import pytest

from beryl_tools import design_tokens

PROPERTY: re.Pattern[str] = re.compile(r"--([a-z0-9-]+):\s*(.*?);", re.S)


def properties(css: str) -> dict[str, str]:
    """Each custom property of `css` and its value, whitespace collapsed, comments dropped."""
    css = re.sub(r"/\*.*?\*/", "", css, flags=re.S)
    found: dict[str, str] = {}
    for name, value in PROPERTY.findall(css):
        value = " ".join(value.split()).replace("( ", "(").replace(" )", ")")
        found[name] = value.replace(", )", ")")
    return found


def test_committed_stylesheet_is_what_the_data_renders() -> None:
    assert not design_tokens.stale()


def test_every_property_keeps_its_value_and_order() -> None:
    data = design_tokens.load()
    rendered = properties(design_tokens.render(data))
    committed = properties(design_tokens.STYLESHEET.read_text())
    assert list(rendered.items()) == list(committed.items())
    assert list(rendered) == [t["name"] for t in data["tokens"]]


def write(tmp_path: Path, tokens: Sequence[Mapping[str, object]]) -> Path:
    path = tmp_path / "design-tokens.json"
    path.write_text(json.dumps({"tokens": tokens}))
    return path


def test_render_writes_notes_groups_and_breaks_long_values(tmp_path: Path) -> None:
    long = ", ".join(f"rgb(1 2 3 / {n}%) {n}%" for n in range(10, 60, 10))
    path = write(
        tmp_path,
        [
            {"name": "a", "kind": "color", "value": "#000000", "note": "First."},
            {"name": "b", "kind": "css", "value": f"linear-gradient({long})"},
            {"name": "c", "kind": "length", "value": "4px", "group": True},
        ],
    )
    css = design_tokens.render(design_tokens.load(path))
    assert css.startswith(design_tokens.HEADER)
    assert "  /* First. */\n  --a: #000000;\n" in css
    assert "  --b: linear-gradient(\n    rgb(1 2 3 / 10%) 10%,\n" in css
    assert "    rgb(1 2 3 / 50%) 50%\n  );\n\n  --c: 4px;\n" in css
    declarations = re.sub(r"/\*.*?\*/", "", css, flags=re.S)
    assert all(len(line) <= design_tokens.LINE for line in declarations.splitlines())


@pytest.mark.parametrize(
    "token",
    [
        {"name": "Bad", "kind": "color", "value": "#000000"},
        {"name": "a", "kind": "colour", "value": "#000000"},
    ],
)
def test_load_rejects_bad_names_and_kinds(
    tmp_path: Path, token: dict[str, str]
) -> None:
    with pytest.raises(ValueError):
        design_tokens.load(write(tmp_path, [token]))


def test_load_rejects_a_repeated_name(tmp_path: Path) -> None:
    token = {"name": "a", "kind": "color", "value": "#000000"}
    with pytest.raises(ValueError):
        design_tokens.load(write(tmp_path, [token, token]))
