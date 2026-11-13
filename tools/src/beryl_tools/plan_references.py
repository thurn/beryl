"""The plan-references check: code never cites a planning document.

A comment that says which decision of a plan it follows points at a document that is later
retired or reworded, and then at nothing. Say what the code does in the comment, or link to
`docs/engine-architecture.md`. The patterns are written so that this file does not match them.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

from . import paths

SCANNED = ("rust", "ui/src", "ui/scripts", "tools/src")
SUFFIXES: frozenset[str] = frozenset(
    {".rs", ".toml", ".ts", ".tsx", ".mjs", ".js", ".py"}
)
REFERENCE: re.Pattern[str] = re.compile(r"plan K[0-9]|[(]plan,|project-50[k]")
ADVICE = "do not cite a planning document; say what the code does, or cite docs/engine-architecture.md"


def check(root: Path) -> list[str]:
    """Each line under the scanned directories of `root` that cites a plan decision."""
    found = []
    for directory in SCANNED:
        base = root / directory
        files = sorted(base.rglob("*")) if base.is_dir() else []
        for path in files:
            if (
                path.suffix not in SUFFIXES
                or "node_modules" in path.parts
                or "target" in path.parts
            ):
                continue
            for number, line in enumerate(path.read_text().splitlines(), 1):
                if REFERENCE.search(line):
                    rel = path.relative_to(root).as_posix()
                    found.append(f"{rel}:{number}: {ADVICE}")
    return found


def step(*_args: str) -> int:
    """The `plan-references` check."""
    found = check(paths.REPO)
    for line in found:
        print(f"plan-references: {line}")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(step(*sys.argv[1:]))
