"""The card-names check: no card name in the kernel, rules, session, generic search or UI.

Generic code works from definitions and the protocol, never from which card it is looking at
. The check scans the code of every generic crate and the UI,
and compares each file's count of card names with `data/card-names-allowlist.json`, the
project's burn-down list: a count may only fall, and a file the list lacks may not appear. The
list's second section names the one-card mechanics by identifier, so their removal is tracked.
"""

from __future__ import annotations

import ast
import io
import json
import re
import sys
import tokenize
from collections import defaultdict
from collections.abc import Iterable
from pathlib import Path
from typing import cast

from . import card_files, cards, paths

ALLOWLIST: Path = paths.REPO / "data" / "card-names-allowlist.json"

# The generic code, as directories under the repository root. `expert` and `frozen` are the
# expert system and its frozen copy, which this project leaves alone.
SCOPE = (
    "rust/crates/beryl_card/src",
    "rust/crates/beryl_card_build/src",
    "rust/crates/beryl_kernel/src",
    "rust/crates/beryl_rules/src",
    "rust/crates/beryl_engine/src",
    "rust/crates/beryl_session/src",
    "rust/crates/beryl_scenario/src",
    "rust/crates/beryl_scenario_types/src",
    "rust/crates/beryl_search/src",
    "rust/crates/beryl_app/src",
    "ui/src",
    "tools/src/beryl_tools",
)
EXEMPT_DIRECTORIES = {
    "expert",
    "frozen",
    "overlay",
    "tests",
    "generated",
    "node_modules",
}
SUFFIXES = {".rs", ".ts", ".tsx", ".py"}
# Rust test modules living in their own files, and the TypeScript spelling of one.
TEST_FILE: re.Pattern[str] = re.compile(
    r"(^|[_.])tests?\.(rs|tsx?)$|_tests\.rs$|^test_.*\.py$"
)

# The basic land types, which are also the basic lands' names; the rules define them
# (CR 205.3i, 305.6), so generic code may use the words. A `names::ISLAND` still counts: it
# looks a card up by name.
BASIC_LAND_TYPES = {"Plains", "Island", "Swamp", "Mountain", "Forest"}

# A card's name is one or more words; a word has its apostrophes inside, as in Aang's.
WORD: re.Pattern[str] = re.compile(r"\w+(?:['’]\w+)*")
# A name given as a constant of `beryl_cards::names` or a deck of `beryl_cards::decks`.
CONSTANT_PATH: re.Pattern[str] = re.compile(r"\b(names|decks)::([A-Z][A-Z0-9_]*)\b")


def lex_c_like(source: str, rust: bool) -> tuple[str, str]:
    """`source` with each comment blanked (`code`), and with each string and character
    literal blanked too (`skeleton`); both keep every character's position and newlines.

    Covers Rust (nested block comments, raw strings) and TypeScript (template literals). A
    `'` is a lifetime in Rust unless a closing one follows within a few characters.
    """
    n = len(source)
    code = list(source)
    skeleton = list(source)

    def blank(chars: list[str], a: int, b: int) -> None:
        for i in range(a, b):
            if chars[i] != "\n":
                chars[i] = " "

    i = 0
    while i < n:
        c = source[i]
        nxt = source[i + 1] if i + 1 < n else ""
        if c == "/" and nxt == "/":
            end = source.find("\n", i)
            end = n if end < 0 else end
            blank(code, i, end)
            blank(skeleton, i, end)
            i = end
        elif c == "/" and nxt == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if source.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif source.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(code, i, j)
            blank(skeleton, i, j)
            i = j
        elif c == "r" and rust and (m := re.compile(r'r(#*)"').match(source, i)):
            # A raw string: r"...", r#"..."#; no escapes.
            if i and (source[i - 1].isalnum() or source[i - 1] == "_"):
                i += 1
                continue
            close = '"' + m.group(1)
            end = source.find(close, m.end())
            end = n if end < 0 else end + len(close)
            blank(skeleton, i, end)
            i = end
        elif c in '"`' or (c == "'" and not rust):
            # A quote in TypeScript ends at its line's end (JSX text holds stray apostrophes).
            j = i + 1
            while j < n and source[j] != c and (rust or c == "`" or source[j] != "\n"):
                j += 2 if source[j] == "\\" else 1
            j = min(j, n - 1)
            blank(skeleton, i, j + 1)
            i = j + 1
        elif c == "'" and rust:
            # A Rust character literal ('x', '\n', '"'), unless a lifetime ('a).
            m = re.compile(r"'(?:\\.[^']*|[^\\'])'").match(source, i)
            if m:
                blank(skeleton, i, m.end())
                i = m.end()
            else:
                i += 1
        else:
            i += 1
    return "".join(code), "".join(skeleton)


def blank_test_items(code: str, skeleton: str) -> str:
    """`code` without the items under `#[cfg(test)]` (a test module, a helper)."""
    chars = list(code)
    for m in re.finditer(r"#\[cfg\(test\)\]", skeleton):
        start = m.end()
        stop = len(skeleton)
        j = start
        while j < len(skeleton) and skeleton[j] not in "{;":
            j += 1
        if j < len(skeleton) and skeleton[j] == "{":
            depth = 0
            for k in range(j, len(skeleton)):
                depth += {"{": 1, "}": -1}.get(skeleton[k], 0)
                if depth == 0:
                    stop = k + 1
                    break
        elif j < len(skeleton):
            stop = j + 1
        for k in range(m.start(), stop):
            if chars[k] != "\n":
                chars[k] = " "
    return "".join(chars)


def python_code(source: str) -> str:
    """Python `source` without comments and docstrings, which cite cards as examples."""
    lines = source.splitlines(keepends=True)
    blanked: list[list[str]] = [list(line) for line in lines]

    def blank(row: int, col: int, end_row: int, end_col: int) -> None:
        for r in range(row, end_row + 1):
            a = col if r == row else 0
            b = end_col if r == end_row else len(blanked[r])
            for k in range(a, min(b, len(blanked[r]))):
                if blanked[r][k] != "\n":
                    blanked[r][k] = " "

    for t in tokenize.generate_tokens(io.StringIO(source).readline):
        if t.type == tokenize.COMMENT:
            blank(t.start[0] - 1, t.start[1], t.end[0] - 1, t.end[1])
    for node in ast.walk(ast.parse(source)):
        if isinstance(
            node, (ast.Module, ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)
        ):
            body = node.body
            first = body[0] if body else None
            if (
                isinstance(first, ast.Expr)
                and isinstance(first.value, ast.Constant)
                and isinstance(first.value.value, str)
            ):
                blank(
                    first.lineno - 1,
                    first.col_offset,
                    (first.end_lineno or first.lineno) - 1,
                    first.end_col_offset or 0,
                )
    return "".join("".join(row) for row in blanked)


def code_of(path: Path) -> str:
    """What of `path` counts: its code and string literals, not comments or test items."""
    source = path.read_text(errors="replace")
    if path.suffix == ".py":
        return python_code(source)
    code, skeleton = lex_c_like(source, rust=path.suffix == ".rs")
    return blank_test_items(code, skeleton) if path.suffix == ".rs" else code


def in_scope_files(root: Path) -> list[Path]:
    """The source files under the scope's directories, as paths under `root`, sorted."""
    found: list[Path] = []
    for directory in SCOPE:
        base = root / directory
        for path in sorted(base.rglob("*")) if base.is_dir() else []:
            parts = path.relative_to(base).parts
            if (
                path.is_file()
                and path.suffix in SUFFIXES
                and not EXEMPT_DIRECTORIES.intersection(parts[:-1])
                and not TEST_FILE.search(path.name)
            ):
                found.append(path)
    return found


class NameIndex:
    """The card names, found in a line by the word each begins with: a lookup per word of the
    line, not a scan for every name, so 50,000 names cost no more than 47."""

    def __init__(self, names: Iterable[str], constants: set[str]) -> None:
        self.by_first_word: dict[str, list[str]] = defaultdict(list)
        for name in names:
            first = WORD.search(name)
            if first:
                self.by_first_word[first.group(0)].append(name)
        for candidates in self.by_first_word.values():
            candidates.sort(key=len, reverse=True)
        self.constants = constants

    def in_line(self, line: str) -> list[str]:
        """The names a line holds, in order: each name at a word boundary, the longest first
        where several begin at one place (Jace, the Mind Sculptor over Jace)."""
        found: list[tuple[int, str]] = []
        for word in WORD.finditer(line):
            for name in self.by_first_word.get(word.group(0), ()):
                end = word.start() + len(name)
                if line.startswith(name, word.start()) and not (
                    end < len(line) and (line[end].isalnum() or line[end] == "_")
                ):
                    found.append((word.start(), name))
                    break
        for m in CONSTANT_PATH.finditer(line):
            if m.group(1) == "decks" or m.group(2) in self.constants:
                found.append((m.start(), m.group(0)))
        return [name for _, name in sorted(found)]


def card_names_in(directory: Path, index: NameIndex) -> list[str]:
    """Each place the files under `directory` name a card, as `path:line: name`."""
    found: list[str] = []
    for path in sorted(p for p in directory.rglob("*") if p.is_file()):
        for number, line in enumerate(path.read_text().splitlines(), 1):
            found += [f"{path}:{number}: {name}" for name in index.in_line(line)]
    return found


def all_card_names() -> set[str]:
    """The cards and tokens of the card data, but a token named by one word, such as Copy,
    which is no name in prose, and the basic land types."""
    names: set[str] = set()
    deck_cards, deck_tokens = cards.listed()
    tokens = {str(t["name"]) for t in deck_tokens}
    for entry in [*deck_cards, *deck_tokens]:
        names.add(str(entry["name"]))
        names.update(
            str(face["name"]) for face in cast(list[dict[str, object]], entry["faces"])
        )
    return {
        n for n in names if (" " in n or n not in tokens) and n not in BASIC_LAND_TYPES
    }


def name_constants() -> set[str]:
    """The identifiers of `beryl_cards::names`."""
    return set(card_files.constants())


def scan(root: Path, index: NameIndex) -> dict[str, list[tuple[int, str]]]:
    """Each in-scope file under `root` that names a card, by its path from `root`, with the
    line and name of each hit."""
    hits: dict[str, list[tuple[int, str]]] = {}
    for path in in_scope_files(root):
        file_hits = [
            (number, name)
            for number, line in enumerate(code_of(path).splitlines(), 1)
            for name in index.in_line(line)
        ]
        if file_hits:
            hits[path.relative_to(root).as_posix()] = file_hits
    return hits


# One-card mechanics by identifier, `Type::member`. A member exists when a `struct`, `enum`,
# `impl` or `trait` block of `Type` mentions it.
def rust_sources(root: Path) -> list[str]:
    """Every crate's Rust source with comments and literals blanked, tests included."""
    return [
        lex_c_like(p.read_text(errors="replace"), rust=True)[1]
        for p in sorted((root / "rust" / "crates").glob("*/src/**/*.rs"))
        if not EXEMPT_DIRECTORIES.intersection(p.parts[:-1])
    ]


def blocks_of(sources: list[str], type_name: str) -> list[str]:
    """The bodies of the items `struct`, `enum`, `impl` and `trait` of `type_name`."""
    header = re.compile(
        rf"\b(?:struct|enum|trait|impl(?:<[^>]*>)?(?:\s+\w+\s+for)?)\s+{re.escape(type_name)}\b[^{{;]*\{{"
    )
    bodies: list[str] = []
    for skeleton in sources:
        for m in header.finditer(skeleton):
            depth = 0
            for k in range(m.end() - 1, len(skeleton)):
                depth += {"{": 1, "}": -1}.get(skeleton[k], 0)
                if depth == 0:
                    bodies.append(skeleton[m.end() : k])
                    break
    return bodies


def missing_mechanics(listed: list[str], sources: list[str]) -> list[str]:
    """The listed `Type::member` identifiers that no longer exist."""
    missing = []
    for identifier in listed:
        type_name, _, member = identifier.partition("::")
        pattern = re.compile(rf"(?<!\w){re.escape(member)}(?!\w)")
        if not any(pattern.search(b) for b in blocks_of(sources, type_name)):
            missing.append(identifier)
    return missing


def load_allowlist(path: Path = ALLOWLIST) -> tuple[dict[str, int], list[str]]:
    """The allowlist's card-name counts by path, and its one-card mechanics."""
    data = json.loads(path.read_text())
    return {str(p): int(c) for p, c in data["names"].items()}, [
        str(m) for m in data["mechanics"]
    ]


def problems(
    hits: dict[str, list[tuple[int, str]]],
    allowed: dict[str, int],
    listed: list[str],
    sources: list[str],
) -> list[str]:
    """Why the allowlist and the code disagree, one line each; none when they agree."""
    found: list[str] = []
    for path, file_hits in sorted(hits.items()):
        count = len(file_hits)
        shown = "; ".join(f"{n}: {name}" for n, name in file_hits[:5])
        if path not in allowed:
            found.append(
                f"{path}: names {count} card(s) ({shown}) and is not on the allowlist: "
                "generic code names no card"
            )
        elif count > allowed[path]:
            found.append(
                f"{path}: {count} card names, up from {allowed[path]} on the allowlist "
                f"({shown}): generic code names no card"
            )
    for path, count in sorted(allowed.items()):
        have = len(hits.get(path, []))
        if have < count:
            found.append(
                f"{path}: {have} card names, below the allowlist's {count}: "
                + ("remove the entry" if have == 0 else f"lower it to {have}")
                + f" in {ALLOWLIST.relative_to(paths.REPO)}"
            )
    for identifier in missing_mechanics(listed, sources):
        found.append(
            f"{identifier}: gone from the code but still listed under mechanics: remove it "
            f"from {ALLOWLIST.relative_to(paths.REPO)}"
        )
    return found


def current_index() -> NameIndex:
    return NameIndex(all_card_names(), name_constants())


def check(
    root: Path, allowlist: Path, index: NameIndex, shrink: bool = False
) -> list[str]:
    """What is wrong with the code under `root` against the `allowlist` file. With `shrink`
    the file is first lowered to what the code has: only down, so a count that rose, or a new
    file, still fails."""
    allowed, listed = load_allowlist(allowlist)
    hits = scan(root, index)
    sources = rust_sources(root)
    if shrink:
        allowed = {
            p: min(c, len(hits[p])) for p, c in sorted(allowed.items()) if p in hits
        }
        listed = [m for m in listed if m not in missing_mechanics(listed, sources)]
        allowlist.write_text(
            json.dumps({"names": allowed, "mechanics": listed}, indent=2) + "\n"
        )
    return problems(hits, allowed, listed, sources)


def step(*args: str) -> int:
    """The `card-names` check; `--shrink` also lowers the allowlist to what the code has."""
    found = check(paths.REPO, ALLOWLIST, current_index(), "--shrink" in args)
    for line in found:
        print(f"card-names: {line}")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(step(*sys.argv[1:]))
