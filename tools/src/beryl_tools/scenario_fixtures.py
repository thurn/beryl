"""Keep focused tests independent of the card files' hand-written scenarios."""

from __future__ import annotations

import re
from collections.abc import Iterable
from pathlib import Path

from .card_names import lex_c_like

ADVICE = (
    "selects a card-file scenario; give the test its own Scenario constants "
    "or use rust/crates/beryl_cards/src/library/"
)
SELECTORS: set[str] = {
    "filter",
    "filter_map",
    "find",
    "find_map",
    "rfind",
    "position",
    "rposition",
    "get",
    "get_unchecked",
    "first",
    "last",
    "split_first",
    "split_last",
    "nth",
    "nth_back",
    "next",
    "next_back",
    "skip",
    "take",
    "skip_while",
    "take_while",
    "index",
}
IDENTIFIER: re.Pattern[str] = re.compile(r"[A-Za-z_]\w*")
CALL: re.Pattern[str] = re.compile(r"\s*\(\s*\)")
METHOD: re.Pattern[str] = re.compile(r"\s*\.\s*(\w+)(?:\s*::\s*<[^>]+>)?\s*")
INDEX: re.Pattern[str] = re.compile(r"\s*\[")
LET: re.Pattern[str] = re.compile(r"\blet\s+([^;={}]+?)(?::[^;=]+)?\s*=\s*[&*]*\s*$")
FOR: re.Pattern[str] = re.compile(r"\bfor\s+([^;={}]+?)\s+in\s+[&*]*\s*$")
CARD_QUERY: re.Pattern[str] = re.compile(
    r"\b(?:beryl_cards|crate)\s*::\s*written_scenarios_for\s*\(\s*"
)


def delimiters(code: str) -> dict[int, int]:
    """Map opening delimiters to their ends in code with literals already blanked."""
    opened: list[tuple[str, int]] = []
    matched: dict[int, int] = {}
    close = {")": "(", "]": "[", "}": "{"}
    for token in re.finditer(r"[()\[\]{}]", code):
        char = token[0]
        if char in "([{":
            opened.append((char, token.start()))
        elif opened and opened[-1][0] == close[char]:
            _, start = opened.pop()
            matched[start] = token.end()
    return matched


def selection(code: str, at: int, ends: dict[int, int]) -> tuple[int, bool]:
    """Follow an expression's method chain and indexing from the registry or a local."""
    while method := METHOD.match(code, at):
        name = method[1]
        at = method.end()
        if name.isdecimal() or (name == "name" and code.startswith(("==", "!="), at)):
            return at, True
        if code[at : at + 1] != "(" or at not in ends:
            break
        at = ends[at]
        if name in SELECTORS:
            return at, True
    return at, INDEX.match(code, at) is not None


def calls(code: str, registry: str) -> list[tuple[int, int]]:
    """Calls through the crate path or an ordinary Rust `use` import."""
    aliases = set()
    path = rf"\b{registry}\s*::\s*"
    for statement in re.finditer(rf"\buse\s+{path}([^;]+);", code):
        for imported in re.finditer(r"\bscenarios\b(?:\s+as\s+(\w+))?", statement[1]):
            aliases.add(imported[1] or "scenarios")
    names = [path + r"scenarios\b", *(rf"(?<![\w:.])\b{a}\b" for a in sorted(aliases))]
    found = []
    for name in re.finditer("|".join(names), code):
        if call := CALL.match(code, name.end()):
            found.append((name.start(), call.end()))
    return found


def borrowed_lines(source: str, in_registry: bool) -> list[int]:
    """Locations of selection from the registry and from its locally bound slices."""
    if "scenarios" not in source:
        return []
    code, skeleton = lex_c_like(source, rust=True)
    registry = r"(?:beryl_cards|crate)" if in_registry else "beryl_cards"
    uses = calls(skeleton, registry)
    if not uses and "written_scenarios_for" not in skeleton:
        return []
    ends = delimiters(skeleton)
    braces = [(a, b) for a, b in ends.items() if skeleton[a] == "{"]
    found = set()
    seen = set()
    while uses:
        at, after = uses.pop()
        if at in seen:
            continue
        seen.add(at)
        tail, selected = selection(skeleton, after, ends)
        if selected:
            found.add(at)
            continue
        # A local binding can carry the corpus, one card's cases, or a case in a loop.
        start = max(skeleton.rfind(mark, 0, at) for mark in ";{}") + 1
        before = skeleton[start:at]
        scope = min((b for a, b in braces if a < at < b), default=len(skeleton))
        if binding := LET.search(before):
            if "[" in binding[1]:
                found.add(at)
                continue
            scope_start = skeleton.find(";", tail, scope) + 1
            if scope_start == 0:
                continue
        elif binding := FOR.search(before):
            body = re.match(r"\s*\{", skeleton[tail:])
            if not body:
                continue
            scope_start = tail + body.end()
            scope = ends.get(scope_start - 1, scope)
        else:
            continue
        for local in IDENTIFIER.findall(binding[1]):
            if local in {"let", "mut", "ref", "_"}:
                continue
            for use in re.finditer(
                rf"(?<![\w.:])\b{local}\b", skeleton[scope_start:scope]
            ):
                uses.append((scope_start + use.start(), scope_start + use.end()))
    # A command can query by the card selected at runtime. A literal or card-name
    # constant in that query would make a caller depend on the card file.
    for query in CARD_QUERY.finditer(code):
        if code[query.end() :].startswith(('"', 'r"', "r#")) or re.match(
            r"(?:\w+\s*::\s*)*[A-Z][A-Z0-9_]+\s*\)", code[query.end() :]
        ):
            found.add(query.start())
    return sorted({source.count("\n", 0, at) + 1 for at in found})


def check(root: Path, files: Iterable[str]) -> list[str]:
    """Check Rust sources outside the card files that own the scenarios."""
    found = []
    for rel in sorted(files):
        if not rel.endswith(".rs") or rel.startswith("rust/cards/"):
            continue
        path = root / rel
        if path.is_file():
            in_registry = rel.startswith("rust/crates/beryl_cards/src/")
            for line in borrowed_lines(path.read_text(), in_registry):
                found.append(f"{rel}:{line}: {ADVICE}")
    return found
