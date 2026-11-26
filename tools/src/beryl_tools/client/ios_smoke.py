"""Replay a recording in the headless iOS Simulator and check the host's frame log."""

import argparse
import os
import shutil
import subprocess
import time
from collections.abc import Callable
from pathlib import Path

from .. import mobile_sdk as mobile
from .. import paths as repo_paths, system
from .phone import (
    BUNDLE,
    ERRORS,
    FIXTURE,
    LIBRARY_LOG,
    finish,
    ios_app,
    out_dir,
    run_name,
    wait_for_log,
    write_fixtures,
)
from .phone_log import lifecycle_faults, logged_inputs, suspension_bounds

HOST_LOG = "host.log"


def ios_smoke_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--fixture", default=FIXTURE, help="the recording to replay")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--lifecycle",
        action="store_true",
        help="suspend and resume a replay, then exercise low memory and platform requests",
    )
    mode.add_argument(
        "--touch",
        action="store_true",
        help="run the UI test that touches, rotates and idles a live game instead of a replay",
    )
    mode.add_argument(
        "--game",
        choices=("wu-control", "forgetful-fish"),
        help="play a whole seeded game of this deck against the AI and check its result",
    )


def run_ios_smoke(args: argparse.Namespace) -> int:
    if args.game:
        from .ios_game import run_ios_game

        return run_ios_game(args.game)
    if args.touch:
        from .ios_touch import run_ios_touch

        return run_ios_touch()
    label = "ios-smoke --lifecycle" if args.lifecycle else "ios-smoke"
    bundle = ios_app("iphonesimulator", release=False, sign=False)
    run = out_dir("phone") / f"{run_name()}-ios"
    run.mkdir()
    (fixture,) = write_fixtures((args.fixture,), run)
    # Share the simulator and lock with the web app's `just ios-smoke` and the touch smoke.
    with system.file_lock(repo_paths.cache_root() / "ios-smoke.lock"):
        udid, booted = mobile.simulator()
        mobile.boot(udid)
        try:
            mobile.install(udid, bundle)
            container = simctl(udid, "get_app_container", BUNDLE, "data")
            documents: Path = Path(container.strip()) / "Documents"
            documents.mkdir(parents=True, exist_ok=True)
            for old in [
                *documents.glob("*.jsonl"),
                documents / ERRORS,
                documents / LIBRARY_LOG,
                documents / HOST_LOG,
            ]:
                old.unlink(missing_ok=True)
            shutil.copyfile(fixture, documents / fixture.name)
            launched = subprocess.run(
                [
                    *["xcrun", "simctl", "launch", "--terminate-running-process"],
                    *[udid, BUNDLE],
                ],
                env={**os.environ, "SIMCTL_CHILD_BERYL_CLIENT_FIXTURE": args.fixture},
                capture_output=True,
                text=True,
                check=True,
            )
            pid = int(launched.stdout.rsplit(":", 1)[1])

            def read() -> str | None:
                logs = sorted(documents.glob("*.jsonl"))
                return logs[-1].read_text(encoding="utf-8") if logs else None

            def errors() -> str:
                error = read_if_present(documents / ERRORS)
                return error + read_if_present(documents / LIBRARY_LOG) if error else ""

            def host_log() -> str:
                return read_if_present(documents / HOST_LOG)

            log: str | None = None
            faults: list[str] = []
            if args.lifecycle:
                faults = exercise_lifecycle(udid, pid, read, errors, host_log, label)
            else:
                log = wait_for_log(read, lambda: mobile.alive(pid), errors, label)
            png = run / "screen.png"
            simctl(udid, "io", "screenshot", str(png))
            if args.lifecycle:
                if not faults:
                    post_notification(udid, "open-url")
                    if not wait_for_request(host_log, "open-url", pid):
                        faults.append("the host did not open the URL")
                host = host_log()
                (run / HOST_LOG).write_text(host, encoding="utf-8")
                print(f"{label}: host log {run / HOST_LOG}")
                log = read()
                faults.extend(lifecycle_faults(log or "", host))
                if reported := errors():
                    faults.append(reported)
        finally:
            mobile.run("xcrun", "simctl", "terminate", udid, BUNDLE)
            if not booted:
                mobile.run("xcrun", "simctl", "shutdown", udid)
    if faults:
        print(f"{label}: screen {png}")
        if log is not None:
            saved = png.with_suffix(".jsonl")
            saved.write_text(log, encoding="utf-8")
            print(f"{label}: frames {saved}")
        for fault in faults:
            print(f"{label}: FAILED: {fault}")
        return 1
    return finish(label, log, png)


def read_if_present(path: Path) -> str:
    return path.read_text(encoding="utf-8") if path.exists() else ""


def simctl(udid: str, command: str, *args: str) -> str:
    result = mobile.run("xcrun", "simctl", command, udid, *args)
    if result.returncode:
        raise SystemExit(f"simctl {command} failed: {result.stderr or result.stdout}")
    return result.stdout


def post_notification(udid: str, event: str) -> None:
    simctl(udid, "spawn", "notifyutil", "-p", f"{BUNDLE}.test.{event}")


def wait_for_request(read: Callable[[], str], event: str, pid: int) -> bool:
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline and mobile.alive(pid):
        for line in read().splitlines():
            if line == event:
                return True
            if line.startswith(f"{event} failed:"):
                print(line)
                return False
        time.sleep(0.25)
    return False


def exercise_lifecycle(
    udid: str,
    pid: int,
    read: Callable[[], str | None],
    errors: Callable[[], str],
    host_log: Callable[[], str],
    label: str,
) -> list[str]:
    def alive() -> bool:
        return mobile.alive(pid)

    if wait_for_log(read, alive, errors, label, event="first_shown_ms") is None:
        return ["the first frame was never shown"]

    simctl(udid, "launch", "com.apple.Preferences")
    if (
        wait_for_log(
            read,
            alive,
            errors,
            label,
            event="suspension and surface loss",
            ready=lambda log: any(
                event["event"] == "Suspended"
                for event in logged_inputs(log, "lifecycle")
            )
            and "surface-lost" in host_log().splitlines(),
        )
        is None
    ):
        return ["Settings did not suspend Beryl or release its surface"]
    # Leave Beryl in the background long enough for iOS to suspend the process.
    time.sleep(3)
    resumed = simctl(udid, "launch", BUNDLE)
    if int(resumed.rsplit(":", 1)[1]) != pid:
        return ["Beryl started a new process instead of resuming the replay"]

    if (
        wait_for_log(
            read,
            alive,
            errors,
            label,
            event="resume",
            ready=lambda log: suspension_bounds(log) is not None,
        )
        is None
    ):
        return ["the foregrounded process did not resume"]

    post_notification(udid, "memory-warning")
    for event in ("haptic", "share"):
        post_notification(udid, event)
        if not wait_for_request(host_log, event, pid):
            return [f"the host did not carry out {event}"]

    if wait_for_log(read, alive, errors, label) is None:
        return ["the replay did not finish"]
    return []
