"""Which Rust tests the fast check's tests step runs, so that it finishes inside the fast check's
target (deadline.py) and leaves the rest to slow CI.

Before this, the step ran every test of the packages a change can break: 225 s for one push
on a loaded machine, nearly four times the whole check's budget. Now, once the tests are built,
the step lists them and runs as many as fit the seconds left, in this order:

1. Tests the run log has never timed. They are most likely the ones the change added or
   renamed.
2. The rest by their record (selection.Record): tests that caught a break first, then those
   that fail most often, then the cheapest.

Doctests and the `allocations` binaries, which run beside nextest, are planned the same way
under their run-log checks, `rust:doctests` and `rust:allocations`.
"""

from __future__ import annotations

import json
import subprocess
from collections.abc import Mapping
from pathlib import Path

from . import paths, selection

# The seconds the plan keeps back for nextest to start and for the step to write its results.
MARGIN = 5.0


def listed(
    scope: list[str],
    expression: str,
    cwd: Path = paths.RUST,
    env: Mapping[str, str] | None = None,
) -> list[str] | None:
    """The tests the built binaries of the workspace at `cwd` hold that match the nextest
    filterset `expression`, each as `<binary> <test>` (rust_tests.nextest_filter), or None when
    nextest cannot list them. Beryl 3D's workspace (beryl3d/test_fit.py) lists its own.
    """
    done = subprocess.run(
        [
            *["cargo", "nextest", "list", "--locked", *scope, "--cargo-quiet"],
            *["--message-format", "json", "-E", expression],
        ],
        cwd=cwd,
        env=None if env is None else dict(env),
        capture_output=True,
        text=True,
        errors="replace",
        check=False,
    )
    if done.returncode != 0:
        return None
    try:
        suites = json.loads(done.stdout)["rust-suites"]
    except (ValueError, KeyError, TypeError):
        return None
    found: list[str] = []
    for binary, suite in suites.items():
        for name, case in suite.get("testcases", {}).items():
            if case.get("filter-match", {}).get("status") == "matches":
                found.append(f"{binary} {name}")
    return sorted(found)


def check_id(test: str, prefix: str = "rust") -> str:
    """The run-log check of a `<binary> <test>` (rust_tests.junit_results), `rust:` for the main
    workspace's tests and another `prefix` for another workspace's."""
    binary, name = test.split(" ", 1)
    return f"{prefix}:{binary}::{name}"


def plan(
    tests: list[str],
    beside: list[str],
    known: Mapping[str, selection.Record],
    left: float,
    threads: int,
    prefix: str = "rust",
) -> tuple[list[str], list[str], list[str]]:
    """The `tests` (`<binary> <test>`) and `beside` checks (`rust:doctests`, `rust:allocations`)
    that fit `left` seconds on `threads` threads, and the run-log checks of those cut. Each
    test's check is named with `prefix` (`check_id`).
    """
    ids = {check_id(t, prefix): t for t in tests}
    candidates = [*ids, *beside]
    new = [c for c in candidates if c not in known]
    ordered = new + selection.order([c for c in candidates if c in known], known)
    chosen, cut = selection.fit(ordered, known, left - MARGIN, max(1, threads))
    run = set(chosen)
    return (
        [t for c, t in ids.items() if c in run],
        [c for c in beside if c in run],
        cut,
    )


def deferred_line(cut: list[str], left: float) -> str:
    return (
        f"rust-tests: {len(cut)} tests deferred to slow CI: they do not fit the"
        f" {max(left, 0):.0f} s left of the fast check's target"
    )
