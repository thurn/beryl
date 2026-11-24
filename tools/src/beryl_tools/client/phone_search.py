"""`just client phone-search`: whether the AI's search starves a phone's frame thread
(docs/beryl-3d.md, "Threads"). It plays one live game against `mcts-hidden` in the headless iOS
Simulator and then in the Android emulator (on Vulkan, and on OpenGL ES 3.0 too with `--gles`),
with the stand-in agent `greedy` answering the person's decisions so nobody has to play. After
5 minutes, or when the game ends, it suspends the app, which makes the app write its frame
log's summary, pulls the log and prints the frames drawn while the AI searched and while it did
not, how many of each were late, and the Mac's load.

A simulator or emulator draws in software on a loaded Mac, so its numbers say nothing about a
phone's speed. What they do show is the difference between frames drawn while the search runs
on every core and frames drawn while it does not. This is a measurement, gated on nothing and not part of any automatic check.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import time
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .. import mobile_sdk as mobile
from .. import paths as repo_paths, system
from . import paths, phone
from .phone_log import lines

# The agent that plays the person's seat, and how long a game may run before it is cut off.
STAND_IN = "greedy"
PLAY_S = 300
# How long the app may take to write its summary once suspended.
SUMMARY_LIMIT_S = 120
# A late share while searching above this many times the share while not counts as
# starvation.
STARVED_RATIO = 2.0


@dataclass(frozen=True)
class Numbers:
    """One platform's frames, from its frame log's last summary and its late lines."""

    adapter: str
    seconds: float
    frames: int
    searching_frames: int
    searching_late: int
    idle_frames: int
    idle_late: int
    worst_gap_searching_ms: float | None
    worst_gap_ms: float
    # The Mac's load average over one minute when the app started, from the log's first line.
    load: float | None

    @property
    def starved(self) -> bool:
        """Whether late frames while searching were over twice the share of late frames while
        not. With no frame drawn while searching, or none while not, nothing compares.
        """
        if not self.searching_frames or not self.idle_frames:
            return False
        searching = self.searching_late / self.searching_frames
        idle = self.idle_late / self.idle_frames
        return searching > STARVED_RATIO * idle

    def table(self, label: str) -> str:
        def share(late: int, frames: int) -> str:
            return f"{late} late ({100 * late / frames:.1f}%)" if frames else "none"

        worst = self.worst_gap_searching_ms
        load = "unknown" if self.load is None else f"{self.load:.1f}"
        return "\n".join(
            [
                f"{label}: adapter {self.adapter}, {self.seconds:.0f} s, load {load} at start",
                f"{label}: frames {self.frames}",
                f"{label}: while searching {self.searching_frames} frames,"
                f" {share(self.searching_late, self.searching_frames)}",
                f"{label}: while not {self.idle_frames} frames,"
                f" {share(self.idle_late, self.idle_frames)}",
                f"{label}: worst gap while searching "
                + ("none late" if worst is None else f"{worst:.1f} ms")
                + f", worst gap {self.worst_gap_ms:.1f} ms",
                f"{label}: "
                + (
                    "STARVED: late frames while searching over twice the share while not"
                    if self.starved
                    else "not starved"
                ),
            ]
        )


def numbers(parsed: list[dict[str, Any]]) -> Numbers | None:
    """The numbers of a frame log's lines, or None when it holds no summary."""
    summaries = [line["summary"] for line in parsed if "summary" in line]
    if not summaries or not parsed or "run" not in parsed[0]:
        return None
    run: dict[str, Any] = parsed[0]["run"]
    summary: dict[str, Any] = summaries[-1]
    searching: dict[str, Any] = summary.get("searching", {})
    gaps = [
        line["late"]["since_last_ms"]
        for line in parsed
        if "late" in line and line["late"]["searching"]
    ]
    load = run.get("load")
    frames = int(summary["frames"])
    searching_frames = int(searching.get("frames", 0))
    searching_late = int(searching.get("late", 0))
    return Numbers(
        adapter=str(run.get("adapter", "")),
        seconds=float(summary["seconds"]),
        frames=frames,
        searching_frames=searching_frames,
        searching_late=searching_late,
        idle_frames=frames - searching_frames,
        idle_late=int(summary["late"]) - searching_late,
        worst_gap_searching_ms=max(gaps) if gaps else None,
        worst_gap_ms=float(summary["worst_gap_ms"]),
        load=float(load[0]) if load else None,
    )


def summary_count(text: str | None) -> int:
    return sum("summary" in line for line in lines(text)) if text else 0


def play(
    read: Callable[[], str | None],
    alive: Callable[[], bool],
    errors: Callable[[], str],
    suspend: Callable[[], None],
    label: str,
    play_s: float = PLAY_S,
    pause: Callable[[float], None] = time.sleep,
    clock: Callable[[], float] = time.monotonic,
) -> tuple[str | None, str | None]:
    """Lets the game run for `play_s`, or until the app writes the summary it writes when the
    game ends, then suspends the app and waits for the summary that follows. Returns the log and
    the fault that stopped the run, if one did."""
    started = clock()
    adapter_named = False
    while clock() - started < play_s:
        text = read()
        parsed = lines(text) if text else []
        if parsed and not adapter_named and "run" in parsed[0]:
            adapter_named = True
            print(f"{label}: adapter {parsed[0]['run']['adapter']}", flush=True)
        if summary_count(text):
            print(f"{label}: the game ended after {clock() - started:.0f} s")
            break
        if reported := errors():
            return text, f"the app reported an error:\n{reported}"
        if not alive():
            return text, errors() or "the app exited and wrote no error"
        pause(5)
    before = summary_count(read())
    suspend()
    deadline = clock() + SUMMARY_LIMIT_S
    while clock() < deadline:
        text = read()
        if summary_count(text) > before:
            return text, None
        pause(1)
    return read(), f"no summary {SUMMARY_LIMIT_S} s after the app was suspended"


def run_ios(run: Path) -> tuple[str | None, str | None]:
    bundle = phone.ios_app("iphonesimulator", release=False, sign=False)
    with system.file_lock(repo_paths.cache_root() / "ios-smoke.lock"):
        simulator, booted = mobile.simulator()
        udid: str = simulator
        mobile.boot(udid)
        try:
            mobile.install(udid, bundle)
            container = mobile.run(
                "xcrun", "simctl", "get_app_container", udid, phone.BUNDLE, "data"
            )
            documents: Path = Path(container.stdout.strip()) / "Documents"
            documents.mkdir(parents=True, exist_ok=True)
            for old in [
                *documents.glob("*.jsonl"),
                documents / phone.ERRORS,
                documents / phone.LIBRARY_LOG,
            ]:
                old.unlink(missing_ok=True)
            # The live game's replays and ledger, so that the app starts a fresh game.
            shutil.rmtree(documents / "logs", ignore_errors=True)
            launched = subprocess.run(
                [
                    *["xcrun", "simctl", "launch", "--terminate-running-process"],
                    *[udid, phone.BUNDLE],
                ],
                env={**os.environ, "SIMCTL_CHILD_BERYL_CLIENT_STAND_IN": STAND_IN},
                capture_output=True,
                text=True,
                check=True,
            )
            pid = int(launched.stdout.rsplit(":", 1)[1])

            def read() -> str | None:
                logs = sorted(documents.glob("*.jsonl"))
                return logs[-1].read_text(encoding="utf-8") if logs else None

            def errors() -> str:
                path = documents / phone.ERRORS
                return path.read_text(encoding="utf-8") if path.exists() else ""

            def suspend() -> None:
                # Opening another app sends the client to the background.
                mobile.run("xcrun", "simctl", "launch", udid, "com.apple.Preferences")

            log, fault = play(read, lambda: mobile.alive(pid), errors, suspend, "ios")
            mobile.run(
                "xcrun", "simctl", "io", udid, "screenshot", str(run / "ios.png")
            )
        finally:
            mobile.run("xcrun", "simctl", "terminate", udid, phone.BUNDLE)
            if not booted:
                mobile.run("xcrun", "simctl", "shutdown", udid)
    return log, fault


def run_android(gles: bool) -> tuple[str | None, str | None]:
    label = "android-gles" if gles else "android"
    with system.file_lock(paths.cache() / "android-phone.lock"):
        apk = phone.android_apk()
        if phone.adb("get-state").stdout.strip() == "device":
            # A run that was killed left its emulator behind, booted with options this run
            # may not want.
            phone.adb("emu", "kill")
            time.sleep(5)
        emulator = phone.start_emulator(gles)
        try:
            installed = phone.adb("install", "--no-streaming", "-r", str(apk))
            if installed.returncode != 0:
                raise SystemExit(f"{label}: install failed: {installed.stderr}")
            shell: list[str] = ["shell", "run-as", phone.BUNDLE, "sh", "-c"]
            phone.adb(*shell, "'rm -rf files/*.jsonl files/logs files/*.log'")

            def read() -> str | None:
                text = phone.adb(*shell, "'cat files/*.jsonl 2>/dev/null'").stdout
                return text or None

            def alive() -> bool:
                return bool(phone.adb("shell", "pidof", phone.BUNDLE).stdout.strip())

            def errors() -> str:
                errors_file = f"files/{phone.ERRORS}"
                text = phone.adb(*shell, f"'cat {errors_file} 2>/dev/null'").stdout
                crash = phone.adb("logcat", "-d", "-b", "crash").stdout
                return "\n".join(part for part in (text, crash[:4000]) if part)

            def suspend() -> None:
                phone.adb_checked("shell", "input", "keyevent", "KEYCODE_HOME")

            phone.adb_checked(
                *["shell", "am", "start", "-W", "-S", "-n", phone.ACTIVITY],
                *["--es", "standIn", STAND_IN],
            )
            return play(read, alive, errors, suspend, label)
        finally:
            phone.adb("emu", "kill")
            emulator.wait(timeout=phone.REPLAY_LIMIT_S)


def phone_search_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--gles",
        action="store_true",
        help="after Vulkan, run the Android emulator again without Vulkan (OpenGL ES 3.0)",
    )


def run_phone_search(args: argparse.Namespace) -> int:
    run = phone.out_dir("phone") / f"{phone.run_name()}-search"
    run.mkdir()
    print(f"phone-search: frame logs in {run}")
    one, five, fifteen = os.getloadavg()
    print(
        f"phone-search: Mac load average {one:.1f} {five:.1f} {fifteen:.1f} at the start"
    )
    platforms: list[tuple[str, Callable[[], tuple[str | None, str | None]]]] = [
        ("ios", lambda: run_ios(run)),
        ("android", lambda: run_android(False)),
    ]
    if args.gles:
        platforms.append(("android-gles", lambda: run_android(True)))
    failed = False
    for label, play_on in platforms:
        log, fault = play_on()
        if log:
            (run / f"{label}.jsonl").write_text(log, encoding="utf-8")
        found = numbers(lines(log)) if log else None
        if found is not None:
            print(found.table(label))
        if fault or found is None:
            print(f"{label}: FAILED: {fault or 'the frame log holds no summary'}")
            failed = True
    one, five, fifteen = os.getloadavg()
    print(
        f"phone-search: Mac load average {one:.1f} {five:.1f} {fifteen:.1f} at the end"
    )
    return 1 if failed else 0
