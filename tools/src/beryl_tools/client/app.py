"""`just client app` runs `beryl`, `just client smoke` waits for its start screen without a
window, and `just client frames` prints a frame log's summary and its worst frames.
`just client release` packages the optimized app on macOS and builds it elsewhere.

The app is built with cargo's `test` profile, as the workspace's tests are: optimized enough to
measure frames, it shares the dependencies `just client cargo-test` built, and the `tools` feature
gives it `--fixture`, `--pause-at`, `--drive`, `--watch`, `--load`, `--slow-gpu`, `--fixed-hz`, `--offer`, `--tier`,
`--probe` and `--agent` with its `--opponent`, `--deck`, `--seed` and `--search-class`, which a release
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
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Any

from .. import audio, bugs, deps, paths as repo_paths, system
from . import cargo, frame_trace, live, paths
from .command import Command

TARGET: Path = paths.WORKSPACE / "target"
BINARY: Path = system.exe(TARGET / "debug" / "beryl")
RELEASE_BINARY: Path = system.exe(TARGET / "release" / "beryl")
MAC_APP_NAME = "Beryl"
# What the app prints when its first frame is shown (`FIRST_SHOWN` in beryl_client's app.rs).
FIRST_SHOWN = "beryl: first frame shown"
# What it prints, followed by the path, when its live socket listens (`LIVE_SOCKET`).
LIVE_SOCKET = "beryl: live socket"
# What it prints, followed by the folder, about where a live game is saved (`data_dirs`).
DATA_IN = "beryl: data in "
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
    return cargo.build(
        [
            *["build", "--profile", "test", "-p", "beryl_client"],
            *["--features", "tools", "--bin", "beryl"],
        ]
    )


def build_release() -> int:
    return cargo.build(
        [
            "build",
            "--release",
            "--target-dir",
            str(TARGET),
            "-p",
            "beryl_client",
            "--bin",
            "beryl",
        ],
        env={**os.environ, **bugs.bundle_filing_env()},
    )


@contextmanager
def staging(bundle: Path) -> Iterator[Path]:
    """Replace the previous release only once the new one has been packaged."""
    with tempfile.TemporaryDirectory(
        prefix=".beryl-client-", dir=bundle.parent
    ) as scratch:
        staged = Path(scratch) / bundle.name
        yield staged
        previous = Path(scratch) / "previous"
        if bundle.exists():
            bundle.rename(previous)
        try:
            staged.rename(bundle)
        finally:
            if previous.exists() and not bundle.exists():
                previous.rename(bundle)


def package_macos(bundle: Path) -> None:
    """Build and sign a complete bundle before replacing an earlier release."""
    crate = paths.WORKSPACE / "crates" / "beryl_client"
    manifest = crate / "Cargo.toml"
    version = tomllib.loads(manifest.read_text())["package"]["version"]
    icon = crate / "macos" / "icon.icns"
    with staging(bundle) as staged:
        contents = staged / "Contents"
        executable = contents / "MacOS" / RELEASE_BINARY.name
        executable.parent.mkdir(parents=True)
        shutil.copy(RELEASE_BINARY, executable)
        resources = contents / "Resources"
        shutil.copytree(
            audio.OUTPUT,
            resources / "audio",
            copy_function=shutil.copy,
        )
        shutil.copyfile(icon, resources / icon.name)
        with (contents / "Info.plist").open("wb") as info:
            plistlib.dump(
                {
                    "CFBundleExecutable": executable.name,
                    "CFBundleIconFile": icon.name,
                    "CFBundleIdentifier": "ca.thurn.beryl",
                    "CFBundleInfoDictionaryVersion": "6.0",
                    "CFBundleName": MAC_APP_NAME,
                    "CFBundlePackageType": "APPL",
                    "CFBundleShortVersionString": version,
                    "CFBundleVersion": version,
                    # What the app is, and that macOS's Game Mode may run it.
                    "LSApplicationCategoryType": "public.app-category.card-games",
                    "LSSupportsGameMode": True,
                    "NSHighResolutionCapable": True,
                },
                info,
            )
        subprocess.run(
            ["/usr/bin/codesign", "--force", "--sign", "-", staged], check=True
        )


def package_windows(folder: Path) -> None:
    with staging(folder) as staged:
        staged.mkdir()
        shutil.copy(RELEASE_BINARY, staged)
        shutil.copytree(audio.OUTPUT, staged / "audio")


def run_release(_args: argparse.Namespace) -> int:
    if build_release() != 0:
        return deps.CANNOT_RUN
    output = RELEASE_BINARY
    try:
        with system.file_lock(paths.build_lock()):
            if sys.platform == "darwin":
                output = RELEASE_BINARY.with_name(f"{MAC_APP_NAME}.app")
                package_macos(output)
            elif system.WINDOWS:
                folder = RELEASE_BINARY.with_name(MAC_APP_NAME)
                package_windows(folder)
                output = folder / RELEASE_BINARY.name
    except (OSError, subprocess.CalledProcessError) as error:
        print(f"release: cannot package {output}: {error}", file=sys.stderr)
        return deps.CANNOT_RUN
    print(f"release: {output}")
    return 0


COMMANDS = (
    Command(
        "release",
        "packages the release app on macOS and Windows",
        (
            (
                "just client release",
                "build the optimized desktop app and print its path",
            ),
            ("just client release --help", "this text"),
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
        help="play this script of actions (a name in client/drive), then end",
    )
    parser.add_argument(
        "--load",
        help="start busy threads: <threads>[:utility|default|initiated|interactive]",
    )
    parser.add_argument(
        "--slow-gpu", help="hold each finished frame back this long (5ms)"
    )
    parser.add_argument(
        "--fixed-hz",
        help="ask the macOS display link for this rate from the start and never step (60)",
    )
    parser.add_argument(
        "--agent",
        help="start a new game this agent plays for the viewer; the run ends when it is over",
    )
    parser.add_argument("--opponent", help="--agent's opponent (mcts-hidden)")
    parser.add_argument("--deck", help="--agent's deck (wu-control)")
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
        ("--fixed-hz", args.fixed_hz),
        ("--agent", args.agent),
        ("--opponent", args.opponent),
        ("--deck", args.deck),
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
    env = os.environ.copy()
    if window != "none":
        env.setdefault("BERYL_FILE_BUGS", str(repo_paths.REPO))
    started_ms = int(time.time() * 1000)
    code = subprocess.run(command, env=env, check=False).returncode
    if code == MISSING_PACK and record() == 0:
        code = subprocess.run(command, env=env, check=False).returncode
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
    return smoke(no_build=args.no_build)


def smoke(*, no_build: bool = False) -> int:
    """Starts the app with no window under a profile of its own, which has no game to resume,
    waits for its first shown frame and for its live socket's tree to show the start screen (a
    region named `start`), and quits it. The app ends itself after `SMOKE_LIMIT_S` if it hangs
    before then.
    """
    if not no_build and build() != 0:
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
    profile = f"client-smoke-{os.getpid()}"
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
    # The app's own runtime limit starts after initialization. This also bounds a hang
    # before initialization finishes or while the app is closing.
    watchdog = threading.Timer(SMOKE_LIMIT_S + 30, process.kill)
    watchdog.start()
    drainer: threading.Thread | None = None
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
        drainer = threading.Thread(target=output.read, daemon=True)
        drainer.start()
        wait_for_start(socket_path)
        accepted, reason = live.request(socket_path, "quit")
        if not accepted:
            raise SmokeError(f"the app rejected quit: {reason}")
        if code := process.wait(timeout=30):
            raise SmokeError(f"the app exited with status {code}")
        return shown
    except OSError as error:
        raise SmokeError(f"the live socket did not answer: {error}") from error
    except subprocess.TimeoutExpired as error:
        raise SmokeError("the app did not quit") from error
    finally:
        watchdog.cancel()
        if process.poll() is None:
            process.kill()
            process.wait()
        if drainer is not None:
            drainer.join()
        output.close()
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


def log_lines(path: Path) -> Iterator[dict[str, Any]]:
    with path.open("rb") as stream:
        for line in stream:
            try:
                yield json.loads(line)
            except json.JSONDecodeError:
                # A live writer may still be flushing the final line.
                if line.endswith(b"\n"):
                    raise


def report(path: Path, *, latest: bool = False) -> str:
    """Show the run, its last summary, its late frames by cause and rate, and captured timing
    windows. A log may hold several summaries, written every 30 s, and the last counts.
    """
    if path.is_dir():
        path = path / "frames.jsonl"
    with_rows = log_lines(path)
    first = next(with_rows, None)
    if first is None:
        raise ValueError(f"{path}: the frame log has no complete run line")
    run = first["run"]
    late: list[dict[str, Any]] = []
    summary: dict[str, Any] | None = None
    tier: dict[str, Any] | None = None
    changes: list[str] = []
    windows: list[str] = []
    latest_window: dict[str, Any] | None = None
    # Every timing record the log holds, for the run's median callback offset.
    records: list[dict[str, Any]] = []
    for line in with_rows:
        if "late" in line:
            late.append(line["late"])
        elif "summary" in line:
            summary = line["summary"]
        elif "tier" in line:
            tier = line["tier"]
        elif "pacing_change" in line:
            changes.append(frame_trace.describe_change(line["pacing_change"]))
        elif "timing_window" in line:
            records.extend(
                e["frame"] for e in line["timing_window"]["events"] if "frame" in e
            )
            if latest:
                latest_window = line["timing_window"]
            else:
                windows.extend(frame_trace.describe(line["timing_window"]))
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
    if tier is not None:
        out.append(tier_line(tier))
    if summary is None:
        out.append("no exit summary in the log")
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
        if summary.get("missed_target"):
            out.append(
                f"finished after the display time their tick was for, so shown a refresh late"
                f" though not late above: {summary['missed_target']}"
            )
        dragging = summary.get("dragging", {})
        if dragging.get("frames"):
            out.append(
                f"while a pointer button was held ({dragging['presses']} presses):"
                f" {dragging['frames']} frames, {dragging['late']} late,"
                f" {dragging['missed_target']} finished after their display time"
            )
        if run.get("pause_at_ms") is not None:
            out.append(
                f"paused at {run['pause_at_ms']} ms:"
                f" {summary['after_pause']} frames drawn after the pause"
            )
        for rate in summary.get("rates", []):
            share = 100 * rate["late"] / rate["frames"] if rate["frames"] else 0.0
            out.append(
                f"  {rate['hz']:g} Hz: {rate['seconds']:.1f} s, {rate['frames']} frames,"
                f" {rate['late']} late ({share:.2f}%)"
            )
    out += late_frames(run, late, records)
    if changes:
        out.append("live pacing changes")
        out.extend(changes)
    if latest_window is not None:
        windows = frame_trace.describe(latest_window)
    out.extend(windows)
    return "\n".join(out)


def late_frames(
    run: dict[str, Any], late: list[dict[str, Any]], records: list[dict[str, Any]]
) -> list[str]:
    """The late frames by cause and rate asked for, and the `WORST` longest with their causes.
    A line written before late lines had timing records has the cause `unknown`."""
    if not late:
        return []
    for frame in late:
        records.extend(frame[key] for key in ("frame", "previous") if frame.get(key))
    median = frame_trace.median_offset(records)
    refresh = 1e9 / max(run["display_hz"], 1.0)
    causes = [
        frame_trace.cause(frame, median_offset_ns=median, refresh_ns=refresh)
        for frame in late
    ]
    rates = [frame.get("hz", run["display_hz"]) for frame in late]
    out = ["late frames by cause and rate asked for:"]
    out += frame_trace.cause_table(list(zip(causes, rates)))
    ranked = sorted(zip(late, causes), key=lambda row: -row[0]["since_last_ms"])
    worst = ranked[:WORST]
    out.append(f"worst {len(worst)} of {len(late)} late frames (ms):")
    for frame, why in worst:
        if why == "handler" and (stage := frame_trace.largest_stage(frame["previous"])):
            why = f"handler, most in {stage}"
        out.append(
            f"  at {frame['at_ms']:.0f}: {why}; gap {frame['since_last_ms']:.1f}"
            f" at {frame.get('hz', run['display_hz'])} Hz,"
            f" waited {frame['waited_ms']:.1f}, client {frame['client_ms']:.2f},"
            f" encoded {frame['encoded_ms']:.2f}, searching {frame['searching']},"
            f" verbs {','.join(frame['verbs']) or '-'}"
        )
    return out


def tier_line(tier: dict[str, Any]) -> str:
    """The tier chosen at startup (docs/beryl-3d.md, 'Choosing a tier'), one line."""
    if tier.get("slowest_ms") is None:
        measured = "nothing measured"
    elif tier.get("median_ms") is not None:
        measured = (
            f"median measured frame {tier['median_ms']:.1f} ms,"
            f" slowest {tier['slowest_ms']:.1f} ms"
        )
    else:
        measured = f"slowest measured frame {tier['slowest_ms']:.1f} ms"
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
        "path",
        nargs="?",
        type=Path,
        help="a frame log or bug report folder; the newest log if none",
    )
    parser.add_argument(
        "--latest", action="store_true", help="show only the most recent timing window"
    )


def run_frames(args: argparse.Namespace) -> int:
    path = args.path or newest_log(frames_dir(), repo_paths.REPO.resolve())
    if path is None:
        print(
            f"frames: no frame log in {frames_dir()} names this checkout;"
            " `just client app` writes one",
            file=sys.stderr,
        )
        return 1
    print(report(path, latest=args.latest))
    return 0
