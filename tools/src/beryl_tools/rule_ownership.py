"""Check which kernel and rules modules implement each Comprehensive Rule."""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass
from pathlib import Path

from . import card_names, citations, paths, runlog

SOURCE_ROOTS = ("rust/crates/beryl_kernel/src", "rust/crates/beryl_rules/src")
ALLOWLIST = "data/rule-ownership-allowlist.json"
HEADER = "//! Implements:"
SEE: re.Pattern[str] = re.compile(r"\bsee[\s(]*$", re.IGNORECASE)


@dataclass(frozen=True)
class Declaration:
    line: int
    ranges: dict[str, set[str]]


def source_files(root: Path) -> list[Path]:
    """Paths relative to the checkout, excluding test modules."""
    files = (
        path.relative_to(root)
        for directory in SOURCE_ROOTS
        for path in sorted((root / directory).rglob("*.rs"))
    )
    return [
        path for path in files if path.name != "tests.rs" and "tests" not in path.parts
    ]


def covers(parent: str, number: str) -> bool:
    return (
        number == parent
        or number.startswith(parent + ".")
        or (
            parent[-1].isdigit()
            and number.startswith(parent)
            and number[len(parent) :].isalpha()
        )
    )


def declare(
    path: Path, lines: list[str], numbers: set[str], problems: list[str]
) -> Declaration | None:
    markers = []
    for index, line in enumerate(lines, 1):
        if not line.startswith("//!") and line.strip():
            break
        if line.startswith(HEADER):
            markers.append((index, line.removeprefix(HEADER).strip()))
    if not markers:
        return None
    line, value = markers[0]
    where = f"{path.as_posix()}:{line}"
    if len(markers) > 1:
        problems.append(f"{where}: write one Implements declaration")
    if re.fullmatch(r"none\.\s+\S.*[.!?]", value):
        return Declaration(line, {})
    ranges: dict[str, set[str]] = {}
    for entry in value.split(","):
        match = re.fullmatch(rf"CR ({citations.NUMBER})", entry.strip())
        if match is None:
            problems.append(f"{where}: expected CR number(s) or 'none. <reason>.'")
            break
        number = match[1]
        if number not in numbers or len(number) < 3:
            problems.append(f"{where}: no such rule CR {number}")
            continue
        if number in ranges:
            problems.append(f"{where}: repeats CR {number}")
        ranges[number] = {child for child in numbers if covers(number, child)}
    return Declaration(line, ranges)


def parent_modules(path: Path) -> list[Path]:
    """The file, then its enclosing Rust modules from innermost to outermost."""
    modules = [path]
    for directory in path.parents:
        if directory.as_posix() in SOURCE_ROOTS:
            break
        modules.extend((directory / "mod.rs", directory.with_suffix(".rs")))
    return modules


def cross_reference(lines: list[str], citation: citations.Citation) -> bool:
    line = lines[citation.line]
    start = citation.number_span[0] - len("CR ")
    if citation.kind in {"link", "reference"}:
        start -= 1  # The opening bracket of the link.
    prefix = line[:start]
    if SEE.search(prefix):
        return True
    comment = citations.comment_text(prefix)
    if citation.line and comment is not None and not comment.strip(" ("):
        previous = citations.comment_text(lines[citation.line - 1])
        return previous is not None and SEE.search(previous) is not None
    return False


def declarations(
    sources: dict[Path, list[str]], rules: dict[str, str], problems: list[str]
) -> tuple[dict[Path, Declaration], dict[str, Path]]:
    numbers = {citations.number_of(anchor) for anchor in rules}
    declared: dict[Path, Declaration] = {}
    owners: dict[str, Path] = {}
    for path, lines in sources.items():
        declaration = declare(path, lines, numbers, problems)
        if declaration is None:
            continue
        declared[path] = declaration
        for number, covered in declaration.ranges.items():
            conflicts = {
                owners[child]
                for child in covered
                if child in owners and owners[child] != path
            }
            for owner in sorted(conflicts):
                problems.append(
                    f"{path.as_posix()}:{declaration.line}: CR {number} overlaps "
                    f"the declaration in {owner.as_posix()}"
                )
            for child in covered:
                owners.setdefault(child, path)
    return declared, owners


def read_sources(root: Path) -> dict[Path, list[str]]:
    return {
        path: (root / path).read_text(encoding="utf-8").splitlines()
        for path in source_files(root)
    }


def check(root: Path, rules: dict[str, str]) -> list[str]:
    """Audit declarations and implementation citations throughout both crates."""
    allowlist_path = root / ALLOWLIST
    allowlist: set[str] = set(json.loads(allowlist_path.read_text(encoding="utf-8")))
    sources = read_sources(root)
    problems: list[str] = []
    declared, _ = declarations(sources, rules, problems)
    used: dict[Path, set[str]] = {path: set() for path in declared}
    stale = allowlist.copy()
    for path, lines in sources.items():
        rel = path.as_posix()
        if rel in allowlist and path not in declared:
            # A directory can start its pass while another pass still owns a
            # file being moved out of it.
            stale.remove(rel)
            continue
        owners = [owner for owner in parent_modules(path) if owner in declared]
        if not owners:
            problems.append(f"{rel}: missing an Implements declaration")
            continue
        if declaration := declared.get(path):
            lines[declaration.line - 1] = ""
        text = "\n".join(lines)
        if "#[cfg(test)]" in text:
            _, skeleton = card_names.lex_c_like(text, rust=True)
            text = card_names.blank_test_items(text, skeleton)
        production = text.splitlines()
        for citation in citations.citations_in(production):
            if citation.kind == "definition" or cross_reference(production, citation):
                continue
            number = citation.number
            matches = [
                (owner, rule)
                for owner in owners
                for rule, covered in declared[owner].ranges.items()
                if number in covered
            ]
            if not matches:
                problems.append(
                    f"{rel}:{citation.line + 1}: CR {number} is outside the module's "
                    "Implements declaration; use 'see' for a cross-reference"
                )
            for owner, rule in matches:
                used[owner].add(rule)
    for path, declaration in declared.items():
        for number in sorted(declaration.ranges.keys() - used[path]):
            problems.append(
                f"{path.as_posix()}:{declaration.line}: declares CR {number} "
                "without an implementation citation"
            )
    problems.extend(f"{ALLOWLIST}: remove {rel}" for rel in sorted(stale))
    return problems


def coverage(
    root: Path, rules: dict[str, str]
) -> tuple[dict[str, list[str]], list[str]]:
    """Return unowned rules in the release's order, grouped by section."""
    problems: list[str] = []
    _, owners = declarations(read_sources(root), rules, problems)
    sections: dict[str, list[str]] = {}
    for anchor in rules:
        number = citations.number_of(anchor)
        if len(number) >= 3 and number not in owners:
            sections.setdefault(number.split(".")[0], []).append(number)
    return sections, problems


def run_coverage(_args: argparse.Namespace) -> int:
    version = citations.pinned(paths.REPO)
    report, problems = coverage(paths.REPO, citations.load_rules(version))
    if problems:
        for problem in problems:
            print(f"rule-ownership: {problem}")
        return 1
    for section, rules in report.items():
        print(f"CR {section}: {', '.join(rules)}")
    out = paths.cache_root() / "rule-coverage.json"
    data = {"effective": version.effective, "unowned": report}
    out.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
    return 0


def step(*_args: str) -> int:
    rules, missing = citations.step_rules(citations.pinned(paths.REPO), fetch=False)
    if rules is None:
        print(f"rule-ownership: skipped: {missing}")
        runlog.result("rule-ownership", "skip", 0.0)
        return 0
    problems = check(paths.REPO, rules)
    for problem in problems:
        print(f"rule-ownership: {problem}")
    return 1 if problems else 0
