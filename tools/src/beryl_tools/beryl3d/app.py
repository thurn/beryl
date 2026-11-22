"""`just b3d app` runs `beryl3d`, `just b3d smoke` waits for its start screen without a
window, and `just b3d frames` prints a frame log's summary and its worst frames.
`just b3d release` packages the optimized app on macOS and builds it elsewhere.

The app is built with cargo's `test` profile, as the workspace's tests are: optimized enough to
measure frames, it shares the dependencies `just b3d cargo-test` built, and the `tools` feature
gives it `--fixture`, `--pause-at`, `--drive`, `--watch`, `--load`, `--slow-gpu`, `--offer`, `--tier`,
`--probe` and `--agent` with its `--opponent`, `--seed` and `--search-class`, which a release
build leaves off.
"""

from __future__ import annotations

import argparse
import json
import os
import plistlib
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
from pathlib import Path
from typing import Any

from .. import deps, paths as repo_paths, system
from . import live, paths
from .command import Command

TARGET: Path = paths.WORKSPACE / "target"
BINARY: Path = system.exe(TARGET / "debug" / "beryl3d")
RELEASE_BINARY: Path = system.exe(TARGET / "release" / "beryl3d")
MAC_APP_NAME = "Beryl 3D"
# What the app prints when its first frame is shown (`FIRST_SHOWN` in beryl_3d's app.rs).
FIRST_SHOWN = "beryl3d: first frame shown"
# What it prints, followed by the path, when its live socket listens (`LIVE_SOCKET`).
LIVE_SOCKET = "beryl3d: live socket"
# What it prints, followed by the folder, about where a live game is saved (`data_dirs`).
DATA_IN = "beryl3d: data in "
# How long `smoke` lets the app run before it ends itself: a bound on a hang, never reached by
# a healthy start, which shows the start screen within a few seconds.
SMOKE_LIMIT_S = 120
# The status the app exits with when the recordings pack its `--fixture` reads is missing
# (`beryl_inspect::recordings::MISSING_STATUS`, `cli.MISSING_PACK`).
MISSING_PACK = 3
# How many of a log's late frames `frames` prints.
WORST = 10
WINDOWS = ("none", "background", "foreground", "fullscreen")


def record() -> int:
    from .cli import record  # cli imports this module.

    return record()


def build() -> int:
    from .cli import build as cargo_build  # cli imports this module.

    return cargo_build(
        [
            *["build", "--profile", "test", "-p", "beryl_3d"],
            *["--features", "tools", "--bin", "beryl3d"],
        ]
    )


def build_release() -> int:
    from .cli import build as cargo_build  # cli imports this module.

    return cargo_build(
        [
            "build",
            "--release",
            "--target-dir",
            str(TARGET),
            "-p",
            "beryl_3d",
            "--bin",
            "beryl3d",
        ]
    )


def package_macos(bundle: Path) -> None:
    """Build and sign a complete bundle before replacing an earlier release."""
    manifest = paths.WORKSPACE / "crates" / "beryl_3d" / "Cargo.toml"
    version = tomllib.loads(manifest.read_text())["package"]["version"]
    icon = repo_paths.APP / "icons" / "icon.icns"
    with tempfile.TemporaryDirectory(prefix=".beryl3d-", dir=bundle.parent) as scratch:
        staged = Path(scratch) / bundle.name
        contents = staged / "Contents"
        executable = contents / "MacOS" / RELEASE_BINARY.name
        executable.parent.mkdir(parents=True)
        shutil.copy(RELEASE_BINARY, executable)
        resources = contents / "Resources"
        shutil.copytree(
            repo_paths.UI / "public" / "audio",
            resources / "audio",
            copy_function=shutil.copy,
        )
        shutil.copyfile(icon, resources / icon.name)
        with (contents / "Info.plist").open("wb") as info:
            plistlib.dump(
                {
                    "CFBundleExecutable": executable.name,
                    "CFBundleIconFile": icon.name,
                    "CFBundleIdentifier": "ca.thurn.beryl3d",
                    "CFBundleInfoDictionaryVersion": "6.0",
                    "CFBundleName": MAC_APP_NAME,
                    "CFBundlePackageType": "APPL",
                    "CFBundleShortVersionString": version,
                    "CFBundleVersion": version,
                    "NSHighResolutionCapable": True,
                },
                info,
            )
        subprocess.run(
            ["/usr/bin/codesign", "--force", "--sign", "-", staged], check=True
        )

        previous = Path(scratch) / "previous.app"
        if bundle.exists():
            bundle.rename(previous)
        try:
            staged.rename(bundle)
        finally:
            if previous.exists() and not bundle.exists():
                previous.rename(bundle)


def run_release(_args: argparse.Namespace) -> int:
    if build_release() != 0:
        return deps.CANNOT_RUN
    output = RELEASE_BINARY
    if sys.platform == "darwin":
        output = RELEASE_BINARY.with_name(f"{MAC_APP_NAME}.app")
        try:
            with system.file_lock(paths.build_lock()):
                package_macos(output)
        except (OSError, subprocess.CalledProcessError) as error:
            print(f"release: cannot package {output}: {error}", file=sys.stderr)
            return deps.CANNOT_RUN
    print(f"release: {output}")
    return 0


COMMANDS = (
    Command(
        "release",
        "builds the release app bundle on macOS and the executable on other platforms",
        (
            ("just b3d release", "build the optimized desktop app and print its path"),
            ("just b3d release --help", "this text"),
        ),
        run_release,
    ),
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
    parser.add_argument(
        "--replay",
        help="resume the game of this replay file (a bug report's replay.json), saving none",
    )
    parser.add_argument("--pause-at", help="stop the clock at this moment (1500ms)")
    parser.add_argument(
        "--for",
        dest="run_for",
        help="end after this long (bare milliseconds or a unit)",
    )
    parser.add_argument(
        "--scale",
        help="pixels per point with no window (2 on macOS, as on a Retina display)",
    )
    parser.add_argument(
        "--drive",
        help="play this script of actions (a name in rust/beryl3d/drive), then end",
    )
    parser.add_argument(
        "--load",
        help="start busy threads: <threads>[:utility|default|initiated|interactive]",
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
        choices=("utility", "default", "initiated", "interactive"),
        help="the scheduling class of the AI's threads (default)",
    )
    parser.add_argument(
        "--offer",
        action="append",
        default=[],
        help="add this agent to the start screen's opponents (expert); repeatable",
    )
    parser.add_argument(
        "--tier",
        choices=("automatic", "baseline", "enhanced"),
        help="how the visual tier is chosen, in place of the saved setting",
    )
    parser.add_argument(
        "--probe",
        choices=("pass", "fail"),
        help="the startup measurement's verdict, in place of the one it measures",
    )
    parser.add_argument(
        "--window",
        choices=WINDOWS,
        help="none for an agent and fullscreen for a person, unless given",
    )
    parser.add_argument(
        "--watch", action="store_true", help="reload tuning files when they change"
    )
    parser.add_argument("--no-build", action="store_true")


def run_app(args: argparse.Namespace) -> int:
    try:
        window = choose_window(args.window, args.run_for, repo_paths.is_agent())
    except ValueError as error:
        print(f"app: {error}", file=sys.stderr)
        return 2
    if not args.no_build and build() != 0:
        return deps.CANNOT_RUN
    command = [str(BINARY), "--window", window]
    for option, value in (
        ("--fixture", args.fixture),
        ("--replay", args.replay),
        ("--pause-at", args.pause_at),
        ("--for", args.run_for),
        ("--scale", args.scale),
        ("--drive", args.drive),
        ("--load", args.load),
        ("--slow-gpu", args.slow_gpu),
        ("--agent", args.agent),
        ("--opponent", args.opponent),
        ("--seed", args.seed),
        ("--search-class", args.search_class),
        ("--tier", args.tier),
        ("--probe", args.probe),
    ):
        if value is not None:
            command += [option, value]
    if args.watch:
        command.append("--watch")
    for name in args.offer:
        command += ["--offer", name]
    started_ms = int(time.time() * 1000)
    code = subprocess.run(command, check=False).returncode
    if code == MISSING_PACK and record() == 0:
        code = subprocess.run(command, check=False).returncode
    if code != 0 or args.run_for is None:
        return code
    path = newest_log(frames_dir(), repo_paths.REPO.resolve())
    started = int(path.name.split("-", 1)[0]) if path else 0
    failure = undelivered(path) if path and started >= started_ms else None
    if failure:
        print(f"app: FAILED: {failure}", file=sys.stderr)
        return 1
    return 0


def undelivered(path: Path) -> str | None:
    """Why the run whose frame log is `path` ended with text it could not draw: glyphs its
    last frame's text asked for that no atlas page held (`missing_glyphs`). A host delivers
    glyphs within a few frames of being asked, so a run that ends seconds in with any missing
    has a host that is not delivering them, as when the renderer refused an atlas page past its
    last and the client drew \"You won the  ip\". None when the run drew everything, or wrote no
    summary."""
    summary = None
    for line in path.read_text(encoding="utf-8").splitlines():
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if isinstance(entry, dict) and isinstance(entry.get("summary"), dict):
            summary = entry["summary"]
    missing = summary.get("missing_glyphs", 0) if summary else 0
    if not missing:
        return None
    return (
        f"the run ended with {missing} glyphs its text asked for that no atlas page held;"
        f" the host is not delivering them ({path})"
    )


def smoke_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--no-build", action="store_true")


def run_smoke(args: argparse.Namespace) -> int:
    """Starts the app with no window under a profile of its own, which has no game to resume,
    waits for its first shown frame and for its live socket's tree to show the start screen (a
    region named `start`), and quits it. The app ends itself after `SMOKE_LIMIT_S` if it hangs
    before then.
    """
    if not args.no_build and build() != 0:
        return deps.CANNOT_RUN
    started = time.monotonic()
    try:
        shown = start_screen()
    except SmokeError as error:
        print(f"smoke: FAILED: {error}")
        return 1
    print(f"{shown}; the start screen {time.monotonic() - started:.1f} s from start")
    print("smoke: passed")
    return 0


class SmokeError(Exception):
    """Why the smoke failed."""


def start_screen() -> str:
    """Runs the app until its tree shows the start screen, then quits it and deletes its
    profile's folder. Gives the line saying the first frame was shown."""
    profile = f"b3d-smoke-{os.getpid()}"
    command = [str(BINARY), "--window", "none", "--for", f"{SMOKE_LIMIT_S}s"]
    process = subprocess.Popen(
        command,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        env={**os.environ, "BERYL_PROFILE": profile},
    )
    output = process.stdout
    if output is None:
        raise RuntimeError("the app's output is not a pipe")
    seen: list[str] = []
    socket_path, shown, data = None, None, None
    try:
        for line in output:
            seen.append(line.rstrip("\n"))
            if line.startswith(f"{LIVE_SOCKET} "):
                socket_path = Path(line.strip().removeprefix(f"{LIVE_SOCKET} "))
            elif line.startswith(DATA_IN):
                data = Path(line.strip().removeprefix(DATA_IN))
            elif line.startswith(FIRST_SHOWN):
                shown = line.strip()
                break
        if shown is None or socket_path is None:
            raise SmokeError(
                f"the app showed no frame (exit {process.poll()}):\n" + "\n".join(seen)
            )
        # Read on, so that the app never blocks on a full pipe.
        threading.Thread(target=output.read, daemon=True).start()
        wait_for_start(socket_path)
        live.request(socket_path, "quit")
        process.wait(timeout=30)
        return shown
    except OSError as error:
        raise SmokeError(f"the live socket did not answer: {error}") from error
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        if data is not None and data.name == profile:
            shutil.rmtree(data, ignore_errors=True)


def wait_for_start(socket_path: Path) -> None:
    """Asks for the tree until it has a region named `start`."""
    deadline = time.monotonic() + SMOKE_LIMIT_S
    tree = ""
    while time.monotonic() < deadline:
        accepted, tree = live.request(socket_path, "tree")
        if accepted and 'region "start" #' in tree:
            return
        time.sleep(0.1)
    raise SmokeError(f"the tree never showed the start screen:\n{tree}")


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
    tier = next((line["tier"] for line in lines if "tier" in line), None)
    if tier is not None:
        out.append(tier_line(tier))
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


def tier_line(tier: dict[str, Any]) -> str:
    """The tier chosen at startup (docs/beryl-3d.md, 'Choosing a tier'), one line."""
    measured = (
        f"slowest measured frame {tier['slowest_ms']:.1f} ms"
        if tier.get("slowest_ms") is not None
        else "nothing measured"
    )
    return (
        f"tier: {tier['tier']} at startup (setting {tier['setting']}, {measured},"
        f" interval {tier['interval_ms']:.1f} ms; {tier.get('took_ms', 0.0):.0f} ms before the first frame)"
    )


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
            else f"Baseline tier at {step['at_ms']:.0f} ms (frame {step.get('frame', '?')})"
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
