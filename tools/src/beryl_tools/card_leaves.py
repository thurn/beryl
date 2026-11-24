"""The card-leaves check: only leaf crates depend on the card crates.

Cargo rebuilds every crate that depends on a changed crate. `beryl_cards` depends on every
set crate (`cards_1993`, ...), so a crate that depends on `beryl_cards` is rebuilt, and its
tests are relinked, by an edit to any card file. A card edit must rebuild only the leaves
that need the cards: the apps, the benchmark, the command line and the client. Everything else
(the kernel, the rules, the search, the sessions) is generic and is tested against cards
through the leaves' own `tests/` directories or as a dev-dependency, which Cargo does not
rebuild a library for.

The check fails when
- a crate outside `data/card-leaves-allowlist.json` lists `beryl_cards` or a `cards_*` set
  crate under `[dependencies]` or `[build-dependencies]` (a dev-dependency is fine);
- a Rust file under such a crate's `src/` names `beryl_cards` or a set crate outside a
  comment or a string literal, because a library cannot name a crate it does not depend on
  and a dev-dependency is visible to `src/` unit tests, which would put the card edit back
  into that crate's rebuild; and
- the allowlist names a crate that does not need it any more, so its count only goes down.
"""

from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path

from . import paths

ALLOWLIST = "data/card-leaves-allowlist.json"
MANIFEST_ROOTS = ("engine/crates", "client/crates")
NORMAL_TABLES = ("dependencies", "build-dependencies")
CARD_CRATE: re.Pattern[str] = re.compile(r"^(beryl_cards|cards_\d\w*)$")
NAMED: re.Pattern[str] = re.compile(r"\b(beryl_cards|cards_\d\w*)\b")
# A string literal, or the start of one that a trailing backslash continues on the next line.
STRING: re.Pattern[str] = re.compile(r'"(?:[^"\\]|\\.)*\\?(?:"|$)')
WHY = (
    "Cargo rebuilds every dependent of a changed crate, so a card edit must rebuild only "
    "the leaf crates; test against cards from a `tests/` directory or a dev-dependency "
    f"instead, or add the crate to {ALLOWLIST} if it is a new leaf"
)


def normal_card_dependencies(manifest: dict[str, object]) -> list[str]:
    """The card crates a parsed Cargo.toml depends on outside `[dev-dependencies]`,
    including its `[target.*]` tables."""
    tables: list[object] = [manifest.get(name) for name in NORMAL_TABLES]
    targets = manifest.get("target")
    if isinstance(targets, dict):
        for target in targets.values():
            if isinstance(target, dict):
                tables.extend(target.get(name) for name in NORMAL_TABLES)
    return sorted(
        {
            name
            for table in tables
            if isinstance(table, dict)
            for name in table
            if CARD_CRATE.match(name)
        }
    )


def code_only(line: str) -> str:
    """`line` without string literals and a trailing line comment."""
    return STRING.sub('""', line).split("//", 1)[0]


def check(root: Path) -> list[str]:
    allowed = set(json.loads((root / ALLOWLIST).read_text())["crates"])
    found: list[str] = []
    needed: set[str] = set()
    for directory in MANIFEST_ROOTS:
        for manifest_path in sorted((root / directory).glob("*/Cargo.toml")):
            manifest = tomllib.loads(manifest_path.read_text())
            name = manifest.get("package", {}).get("name", manifest_path.parent.name)
            if name.startswith("cards_"):
                continue
            rel = manifest_path.relative_to(root).as_posix()
            used = normal_card_dependencies(manifest)
            if used:
                needed.add(name)
            if used and name not in allowed:
                found.append(
                    f"{rel}: {name} depends on {', '.join(used)} but is not a leaf "
                    f"crate: {WHY}"
                )
            if name in allowed:
                continue
            for source in sorted((manifest_path.parent / "src").rglob("*.rs")):
                for number, line in enumerate(source.read_text().splitlines(), 1):
                    hit = NAMED.search(code_only(line))
                    if hit:
                        found.append(
                            f"{source.relative_to(root).as_posix()}:{number}: names "
                            f"{hit.group(1)} in {name}, which is not a leaf crate: {WHY}"
                        )
    found.extend(
        f"{ALLOWLIST}: remove {name}, which no longer depends on a card crate"
        for name in sorted(allowed - needed)
    )
    return found


def step(*_args: str) -> int:
    """The `card-leaves` check."""
    found = check(paths.REPO)
    for line in found:
        print(f"card-leaves: {line}")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(step(*sys.argv[1:]))
