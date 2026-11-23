"""The phone apps: `mobile-setup` installs their toolchains, `ios` builds the app for the iOS
Simulator and launches it there, `ios-smoke` does the same headless until the UI reports that
it loaded, and `ios-device` builds, signs and installs it on a paired iPhone. The Android
recipes are `android`'s."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from . import android_sdk, app, mobile_sdk, paths, system
from .mobile_sdk import (
    ANDROID_HOME as ANDROID_HOME,
    ANDROID_SYSTEM_IMAGE as ANDROID_SYSTEM_IMAGE,
    ANDROID_TARGETS,
    SIMULATOR as SIMULATOR,
    alive,
    boot,
    install,
    paired_device as paired_device,
    run as run,
    simulator,
)

BUNDLE_ID = "ca.thurn.beryl"
APPLE: Path = paths.APP / "gen" / "apple"
# What lib.rs's `ui_ready` journals in smoke mode on a phone.
READY = "ui ready"


def check(args: argparse.Namespace) -> int:
    """Type-checks the phone targets supported by this host (`by mobile-check`)."""
    if sys.platform not in ("darwin", "win32"):
        print("mobile-check: supported on macOS and Windows", file=sys.stderr)
        return 2
    manifest = str(paths.RUST / "Cargo.toml")
    targets: tuple[str, ...] = (
        ("aarch64-apple-ios-sim", *ANDROID_TARGETS)
        if sys.platform == "darwin"
        else ANDROID_TARGETS
    )
    for target in targets:
        started = time.monotonic()
        # From ui/, where Xcode's and Gradle's build scripts run cargo, so this
        # resolves the toolchain the way the phone builds do.
        code = subprocess.run(
            [
                "cargo",
                "check",
                "--locked",
                "--manifest-path",
                manifest,
                "-p",
                "beryl_app",
                "--target",
                target,
            ],
            cwd=paths.UI,
            env=android_sdk.ndk_env() if target == android_sdk.TARGET else None,
            check=False,
        ).returncode
        print(f"mobile-check: {target} in {time.monotonic() - started:.0f} s")
        if code != 0:
            print("mobile-check: missing targets or NDK? `just mobile-setup`")
            return code
    return 0


def android_env() -> dict[str, str]:
    return mobile_sdk.android_env(paths.tauri_env())


def legacy_simulator_ready() -> bool:
    return mobile_sdk.can_build_for_simulator(
        APPLE / "beryl_app.xcodeproj", "beryl_app_iOS"
    )


def setup(args: argparse.Namespace) -> int:
    if system.WINDOWS:
        return mobile_sdk.android_setup()
    if sys.platform != "darwin":
        print("mobile-setup: supported on macOS and Windows", file=sys.stderr)
        return 2
    return (
        mobile_sdk.ios_setup(("cocoapods", "xcodegen"), legacy_simulator_ready)
        or mobile_sdk.android_setup()
    )


def build_simulator_app() -> int:
    """Builds the debug app for the iOS Simulator through Tauri's CLI ([`SIMULATOR_APP`])."""
    started = time.monotonic()
    code = app.tauri(
        "ios", "build", "--debug", "--target", "aarch64-sim", "--features", "iroh"
    )
    print(f"ios: built in {time.monotonic() - started:.0f} s")
    return code


# Where `tauri ios build --target aarch64-sim` leaves the simulator app.
SIMULATOR_APP: Path = APPLE / "build" / "arm64-sim" / "Beryl.app"
# And where `tauri ios build --target aarch64` leaves the device app's archive.
DEVICE_IPA: Path = APPLE / "build" / "arm64" / "Beryl.ipa"


def ios(args: argparse.Namespace) -> int:
    if not args.no_build:
        code = build_simulator_app()
        if code != 0:
            return code
    udid, _ = simulator()
    boot(udid)
    install(udid, SIMULATOR_APP)
    if not paths.is_agent():
        subprocess.run(
            ["open", "-a", "Simulator", "--args", "-CurrentDeviceUDID", udid]
        )
    subprocess.run(
        [
            "xcrun",
            "simctl",
            "launch",
            "--terminate-running-process",
            udid,
            BUNDLE_ID,
        ],
        check=True,
    )
    print(f"ios: launched on {SIMULATOR} ({udid})")
    return 0


def smoke(args: argparse.Namespace) -> int:
    if not args.no_build:
        code = build_simulator_app()
        if code != 0:
            return code
    # One smoke at a time: another would reinstall the app under this one, or shut down the
    # simulator it booted.
    with system.file_lock(paths.cache_root() / "ios-smoke.lock"):
        udid, booted = simulator()
        started = time.monotonic()
        boot(udid)
        try:
            install(udid, SIMULATOR_APP)
            print(
                f"ios-smoke: booted and installed in {time.monotonic() - started:.0f} s"
            )
            ready = launch_until_ready(udid, args.seconds)
        finally:
            run("xcrun", "simctl", "terminate", udid, BUNDLE_ID)
            if not booted:
                run("xcrun", "simctl", "shutdown", udid)
    print(f"ios-smoke: {'passed' if ready else 'FAILED'}")
    return 0 if ready else 1


def launch_until_ready(udid: str, seconds: int) -> bool:
    """Launches the app in smoke mode and reports whether its journal says the UI loaded.
    Smoke mode exits the app then, or after `seconds` without. The app's stdout goes to the
    system log, not to simctl, so its journal is what tells."""
    # simctl hands SIMCTL_CHILD_-prefixed variables to the app without the prefix.
    env = {**os.environ, "SIMCTL_CHILD_BERYL_SMOKE": str(seconds)}
    launched = subprocess.run(
        ["xcrun", "simctl", "launch", "--terminate-running-process", udid, BUNDLE_ID],
        env=env,
        capture_output=True,
        text=True,
        check=True,
    )
    # "ca.thurn.beryl: 1234": a simulator's apps are this Mac's processes.
    pid = int(launched.stdout.rsplit(":", 1)[1])
    # The app's own deadline ends it; this one catches an app that never exits.
    deadline = time.monotonic() + seconds + 30
    while alive(pid) and time.monotonic() < deadline:
        time.sleep(0.25)
    if alive(pid):
        print(f"ios-smoke: the app appears hung, it did not exit in {seconds + 30} s")
    records = journal(udid, pid)
    return smoke_result(records, "ios-smoke")


def smoke_result(records: list[dict[str, Any]], label: str) -> bool:
    """Prints snapshot timing and errors from one mobile smoke run."""
    for record in records:
        message = record.get("message", "")
        if record.get("level") in ("warn", "error") or message.startswith(
            "window snapshot in "
        ):
            print(f"{label}: {record.get('source')}: {message}")
    return any(r.get("message") == READY for r in records)


def journal(udid: str, pid: int) -> list[dict[str, Any]]:
    """The journal records of the app run `pid` (beryl_session's `journal`) in the
    simulator."""
    data = run("xcrun", "simctl", "get_app_container", udid, BUNDLE_ID, "data")
    path = (
        Path(data.stdout.strip())
        / "Library"
        / "Application Support"
        / BUNDLE_ID
        / "logs"
        / "beryl.jsonl"
    )
    if data.returncode != 0 or not path.exists():
        return []
    return run_records(path.read_text(), pid)


def run_records(journal: str, pid: int) -> list[dict[str, Any]]:
    """The records of the app run `pid` in `journal`'s text: those after the last record that
    opened the journal in that process. The last: an emulator booted afresh gives the app the
    same pid it gave an earlier run."""
    records: list[dict[str, Any]] = []
    ours = False
    for line in journal.splitlines():
        try:
            record: dict[str, Any] = json.loads(line)
        except json.JSONDecodeError:
            # A killed app can leave a torn or NUL-filled journal line behind.
            continue
        if record.get("source") == "journal" and record.get("message") == "opened":
            ours = (record.get("data") or {}).get("pid") == pid
            if ours:
                records = []
        if ours:
            records.append(record)
    return records


def device(args: argparse.Namespace) -> int:
    install = not (args.no_install or args.no_sign)
    if install and paths.is_agent() and not args.user_asked:
        # Agents verify in the simulator. They install to the user's phone only when the
        # user has asked for it in chat, and say so with --user-asked.
        print(
            "ios-device: agents install to the phone only when the user asks"
            " (--user-asked); otherwise use --no-install or --no-sign",
            file=sys.stderr,
        )
        return 2
    started = time.monotonic()
    sign = ["--no-sign"] if args.no_sign else []
    code = app.tauri("ios", "build", "--target", "aarch64", "--features", "iroh", *sign)
    print(f"ios-device: built in {time.monotonic() - started:.0f} s")
    if code != 0:
        if not args.no_sign:
            print(SIGNING_HELP, file=sys.stderr)
        return code
    print(f"ios-device: {DEVICE_IPA}")
    if not install:
        return 0
    udid = paired_device()
    if udid is None:
        print("ios-device: no iPhone is paired with this Mac", file=sys.stderr)
        return 1
    return subprocess.run(
        [
            "xcrun",
            "devicectl",
            "device",
            "install",
            "app",
            "--device",
            udid,
            str(DEVICE_IPA),
        ],
        check=False,
    ).returncode


SIGNING_HELP = """ios-device: if signing failed, once on this Mac:
  1. Xcode > Settings > Accounts: sign in with the Apple ID of team D77745XTVL.
  2. Connect the iPhone, trust this Mac, and turn on Settings > Privacy & Security >
     Developer Mode on the phone.
then run `just ios-device` again."""


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    sub.add_parser(
        "mobile-setup", help="install the phone apps' toolchains (idempotent)"
    ).set_defaults(func=setup)
    sub.add_parser(
        "mobile-check", help="cargo check the phone targets supported by this host"
    ).set_defaults(func=check)
    p = sub.add_parser("ios", help="build the iOS app and launch it in the Simulator")
    p.add_argument("--no-build", action="store_true")
    p.set_defaults(func=ios)
    p = sub.add_parser(
        "ios-smoke", help="build the iOS app and launch it headless until the UI loads"
    )
    p.add_argument(
        "--seconds",
        type=int,
        default=600,
        help="treat the app as hung after this long; a healthy start exits much sooner",
    )
    p.add_argument("--no-build", action="store_true")
    p.set_defaults(func=smoke)
    p = sub.add_parser(
        "ios-device",
        help="build and sign the iOS app and install it on a paired iPhone",
    )
    p.add_argument("--no-install", action="store_true", help="build and sign only")
    p.add_argument(
        "--no-sign", action="store_true", help="build only, unsigned (no Xcode login)"
    )
    p.add_argument(
        "--user-asked",
        action="store_true",
        help="let an agent install: the user asked in chat for the app on their phone",
    )
    p.set_defaults(func=device)
