"""The `repo:just` check: the justfile is as `just --fmt` writes it.

`just`'s formatter changes between releases (1.59 writes `set positional-arguments` where
older ones wrote `set positional-arguments := true`), so the check's result depends on the
installed `just` as well as the tree. An older `just` cannot run the check: it exits
`CANNOT_RUN` with how to upgrade, which fails the lane but gives no verdict, instead of
reporting a justfile that is fine as badly formatted and then as flaky once `just` is upgraded.
"""

from __future__ import annotations

import re
import subprocess
import sys

from . import paths
from .reruns import CANNOT_RUN

# The oldest `just` whose formatting the justfile follows; README.md's setup table says so too.
MINIMUM = (1, 59)


def version(text: str) -> tuple[int, int] | None:
    """The major and minor version in `just --version`'s output, such as `just 1.59.0`."""
    found = re.search(r"(\d+)\.(\d+)", text)
    return (int(found[1]), int(found[2])) if found else None


def step(*_args: str) -> int:
    """Checks the justfile's format, or exits `CANNOT_RUN` when `just` is too old to."""
    said = subprocess.run(["just", "--version"], capture_output=True, text=True)
    have = version(said.stdout)
    if have is None or have < MINIMUM:
        print(
            f"just: {said.stdout.strip() or 'no version'} is older than "
            f"{'.'.join(map(str, MINIMUM))}, whose formatting the justfile follows: "
            "run `cargo install just --locked`"
        )
        return CANNOT_RUN
    check = ["just", "--fmt", "--check", "--unstable"]
    return subprocess.run(check, cwd=paths.REPO).returncode


if __name__ == "__main__":
    sys.exit(step(*sys.argv[1:]))
