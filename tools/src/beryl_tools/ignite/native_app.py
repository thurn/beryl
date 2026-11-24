"""List and run the native desktop smoke and build-only phone checks."""

from __future__ import annotations

import subprocess
import sys
from collections.abc import Callable

from ..client import app, phone_build
from . import Listed, Result, listed, report

CHECKS = {
    "native-app": "native-app:smoke",
    "native-ios": "native-ios:simulator-build",
    "native-android": "native-android:arm64-build",
}


def desktop() -> None:
    if app.smoke() != 0:
        raise RuntimeError("native desktop smoke failed")


def ios() -> None:
    bundle = phone_build.ios_app(phone_build.IOS_SIMULATOR, release=False, sign=False)
    if not (bundle / "Beryl").is_file():
        raise RuntimeError(f"iOS built no executable in {bundle}")


def android() -> None:
    apk = phone_build.android_apk()
    if not apk.is_file():
        raise RuntimeError(f"Android built no APK at {apk}")


RUN: dict[str, Callable[[], None]] = {
    "native-app": desktop,
    "native-ios": ios,
    "native-android": android,
}


def main(argv: list[str]) -> int:
    match argv:
        case ["list", family] if family in CHECKS:
            listed([Listed(CHECKS[family])])
            return 0
        case ["run", family, check] if CHECKS.get(family) == check:
            try:
                RUN[family]()
            except (
                OSError,
                RuntimeError,
                subprocess.SubprocessError,
                SystemExit,
            ) as error:
                report([Result(check, "fail", str(error))])
                return 1
            report([Result(check, "pass")])
            return 0
        case _:
            print(
                "usage: native_app (list FAMILY | run FAMILY CHECK)",
                file=sys.stderr,
            )
            return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
