"""The discarded-push check: the kernel, the rules and the cards never throw a failed push away.

A push to a fixed list returns `Full` when the list has no room, and a game that runs out of
room stalls into the layout whose lists grow (docs/rules-coverage.md, principle 3).
`let _ = list.push(x)` plays on without what did not fit instead: an effect the layers never
applied, a blocker that was dealt no damage. A comment beside it arguing that the list cannot
fill is checked by nothing, and is wrong once the game outgrows what it assumed.

So a push says which it is. A list whose bound is argued takes `Queue::push_bounded`, which
checks the argument in debug builds, where the scenarios and `just fuzz` trip on a wrong one.
A list a heuristic samples takes `Queue::push_if_room`. Every other push propagates `Full`.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

from . import paths, two_player

# The kernel beside where the rules live: its mutators fill the state's lists.
SCANNED: tuple[str, ...] = ("rust/crates/beryl_kernel/src", *two_player.RULES)
# A push whose result is bound to nothing or turned into an option nobody reads. The
# statement may run over several lines: a `;` ends it. A match arm's `_ =>` binds nothing.
DISCARDED: re.Pattern[str] = re.compile(
    r"(?<![\w.])_\s*=(?![=>])[^;]*?\.push\(|\.push\([^;]*\)\s*\.ok\(\)\s*;"
)
# A file's own tests follow its code.
TESTS: re.Pattern[str] = re.compile(r"^#\[cfg\(test\)\]", re.MULTILINE)
ADVICE = (
    "say why the list cannot be full and call push_bounded, which checks it in debug "
    "builds; or propagate Full, so the game stalls into Large; or, for a heuristic's "
    "sample, push_if_room (beryl_kernel's Queue)"
)


def exempt(rel: str) -> bool:
    parts = rel.split("/")
    return parts[-1] == "tests.rs" or bool(
        two_player.SKIPPED_DIRECTORIES.intersection(parts[:-1])
    )


def check(root: Path) -> list[str]:
    """Each push under `root`, outside tests, whose result is thrown away."""
    found = []
    for directory in SCANNED:
        base = root / directory
        for path in sorted(base.rglob("*.rs")) if base.is_dir() else []:
            rel = path.relative_to(root).as_posix()
            if exempt(rel):
                continue
            text = path.read_text()
            tests = TESTS.search(text)
            code = text[: tests.start()] if tests else text
            for match in DISCARDED.finditer(code):
                number = code.count("\n", 0, match.start()) + 1
                found.append(f"{rel}:{number}: discards a failed push: {ADVICE}")
    return found


def step(*_args: str) -> int:
    """The `discarded-push` check."""
    found = check(paths.REPO)
    for line in found:
        print(f"discarded-push: {line}")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(step(*sys.argv[1:]))
