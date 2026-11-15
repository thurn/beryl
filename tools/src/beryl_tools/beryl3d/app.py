"""The running app (docs/beryl-3d.md, 'Driving the running app'): `just b3d app` runs
`beryl3d`, `just b3d smoke` starts it with no window and waits for its first frame, and
`just b3d frames` prints a frame log's summary and its worst frames.

The app is built with cargo's `test` profile, as the workspace's tests are: optimized enough to
measure frames, it shares the dependencies `just b3d cargo-test` built, and the `tools` feature
gives it `--fixture`, `--pause-at`, `--drive`, `--load`, `--slow-gpu` and `--agent` with its
`--opponent`, `--seed` and `--search-class`, which a release build leaves off.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from .. import checks, paths as repo_paths, system
from . import paths

BINARY: Path = system.exe(paths.WORKSPACE / "target" / "debug" / "beryl3d")
# What the app prints when its first frame is shown (`FIRST_SHOWN` in beryl_3d's app.rs).
FIRST_SHOWN = "beryl3d: first frame shown"
# How long `smoke` lets the app run before it ends itself: a bound on a hang, never reached by
# a healthy start, which shows a frame within a second of the recordings pack being read.
SMOKE_LIMIT_S = 120
# How many of a log's late frames `frames` prints.
WORST = 10
WINDOWS = ("none", "background", "foreground", "fullscreen")


def build() -> int:
    from .cli import cargo  # cli imports this module.

    return cargo(
        [
            *["build", "--profile", "test", "-p", "beryl_3d"],
            *["--features", "tools", "--bin", "beryl3d"],
        ]
    )


def frames_dir() -> Path:
    return paths.cache() / "frames"


def choose_window(window: str | None, run_for: str | None, agent: bool) -> str:
    """The window a run opens: what it names, else `none` for an agent and `fullscreen` for a
    person (decision C15). An agent's run must give `--for`, so that it always ends: with no
    window, nothing else would end it either. Raises ValueError when it does not."""
    if agent and run_for is None:
        raise ValueError("an agent's run must give --for, so that it ends")
    if window is None:
        return "none" if agent else "fullscreen"
    return window


def app_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--fixture", help="replay this recording instead of a game")
    parser.add_argument("--pause-at", help="stop the clock at this moment (1500ms)")
    parser.add_argument("--for", dest="run_for", help="end the run after this long")
    parser.add_argument(
        "--drive",
        help="play this script of actions (a name in rust/beryl3d/drive), then end",
    )
    parser.add_argument(
        "--load", help="start busy threads: <threads>[:default|utility|interactive]"
    )
    parser.add_argument(
        "--slow-gpu", help="hold each finished frame back this long (5ms)"
    )
    parser.add_argument(
        "--agent",
        help="start a new game this agent plays for the viewer; the run ends when it is over",
    )
    parser.add_argument("--opponent", help="--agent's opponent (mcts-hidden)")
    parser.add_argument("--seed", help="--agent's game's seed")
    parser.add_argument(
        "--search-class",
        choices=("utility", "default", "interactive"),
        help="the scheduling class of the AI's threads (utility)",
    )
    parser.add_argument(
        "--window",
        choices=WINDOWS,
        help="none for an agent and fullscreen for a person, unless given",
    )
    parser.add_argument("--no-build", action="store_true")


def run_app(args: argparse.Namespace) -> int:
    try:
        window = choose_window(args.window, args.run_for, repo_paths.is_agent())
    except ValueError as error:
        print(f"app: {error}", file=sys.stderr)
        return 2
    if not args.no_build and build() != 0:
        return checks.CANNOT_RUN
    command = [str(BINARY), "--window", window]
    for option, value in (
        ("--fixture", args.fixture),
        ("--pause-at", args.pause_at),
        ("--for", args.run_for),
        ("--drive", args.drive),
        ("--load", args.load),
        ("--slow-gpu", args.slow_gpu),
        ("--agent", args.agent),
        ("--opponent", args.opponent),
        ("--seed", args.seed),
        ("--search-class", args.search_class),
    ):
        if value is not None:
            command += [option, value]
    return subprocess.run(command, check=False).returncode


def smoke_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--no-build", action="store_true")


def run_smoke(args: argparse.Namespace) -> int:
    """Starts the app with no window on `verbs-zones`, waits for its first shown frame and ends
    it. The app ends itself after `SMOKE_LIMIT_S` if it hangs before then."""
    if not args.no_build and build() != 0:
        return checks.CANNOT_RUN
    started = time.monotonic()
    command = [str(BINARY), "--window", "none", "--fixture", "verbs-zones"]
    process = subprocess.Popen(
        [*command, "--for", f"{SMOKE_LIMIT_S}s"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        text=True,
    )
    output = process.stdout
    if output is None:
        raise RuntimeError("the app's output is not a pipe")
    shown = None
    for line in output:
        if line.startswith(FIRST_SHOWN):
            shown = line.strip()
            break
    process.terminate()
    try:
        process.wait(timeout=30)
    except subprocess.TimeoutExpired:
        process.kill()
    if shown is None:
        print(f"smoke: FAILED: the app showed no frame (exit {process.returncode})")
        return 1
    print(f"{shown}; {time.monotonic() - started:.1f} s from start")
    print("smoke: passed")
    return 0


def first_line(path: Path) -> dict[str, object] | None:
    """The run a frame log's first line describes, or None when it is not one."""
    try:
        with path.open(encoding="utf-8") as file:
            line = json.loads(file.readline())
    except (OSError, ValueError):
        return None
    run = line.get("run") if isinstance(line, dict) else None
    return run if isinstance(run, dict) else None


def newest_log(folder: Path, checkout: Path) -> Path | None:
    """The newest log in `folder` of a run started from `checkout`. Names begin with the run's
    start time in milliseconds, so the largest is the newest."""
    found = []
    for path in folder.glob("*.jsonl"):
        start = path.name.split("-", 1)[0]
        run = first_line(path)
        if start.isdigit() and run and Path(str(run.get("checkout"))) == checkout:
            found.append((int(start), path))
    return max(found)[1] if found else None


def report(path: Path) -> str:
    """A log's summary and its `WORST` worst frames, as `frames` prints them."""
    lines = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]
    run = lines[0]["run"]
    late = [line["late"] for line in lines if "late" in line]
    summary = next((line["summary"] for line in lines if "summary" in line), None)
    out = [
        f"{path}",
        f"run: {run['window']} window, {run['display_hz']} Hz, {run['adapter']},"
        f" load {run['load']}, fixture {run['fixture']}",
    ]
    conditions = [
        f"{label} {run[key]}"
        for key, label in (
            ("drive", "drive"),
            ("load_threads", "load threads"),
            ("slow_gpu_ms", "slow GPU ms"),
            ("game", "game"),
        )
        if run.get(key) is not None
    ]
    if conditions:
        out.append("made to imitate: " + ", ".join(conditions))
    if summary is None:
        out.append("no summary: the run ended before it wrote one")
    else:
        out += [
            f"{summary['frames']} frames in {summary['seconds']:.1f} s, each on a display tick;"
            f" first shown at {summary['first_shown_ms']} ms",
            rate_line(run, summary),
            f"late against the rate asked for: {summary['late']} ({summary['held_to_60']} held to 60 Hz,"
            f" {summary['missed_60']} missed 60 Hz); worst gap {summary['worst_gap_ms']:.1f} ms",
            f"waited for a drawable: median {summary.get('median_wait_ms', 0.0):.1f} ms,"
            f" 95th percentile {summary.get('p95_wait_ms', 0.0):.1f} ms,"
            f" worst {summary['worst_wait_ms']:.1f} ms; {summary['total_wait_ms']:.1f} ms in all;"
            f" {summary['skipped']} frames found none",
        ]
        searching = summary.get("searching", {})
        if searching.get("frames"):
            out.append(
                f"while the AI searched: {searching['frames']} frames, {searching['late']} late;"
                f" worst client {searching['worst_client_ms']:.2f} ms,"
                f" worst encode {searching['worst_encoded_ms']:.2f} ms"
            )
        if run.get("pause_at_ms") is not None:
            out.append(
                f"paused at {run['pause_at_ms']} ms:"
                f" {summary['after_pause']} frames drawn after the pause"
            )
    worst = sorted(late, key=lambda frame: -frame["since_last_ms"])[:WORST]
    if worst:
        out.append(f"worst {len(worst)} of {len(late)} late frames (ms):")
        for frame in worst:
            out.append(
                f"  at {frame['at_ms']:.0f}: gap {frame['since_last_ms']:.1f}"
                f" at {frame.get('hz', run['display_hz'])} Hz,"
                f" waited {frame['waited_ms']:.1f}, client {frame['client_ms']:.2f},"
                f" encoded {frame['encoded_ms']:.2f}, searching {frame['searching']},"
                f" verbs {','.join(frame['verbs']) or '-'}"
            )
    return "\n".join(out)


def rate_line(run: dict[str, Any], summary: dict[str, Any]) -> str:
    """The rate the host asked the display for and its steps down and up (decision C18), one
    line. A log written before the host stepped has no steps and asked for the display's rate.
    """
    steps = summary.get("steps", [])
    if not steps:
        return f"asked for {run['display_hz']} Hz throughout"
    said = [
        (
            f"{step['hz']} Hz at {step['at_ms']:.0f} ms"
            if step["hz"] is not None
            else f"Baseline tier at {step['at_ms']:.0f} ms"
        )
        for step in steps
    ]
    return (
        f"asked for {run['display_hz']} Hz, then {', '.join(said)};"
        f" {summary['hz']} Hz at the end"
    )


def frames_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "path", nargs="?", type=Path, help="a frame log; the newest if none"
    )


def run_frames(args: argparse.Namespace) -> int:
    path = args.path or newest_log(frames_dir(), repo_paths.REPO.resolve())
    if path is None:
        print(
            f"frames: no frame log in {frames_dir()} names this checkout;"
            " `just b3d app` writes one",
            file=sys.stderr,
        )
        return 1
    print(report(path))
    return 0
