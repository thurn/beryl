"""The card vocabulary's entries, read from beryl_card's source as text: each public enum
variant, function and constant with a doc comment, where it is, and what its doc comment says.

Nothing is built, so listing them is instant however the vocabulary changes. The reading is
structural, not a Rust parser: it tracks braces (outside strings and comments) to know which
enum or impl a line is in, which holds for rustfmt'd source.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

from . import nearest, paths

SOURCES: Path = paths.RUST / "crates" / "beryl_card" / "src"

ENUM: re.Pattern[str] = re.compile(r"^\s*pub enum (\w+)")
IMPL: re.Pattern[str] = re.compile(
    r"^\s*impl(?:<[^>]*>)?\s+(?:[\w:<>]+\s+for\s+)?(\w+)"
)
FUNCTION: re.Pattern[str] = re.compile(r"^\s*pub (?:const )?fn (\w+)")
CONSTANT: re.Pattern[str] = re.compile(r"^\s*pub const ([A-Z][A-Z0-9_]*)\s*:")
VARIANT: re.Pattern[str] = re.compile(r"^\s*([A-Z]\w*)\s*(?:[({,]|$)")
# String and character literals, whose braces and slashes are not code.
LITERAL: re.Pattern[str] = re.compile(r'"(?:[^"\\]|\\.)*"|\'(?:[^\'\\]|\\.)\'')
# A citation of the Comprehensive Rules with the rule it quotes, `([CR 701.6a](url)): "To
# counter ..."`, and one without: rules words, not what the card says.
QUOTED_RULE: re.Pattern[str] = re.compile(r'\(?\[CR [^\]]*\]\([^)]*\)\)?:\s*"[^"]*"')
CITATION: re.Pattern[str] = re.compile(r"\(?\[CR [^\]]*\]\([^)]*\)\)?")
CAMEL: re.Pattern[str] = re.compile(r"(?<=[a-z0-9])(?=[A-Z])|_")


@dataclass(frozen=True)
class Entry:
    # As a card file writes it: `Effect::Counter`, `Cost::tap`, `build::destroy`.
    name: str
    path: Path
    line: int
    doc: str

    def words(self) -> set[str]:
        """The words of what it says a card says: its doc comment less the rules it cites, and
        its name's words (`Effect::GainLife`: "gain life")."""
        said = CITATION.sub("", QUOTED_RULE.sub("", self.doc))
        spoken = " ".join(CAMEL.split(self.name.rsplit("::", 1)[-1]))
        return nearest.words(said + "\n" + spoken, stemmed=True)


def module(path: Path, root: Path) -> str:
    """The module a source file is: `effect` for effect/mod.rs, `build` for effect/build.rs."""
    part = path.relative_to(root)
    return part.parent.name if part.stem in ("mod", "lib") else part.stem


def code(line: str) -> str:
    """A line without its literals and comment, for counting braces."""
    return LITERAL.sub('""', line).split("//", 1)[0]


def entries_of(path: Path, root: Path) -> list[Entry]:
    """The entries in one source file, in order."""
    out: list[Entry] = []
    # The enums and impls open at this point: kind, name and the depth inside them.
    scopes: list[tuple[str, str, int]] = []
    depth = 0
    doc: list[str] = []
    for number, line in enumerate(path.read_text().splitlines(), start=1):
        stripped = line.strip()
        if stripped.startswith("///"):
            doc.append(stripped[3:].removeprefix(" "))
            continue
        if stripped.startswith("#["):
            continue
        scope = scopes[-1] if scopes else None
        name = None
        if doc:
            if (
                scope is not None
                and scope[0] == "enum"
                and depth == scope[2]
                and (variant := VARIANT.match(line))
            ):
                name = f"{scope[1]}::{variant.group(1)}"
            elif found := FUNCTION.match(line) or CONSTANT.match(line):
                owner = scope[1] if scope is not None else module(path, root)
                name = f"{owner}::{found.group(1)}"
        if name is not None:
            out.append(Entry(name, path, number, "\n".join(doc)))
        doc = []
        text = code(line)
        opened = depth + text.count("{") - text.count("}")
        if opened > depth:
            if enum := ENUM.match(line):
                scopes.append(("enum", enum.group(1), opened))
            elif impl := IMPL.match(line):
                scopes.append(("impl", impl.group(1), opened))
        depth = opened
        while scopes and depth < scopes[-1][2]:
            scopes.pop()
    return out


def entries(root: Path = SOURCES) -> list[Entry]:
    """Every entry of the vocabulary, file by file."""
    return [e for path in sorted(root.rglob("*.rs")) for e in entries_of(path, root)]


def matching(text: str, name: str, among: list[Entry], most: int = 10) -> list[Entry]:
    """The entries of `among` whose doc comments share most with card `name`'s rules text,
    nearest first, and in the source's order among equals."""
    mine = nearest.words(text, name, stemmed=True)
    scored = [(nearest.overlap(mine, e.words()), i, e) for i, e in enumerate(among)]
    best = sorted((s for s in scored if s[0] > 0), key=lambda s: (-s[0], s[1]))
    return [e for _, _, e in best[:most]]
