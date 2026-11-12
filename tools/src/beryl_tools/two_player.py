"""The two-player check: rules code and card bodies never name the other player of two.

`Seat` has no `opponent`. The shortcut is a trait in `beryl_kernel::two_player`, in scope
only where a module imports it, so the modules that assume a table of two are the ones that
name that module (docs/project-50k.md, K7; the kernel's contract, "Players"). The compiler
suggests the import to anyone who writes `.opponent()`: this check refuses it where the rules
live, and says what to ask the state instead.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

from . import paths

# Where the rules live: the rules' modules, the facade in front of them, which is held as
# they are so that no rule is written there, and the card definitions.
RULES = (
    "rust/crates/beryl_rules/src",
    "rust/crates/beryl_engine/src",
    "rust/crates/beryl_cards/src",
)
# The facade other crates import the kernel through re-exports the module, and the rules'
# tests set up tables of two.
EXEMPT = ("rust/crates/beryl_engine/src/lib.rs",)
SKIPPED_DIRECTORIES = {"tests"}
NAMED: re.Pattern[str] = re.compile(r"\btwo_player\b")
ADVICE = (
    "ask the state the question the rule asks: opponents_of, defending_players, "
    'next_player, players_in_turn_order (beryl_kernel\'s contract, "Players")'
)


def exempt(rel: str) -> bool:
    parts = rel.split("/")
    return (
        rel in EXEMPT
        or parts[-1] == "tests.rs"
        or bool(SKIPPED_DIRECTORIES.intersection(parts[:-1]))
    )


def check(root: Path) -> list[str]:
    """Each line of rules code under `root` that names the two-player module."""
    found = []
    for directory in RULES:
        base = root / directory
        for path in sorted(base.rglob("*.rs")) if base.is_dir() else []:
            rel = path.relative_to(root).as_posix()
            if exempt(rel):
                continue
            for number, line in enumerate(path.read_text().splitlines(), 1):
                if NAMED.search(line):
                    found.append(
                        f"{rel}:{number}: names the two-player module: {ADVICE}"
                    )
    return found


def step(*_args: str) -> int:
    """The `two-player` check."""
    found = check(paths.REPO)
    for line in found:
        print(f"two-player: {line}")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(step(*sys.argv[1:]))
