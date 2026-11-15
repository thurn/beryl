"""`just timing-parity`: holds the timing the session sends with each step of a script
(`Script.timing`, from `beryl_session`'s `script::timing`) to the web interface's own rules
(`ui/src/stage/timing.ts`), until the web interface plays what the session sends.
`beryl_cli step-timings` records every UI fixture and every card spec moment from both seats,
writes their views to a pack and prints the timing each step was sent with;
`ui/scripts/timings.mjs` prints the web interface's for the same pack; and the two are compared
line by line. Numbers are equal within 0.001. It prints the first differing lines with both
values, and fails on any.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import TYPE_CHECKING, Any

from . import paths
from .reruns import CANNOT_RUN

if TYPE_CHECKING:
    from .checks import Lane

FIXTURES: Path = paths.UI / "fixtures.json"
# How far apart two numbers may be and be equal: the two languages round differently.
TOLERANCE = 0.001
# How many differing lines a failure prints.
SHOWN = 20
# A change to these can change either side's timing of a step.
INPUTS = (
    r"^ui/src/stage/(timing|tracks|spans|table)\.ts$"
    r"|^ui/scripts/timings\.mjs$"
    r"|^rust/crates/beryl_session/src/script/"
    r"|^rust/crates/beryl_cli/src/step_timings\.rs$"
    r"|^data/ui-conventions\.json$"
    r"|^tools/src/beryl_tools/timing_parity\.py$"
)

# One step's place in the recordings: fixture, seat, view number and step number.
Key = tuple[str, str, int, int]


def lane() -> Lane:
    """The parity check as a lane of the fast check (checks.py), run when a file either side's
    timing reads changed."""
    from .checks import BY, Lane  # It imports this module.

    return Lane("timing", warm=True).step("timing-parity", [*BY, "timing-parity"])


def same(a: Any, b: Any) -> bool:
    """Whether two values of a line's timing are equal, numbers within TOLERANCE."""
    numbers = (int, float)
    if isinstance(a, bool) or isinstance(b, bool):
        return a is b
    if isinstance(a, numbers) and isinstance(b, numbers):
        return abs(float(a) - float(b)) <= TOLERANCE
    if isinstance(a, dict) and isinstance(b, dict):
        return a.keys() == b.keys() and all(same(a[k], b[k]) for k in a)
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(same(x, y) for x, y in zip(a, b))
    return a == b


def lines(text: str) -> dict[Key, dict[str, Any]]:
    """Each step's timing in a printer's output, by its place."""
    found: dict[Key, dict[str, Any]] = {}
    for line in text.splitlines():
        if line.strip():
            d = json.loads(line)
            found[(d["fixture"], d["seat"], d["view"], d["step"])] = d["timing"]
    return found


def differences(rust: str, web: str) -> list[str]:
    """One line for each step whose timing differs between the two printers' outputs, or that
    only one of them printed, in the recordings' order, naming the fields that differ.
    """
    ours, theirs = lines(rust), lines(web)
    found: list[str] = []
    for key in list(ours) + [k for k in theirs if k not in ours]:
        where = "{} {} view {} step {}".format(*key)
        if key not in theirs:
            found.append(f"{where}: only the session times it")
        elif key not in ours:
            found.append(f"{where}: only the web interface times it")
        elif not same(ours[key], theirs[key]):
            a, b = ours[key], theirs[key]
            fields = sorted(
                f for f in a.keys() | b.keys() if not same(a.get(f), b.get(f))
            )
            said = ", ".join(
                f"{f}: session {json.dumps(a.get(f))}, web {json.dumps(b.get(f))}"
                for f in fields
            )
            found.append(f"{where} ({a.get('case') or b.get('case')}): {said}")
    return found


def run(fixtures_only: bool = False) -> int:
    """Records, times both ways and compares; 0 with no difference."""
    with tempfile.TemporaryDirectory(prefix="timing-parity-") as scratch:
        pack = Path(scratch) / "pack.json"
        flags = ["--fixtures-only"] if fixtures_only else []
        rust = subprocess.run(
            ["cargo", "run", "--locked", "--quiet", "--package", "beryl_cli", "--"]
            + ["step-timings", str(FIXTURES), str(pack), *flags],
            cwd=paths.RUST,
            env=paths.run_env(),
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
        )
        if rust.returncode != 0:
            sys.stderr.write(rust.stderr)
            print("timing-parity: the session's timings could not be made")
            return CANNOT_RUN
        web = subprocess.run(
            ["node", "scripts/timings.mjs", str(pack)],
            cwd=paths.UI,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
        )
        if web.returncode != 0:
            sys.stderr.write(web.stderr)
            print("timing-parity: the web interface's timings could not be made")
            return 1
    found = differences(rust.stdout, web.stdout)
    steps = len(lines(rust.stdout))
    for line in found[:SHOWN]:
        print(line)
    if len(found) > SHOWN:
        print(f"... and {len(found) - SHOWN} more")
    print(f"timing-parity: {steps} steps, {len(found)} differ")
    return 1 if found else 0


def main(args: argparse.Namespace) -> int:
    return run(args.fixtures_only)


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "timing-parity",
        help="hold the session's step timings to the web interface's on every recording",
    )
    p.add_argument(
        "--fixtures-only",
        action="store_true",
        help="time the UI fixtures alone, not the card spec moments",
    )
    p.set_defaults(func=main)
