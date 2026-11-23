"""The pinned Android SDK and the C tools for the arm64 Android target."""

from __future__ import annotations

import os
import sys
from pathlib import Path

TARGET = "aarch64-linux-android"
NDK_VERSION = "28.2.13676358"
# gen/android/app/build.gradle.kts's minSdk.
MIN_SDK = 24


def sdk_root() -> Path:
    """Use the SDK configured for Windows, or the default SDK of the host."""
    if sys.platform == "win32":
        configured = os.environ.get("ANDROID_HOME") or os.environ.get(
            "ANDROID_SDK_ROOT"
        )
        if configured:
            return Path(configured)
        return Path(os.environ["LOCALAPPDATA"]) / "Android" / "sdk"
    return Path.home() / "Library" / "Android" / "sdk"


def ndk_env() -> dict[str, str]:
    """The environment for Cargo build scripts that compile C for Android."""
    ndk = sdk_root() / "ndk" / NDK_VERSION
    prebuilt = ndk / "toolchains" / "llvm" / "prebuilt"
    if sys.platform == "win32":
        # Invoke clang.exe directly with the API level. The target-prefixed entry
        # point on Windows is a batch file.
        tools = prebuilt / "windows-x86_64" / "bin"
        return {
            **os.environ,
            "CC_aarch64_linux_android": str(tools / "clang.exe"),
            "CFLAGS_aarch64_linux_android": f"--target={TARGET}{MIN_SDK}",
            "AR_aarch64_linux_android": str(tools / "llvm-ar.exe"),
        }
    tools = prebuilt / "darwin-x86_64" / "bin"
    return {
        **os.environ,
        "CC_aarch64_linux_android": str(tools / f"{TARGET}{MIN_SDK}-clang"),
        "AR_aarch64_linux_android": str(tools / "llvm-ar"),
    }
