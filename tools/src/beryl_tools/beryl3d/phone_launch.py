"""Build and launch the native client on the simulator or emulator."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

from .. import mobile_sdk, paths as repo_paths, system
from . import paths, phone, phone_build
from .command import Command
from .phone_build import IOS_SIMULATOR


def build_only(_args: argparse.Namespace) -> int:
    if sys.platform == "darwin":
        print(
            f"ios-build: {phone_build.ios_app(IOS_SIMULATOR, release=False, sign=False)}"
        )
    elif not system.WINDOWS:
        raise SystemExit("mobile-build: supported on macOS and Windows")
    print(f"android-build: {phone_build.android_apk()}")
    return 0


def ios_build(_args: argparse.Namespace) -> int:
    print(f"ios-build: {phone_build.ios_app(IOS_SIMULATOR, release=False, sign=False)}")
    return 0


def android_build(_args: argparse.Namespace) -> int:
    print(f"android-build: {phone_build.android_apk()}")
    return 0


def setup(_args: argparse.Namespace) -> int:
    if system.WINDOWS:
        return mobile_sdk.android_setup()
    if sys.platform != "darwin":
        raise SystemExit("mobile-setup: supported on macOS and Windows")
    return mobile_sdk.ios_setup(("xcodegen",), phone_build.simulator_ready) or (
        mobile_sdk.android_setup()
    )


def no_build(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--no-build", action="store_true", help="use the last build")


def require_artifact(artifact: Path, command: str) -> None:
    if not artifact.exists():
        raise SystemExit(f"{artifact} is missing; run `just b3d {command}`")


def ios(args: argparse.Namespace) -> int:
    bundle = (
        phone_build.ios_bundle(IOS_SIMULATOR)
        if args.no_build
        else phone_build.ios_app(IOS_SIMULATOR, release=False, sign=False)
    )
    require_artifact(bundle, "ios-build")
    with system.file_lock(repo_paths.cache_root() / "ios-smoke.lock"):
        udid, _ = mobile_sdk.simulator()
        mobile_sdk.boot(udid)
        mobile_sdk.install(udid, bundle)
        subprocess.run(
            [
                "xcrun",
                "simctl",
                "launch",
                "--terminate-running-process",
                udid,
                phone.BUNDLE,
            ],
            check=True,
        )
        if not repo_paths.is_agent():
            subprocess.run(
                ["open", "-a", "Simulator", "--args", "-CurrentDeviceUDID", udid],
                check=True,
            )
    print(f"ios: launched on {mobile_sdk.SIMULATOR} ({udid})")
    return 0


def android(args: argparse.Namespace) -> int:
    apk = phone_build.android_bundle() if args.no_build else phone_build.android_apk()
    require_artifact(apk, "android-build")
    with system.file_lock(paths.cache() / "android-phone.lock"):
        emulator = None
        if phone.adb("get-state").stdout.strip() != "device":
            emulator = phone.start_emulator(
                gles=False, window=not repo_paths.is_agent()
            )
        try:
            phone.install_android(apk)
            phone.adb_checked("shell", "am", "start", "-W", "-S", "-n", phone.ACTIVITY)
        except BaseException:
            if emulator is not None:
                phone.adb("emu", "kill")
                emulator.wait(timeout=phone.REPLAY_LIMIT_S)
            raise
    print(f"android: launched on {phone.AVD} ({phone.SERIAL})")
    return 0


COMMANDS = (
    Command(
        "ios",
        "builds and launches the native app in the iOS Simulator",
        (
            ("just b3d ios", "build, install and launch in the Simulator"),
            ("just b3d ios --no-build", "launch the last simulator build"),
        ),
        phone.checked(ios),
        no_build,
    ),
    Command(
        "android",
        "builds and launches the native app in the Android emulator",
        (
            ("just b3d android", "build, install and launch in the emulator"),
            ("just b3d android --no-build", "launch the last APK"),
        ),
        phone.checked(android),
        no_build,
    ),
    Command(
        "ios-build",
        "builds the native iOS Simulator app without launching it",
        (("just b3d ios-build", "print the unsigned simulator app's path"),),
        phone.checked(ios_build),
    ),
    Command(
        "android-build",
        "builds the native Android APK without launching it",
        (("just b3d android-build", "print the debug APK's path"),),
        phone.checked(android_build),
    ),
    Command(
        "mobile-check",
        "builds the native phone apps supported by this host",
        (("just b3d mobile-check", "build the supported phone hosts"),),
        phone.checked(build_only),
    ),
    Command(
        "mobile-setup",
        "installs the native phone build toolchains",
        (("just b3d mobile-setup", "install missing build tools"),),
        phone.checked(setup),
    ),
)
