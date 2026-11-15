"""The modules and check bodies registered by ui/scripts/kept.mjs."""

from __future__ import annotations

import json
import re
from dataclasses import dataclass

from . import paths

KEPT_SCRIPT = "ui/scripts/kept.mjs"
KEPT_HARNESS = "ui/scripts/kept-harness.mjs"
# Kept modules register checks when imported for their side effects.
PART_IMPORT: re.Pattern[str] = re.compile(
    r'^\s*(?:await import\("\./([^" /]+\.mjs)"\)|import "\./([^" /]+\.mjs)");', re.M
)
COMMENT = ("//", "/*", "*")


def check_sources() -> dict[str, str]:
    """The entry point, its harness and the literal side-effect imports it registers."""
    entry = paths.REPO / KEPT_SCRIPT
    if not entry.is_file():
        return {}
    source = entry.read_text()
    files = [
        KEPT_HARNESS,
        *(f"ui/scripts/{m[1] or m[2]}" for m in PART_IMPORT.finditer(source)),
    ]
    return {
        KEPT_SCRIPT: source,
        **{
            f: (paths.REPO / f).read_text() for f in files if (paths.REPO / f).is_file()
        },
    }


@dataclass
class Region:
    """A `check(...)` call's lines in a kept file and the titles it can give: a pattern,
    None when its title is computed where this cannot follow, and the title itself when it is
    spelled out."""

    first: int
    last: int
    title: re.Pattern[str] | None
    literal: str | None = None


CALL: re.Pattern[str] = re.compile(r"^(\s*)(?:await\s+)?check\((.*)$")
TITLE: re.Pattern[str] = re.compile(
    r'^\s*(?:"((?:[^"\\]|\\.)*)"|`([^`]*)`|([A-Za-z_]\w*))'
)


def title_pattern(text: str, source: str) -> tuple[re.Pattern[str] | None, str | None]:
    """The titles a call's first argument `text` gives, and the title when it is one string: a
    string, a template whose `${...}` parts match anything, or a constant defined as either last
    in `source` (the file up to the call)."""
    m = TITLE.match(text)
    if not m:
        return None, None
    if text[m.end() :].lstrip().startswith("+"):
        # A concatenation: what follows the first part matches anything.
        first, _ = title_pattern(text[: m.end()], source)
        return (re.compile(first.pattern + ".*") if first else None), None
    if m[1] is not None:
        title = str(json.loads(f'"{m[1]}"'))
        return re.compile(re.escape(title)), title
    if m[2] is not None:
        parts = re.split(r"\$\{[^}]*\}", m[2])
        literal = m[2] if len(parts) == 1 else None
        return re.compile(".+".join(re.escape(p) for p in parts)), literal
    defined = re.findall(rf"^\s*const {m[3]}\s*=\s*([\"`].*)", source, re.M)
    return title_pattern(defined[-1], "") if defined else (None, None)


def regions(source: str) -> list[Region]:
    """The `check(...)` calls of a kept file, 1-based. Prettier formats the file, so a call
    closes on the first line at its own indentation that ends `);`."""
    lines = source.splitlines()
    found: list[Region] = []
    for n, line in enumerate(lines):
        m = CALL.match(line)
        # A zero-argument callback named check is not a test registration.
        if not m or m[2] == ");":
            continue
        indent = m[1]
        argument = m[2] or (lines[n + 1] if n + 1 < len(lines) else "")
        last = n
        if not line.rstrip().endswith(");"):
            for k in range(n + 1, len(lines)):
                text = lines[k]
                if (
                    text.startswith(indent)
                    and not text[len(indent) :].startswith((" ", "\t"))
                    and text.rstrip().endswith(");")
                ):
                    last = k
                    break
        above = "\n".join(lines[:n])
        found.append(Region(n + 1, last + 1, *title_pattern(argument, above)))
    return found


def statements(source: str) -> list[tuple[int, int]]:
    """The file's top-level statements, 1-based: each from a line at column 0 to the line before
    the next, comments joining the statement after them."""
    lines = source.splitlines()
    starts = [
        n
        for n, line in enumerate(lines)
        if line
        and not line[0].isspace()
        and line[0] not in "})]"
        and not (n and lines[n - 1].startswith(COMMENT))
    ]
    return [
        (s + 1, (starts[i + 1] if i + 1 < len(starts) else len(lines)))
        for i, s in enumerate(starts)
    ]


def kept_titles(source: str) -> tuple[list[str], list[re.Pattern[str]]]:
    """The checks a kept file has: the titles it spells out, and the patterns of all of
    them (a computed title it cannot follow matches anything)."""
    found = regions(source)
    patterns = [r.title if r.title is not None else re.compile(".*") for r in found]
    return [f"ui-test:{r.literal}" for r in found if r.literal is not None], patterns
