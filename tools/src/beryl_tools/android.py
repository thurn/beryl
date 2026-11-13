"""The Android app: `android` builds it and launches it in the Beryl emulator, `android-smoke`
does the same headless until the UI reports that it loaded. `mobile-setup` installs the SDK.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import time
from pathlib import Path
from typing import Any

from . import mobile, paths, system, warm

PACKAGE: str = mobile.BUNDLE_ID
ACTIVITY: str = f"{PACKAGE}/.MainActivity"
# The emulator the recipes create and use, on a port of its own so they never disturb one of
# the user's.
AVD = "beryl_phone"
AVD_DEVICE = "pixel_8"
PORT = 5584
SERIAL = f"emulator-{PORT}"
# lib.rs's SMOKE_PROPERTY: an Android app inherits no environment.
SMOKE_PROPERTY = "debug.beryl.smoke"
# A cold boot takes about a minute; this only catches one that never finishes.
BOOT_SECONDS = 600
# Where `tauri android build --debug --apk --target aarch64` leaves the app.
APK: Path = (
    paths.APP
    / "gen"
    / "android"
    / "app"
    / "build"
    / "outputs"
    / "apk"
    / "universal"
    / "debug"
    / "app-universal-debug.apk"
)


def tool(*parts: str) -> str:
    return str(mobile.ANDROID_HOME.joinpath(*parts))


def adb(*args: str, check: bool = False) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [tool("platform-tools", "adb"), "-s", SERIAL, *args],
        capture_output=True,
        text=True,
        check=check,
    )


def build() -> int:
    """Builds the debug app for arm64 through Tauri's CLI ([`APK`])."""
    warm.ensure(tauri=True)
    started = time.monotonic()
    code = subprocess.run(
        [
            "npx",
            "--no-install",
            "tauri",
            "android",
            "build",
            "--debug",
            "--apk",
            "--target",
            "aarch64",
        ],
        cwd=paths.UI,
        env=mobile.android_env(),
        check=False,
    ).returncode
    print(f"android: built in {time.monotonic() - started:.0f} s")
    return code


def ensure_avd() -> None:
    listed = subprocess.run(
        [tool("emulator", "emulator"), "-list-avds"],
        capture_output=True,
        text=True,
        check=True,
    )
    if AVD in listed.stdout.split():
        return
    subprocess.run(
        [
            tool("cmdline-tools", "latest", "bin", "avdmanager"),
            "create",
            "avd",
            "--name",
            AVD,
            "--package",
            mobile.ANDROID_SYSTEM_IMAGE,
            "--device",
            AVD_DEVICE,
        ],
        input="no\n",  # No custom hardware profile.
        text=True,
        env=mobile.android_env(),
        stdout=subprocess.DEVNULL,
        check=True,
    )


def running() -> bool:
    return adb("get-state").stdout.strip() == "device"


def start_emulator(window: bool) -> subprocess.Popen[bytes]:
    """Starts the Beryl emulator, headless unless `window`, and waits until it has booted."""
    ensure_avd()
    started = time.monotonic()
    headless = [] if window else ["-no-window", "-no-audio", "-no-boot-anim"]
    proc = subprocess.Popen(
        [tool("emulator", "emulator"), "-avd", AVD, "-port", str(PORT), *headless],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        env=mobile.android_env(),
    )
    # Not `adb wait-for-device`, which waits forever on an emulator that died, as one does
    # when another emulator runs its AVD.
    while adb("shell", "getprop", "sys.boot_completed").stdout.strip() != "1":
        if proc.poll() is not None:
            raise SystemExit(f"android: the emulator exited ({proc.returncode})")
        if time.monotonic() - started > BOOT_SECONDS:
            proc.kill()
            raise SystemExit(f"android: the emulator did not boot in {BOOT_SECONDS} s")
        time.sleep(1)
    print(f"android: emulator booted in {time.monotonic() - started:.0f} s")
    return proc


def install() -> None:
    # The emulator rejects streamed installs of this large APK intermittently.
    adb("install", "--no-streaming", "-r", str(APK), check=True)


def android(args: argparse.Namespace) -> int:
    if not args.no_build:
        code = build()
        if code != 0:
            return code
    if not running():
        # Agents never open a window: theirs runs headless, driven over adb.
        start_emulator(window=not paths.is_agent())
    install()
    adb("shell", "am", "start", "-W", "-S", "-n", ACTIVITY, check=True)
    print(f"android: launched on {AVD} ({SERIAL})")
    return 0


def smoke(args: argparse.Namespace) -> int:
    if not args.no_build:
        code = build()
        if code != 0:
            return code
    # One smoke at a time: another would reinstall the app under this one, or kill the
    # emulator it started.
    with system.file_lock(paths.cache_root() / "android-smoke.lock"):
        emulator = None if running() else start_emulator(window=False)
        try:
            install()
            ready = launch_until_ready(args.seconds)
        finally:
            adb("shell", "setprop", SMOKE_PROPERTY, "''")
            adb("shell", "am", "force-stop", PACKAGE)
            if emulator is not None:
                adb("emu", "kill")
                emulator.wait(timeout=BOOT_SECONDS)
    print(f"android-smoke: {'passed' if ready else 'FAILED'}")
    return 0 if ready else 1


def launch_until_ready(seconds: int) -> bool:
    """Launches the app in smoke mode and reports whether its journal says the UI loaded.
    Smoke mode exits the app then, or after `seconds` without."""
    adb("shell", "setprop", SMOKE_PROPERTY, str(seconds), check=True)
    adb("shell", "am", "start", "-W", "-S", "-n", ACTIVITY, check=True)
    pid = adb("shell", "pidof", PACKAGE).stdout.strip()
    if not pid:
        print("android-smoke: the app did not start")
        return False
    # The app's own deadline ends it; this one catches an app that never exits.
    deadline = time.monotonic() + seconds + 30
    while adb("shell", "pidof", PACKAGE).stdout.strip() == pid:
        if time.monotonic() > deadline:
            print(
                f"android-smoke: the app appears hung, it did not exit in {seconds + 30} s"
            )
            break
        time.sleep(0.25)
    records = journal(int(pid))
    return mobile.smoke_result(records, "android-smoke")


def journal(pid: int) -> list[dict[str, Any]]:
    """The journal records of the app run `pid`, read from the app's data through `run-as`
    (debug builds allow it)."""
    found = adb("shell", "run-as", PACKAGE, "find", ".", "-name", "beryl.jsonl")
    path = found.stdout.strip().splitlines()[:1]
    if not path:
        return []
    text = adb("exec-out", "run-as", PACKAGE, "cat", path[0]).stdout
    return mobile.run_records(text, pid)


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "android", help="build the Android app and launch it in the Beryl emulator"
    )
    p.add_argument("--no-build", action="store_true")
    p.set_defaults(func=android)
    p = sub.add_parser(
        "android-smoke",
        help="build the Android app and launch it headless until the UI loads",
    )
    p.add_argument(
        "--seconds",
        type=int,
        default=600,
        help="treat the app as hung after this long; a healthy start exits much sooner",
    )
    p.add_argument("--no-build", action="store_true")
    p.set_defaults(func=smoke)
