"""The board-door check: rules code and card bodies never open the board door.

`State::board()` lends `beryl_kernel`'s `Board`, which writes any field raw, with no event and
no invariant kept. It is public because scenarios, checkpoints, tools and tests arrange
positions through it; the rules change the state only through the mutators (the kernel's
contract, "The board door" and "Mutators"). `beryl_rules` holds `&mut State` in every rules
module, so the compiler cannot keep the door shut there: this check refuses it where the rules
live, outside their tests, and says what to add instead (docs/project-50k.md, K6).
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

from . import paths, two_player

NAMED: re.Pattern[str] = re.compile(r"\.board\(\)|\bboard::Board\b")
ADVICE = (
    "rules code changes the state through a mutator (beryl_kernel's contract, "
    "'Mutators'); add the mutator the rule needs"
)


def exempt(rel: str) -> bool:
    """Tests may arrange positions through the door; unlike the two-player check, the engine's
    facade (lib.rs) may not."""
    parts = rel.split("/")
    return parts[-1] == "tests.rs" or bool(
        two_player.SKIPPED_DIRECTORIES.intersection(parts[:-1])
    )


def check(root: Path) -> list[str]:
    """Each line of rules code under `root` that opens the board door."""
    found = []
    for directory in two_player.RULES:
        base = root / directory
        for path in sorted(base.rglob("*.rs")) if base.is_dir() else []:
            rel = path.relative_to(root).as_posix()
            if exempt(rel):
                continue
            for number, line in enumerate(path.read_text().splitlines(), 1):
                if NAMED.search(line):
                    found.append(f"{rel}:{number}: opens the board door: {ADVICE}")
    return found


def step(*_args: str) -> int:
    """The `board-door` check."""
    found = check(paths.REPO)
    for line in found:
        print(f"board-door: {line}")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(step(*sys.argv[1:]))
