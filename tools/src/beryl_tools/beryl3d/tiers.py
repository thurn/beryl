"""`just b3d tiers`: the tier chosen at startup and its drop during play, in the running app
with no window (docs/beryl-3d.md, 'Choosing a tier'). It plays `verbs-zones` three times with
every frame held back 25 ms, so that frames run late at 60 Hz, and reads each run's frame log:

- with the setting at Automatic and the measurement passing (`--probe pass`), the tier starts
  Enhanced and drops to Baseline within 120 animating frames after the rate rule's settling
  frames, before any rate step;
- with the setting at Enhanced, it starts Enhanced and never drops;
- with the measurement failing (`--probe fail`), it starts Baseline.

`--probe` replaces the measurement's verdict, so the check does not depend on how loaded the
machine is. It builds the app, so it takes minutes on a cold target.
"""

from __future__ import annotations

import argparse
import json
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .. import deps
from . import app, paths

# The rate rule's frames that are not counted while a new window settles
# (`settle_frames` in data/beryl3d/frame-rate.json).
SETTLE_FRAMES: int = json.loads(
    (paths.DATA / "frame-rate.json").read_text(encoding="utf-8")
)["settle_frames"]
# How many animating frames after settling the drop may take.
DROP_WITHIN = 120
# What the app prints, followed by the path, when it has written its frame log.
FRAME_LOG = "frame log: "
COMMON: tuple[str, ...] = (
    *("--window", "none", "--fixture", "verbs-zones"),
    *("--slow-gpu", "25ms", "--for", "8s"),
)


@dataclass(frozen=True)
class Run:
    name: str
    arguments: tuple[str, ...]
    # The tier the log must say the client started in.
    starts: str
    # Whether the tier must drop to Baseline during play; never, when false.
    drops: bool


RUNS: tuple[Run, ...] = (
    Run("automatic", ("--tier", "automatic", "--probe", "pass"), "enhanced", True),
    Run("enhanced", ("--tier", "enhanced"), "enhanced", False),
    Run(
        "measurement fails",
        ("--tier", "automatic", "--probe", "fail"),
        "baseline",
        False,
    ),
)


def problems(run: Run, lines: list[dict[str, Any]]) -> list[str]:
    """What is wrong with the frame log `lines` of `run`, one sentence each."""
    tier = next((line["tier"] for line in lines if "tier" in line), None)
    summary = next((line["summary"] for line in lines if "summary" in line), None)
    found: list[str] = []
    if tier is None:
        found.append("the log has no tier line")
    elif tier["tier"] != run.starts:
        found.append(f"the tier started {tier['tier']}, not {run.starts}")
    if summary is None:
        return [*found, "the log has no summary"]
    steps = summary.get("steps", [])
    drops = [index for index, step in enumerate(steps) if step["hz"] is None]
    if not run.drops:
        if drops:
            found.append(f"the tier dropped at frame {steps[drops[0]]['frame']}")
        return found
    if not drops:
        return [*found, f"the tier never dropped; the steps were {steps}"]
    if drops[0] != 0:
        found.append(f"the rate stepped before the tier dropped: {steps}")
    # The first frame has no gap to measure, then `SETTLE_FRAMES` are not counted.
    latest = 1 + SETTLE_FRAMES + DROP_WITHIN
    frame = steps[drops[0]]["frame"]
    if frame > latest:
        found.append(f"the tier dropped at frame {frame}, after frame {latest}")
    return found


def play(run: Run) -> list[dict[str, Any]]:
    """Plays `run` in the built app and gives its frame log's lines."""
    command = [str(app.BINARY), *COMMON, *run.arguments]
    result = subprocess.run(
        command, capture_output=True, text=True, check=False, timeout=300
    )
    path = next(
        (
            Path(line.removeprefix(FRAME_LOG).strip())
            for line in result.stdout.splitlines()
            if line.startswith(FRAME_LOG)
        ),
        None,
    )
    if result.returncode != 0 or path is None:
        raise RuntimeError(
            f"{' '.join(command)} exited {result.returncode} with no frame log:\n"
            f"{result.stdout}{result.stderr}"
        )
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]


def tiers_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--no-build", action="store_true")


def run_tiers(args: argparse.Namespace) -> int:
    if not args.no_build and app.build() != 0:
        return deps.CANNOT_RUN
    failed = False
    for run in RUNS:
        try:
            found = problems(run, play(run))
        except RuntimeError as error:
            found = [str(error)]
        for problem in found:
            print(f"tiers: {run.name}: {problem}")
        failed = failed or bool(found)
        if not found:
            print(f"tiers: {run.name}: passed")
    return 1 if failed else 0
