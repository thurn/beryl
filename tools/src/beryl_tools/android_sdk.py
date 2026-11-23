"""The pinned Android SDK and the C tools for the arm64 Android target."""

from __future__ import annotations

import os
import sys
from pathlib import Path

TARGET = "aarch64-linux-android"
NDK_VERSION = "28.2.13676358"
# Both Android hosts declare this minSdk.
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


def ndk_tools() -> Path:
    host = "windows-x86_64" if sys.platform == "win32" else "darwin-x86_64"
    return (
        sdk_root()
        / "ndk"
        / NDK_VERSION
        / "toolchains"
        / "llvm"
        / "prebuilt"
        / host
        / "bin"
    )


def ndk_env() -> dict[str, str]:
    """The environment for Cargo build scripts that compile C for Android."""
    tools = ndk_tools()
    if sys.platform == "win32":
        # Invoke clang.exe directly with the API level. The target-prefixed entry
        # point on Windows is a batch file.
        return {
            **os.environ,
            "CC_aarch64_linux_android": str(tools / "clang.exe"),
            "CFLAGS_aarch64_linux_android": f"--target={TARGET}{MIN_SDK}",
            "AR_aarch64_linux_android": str(tools / "llvm-ar.exe"),
        }
    return {
        **os.environ,
        "CC_aarch64_linux_android": str(tools / f"{TARGET}{MIN_SDK}-clang"),
        "AR_aarch64_linux_android": str(tools / "llvm-ar"),
    }


def native_env() -> dict[str, str]:
    """Adds the C++ compiler and Rust linker for the native host's shared library."""
    env = ndk_env()
    tools = ndk_tools()
    clang = env["CC_aarch64_linux_android"]
    env["CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"] = clang
    if sys.platform == "win32":
        env.update(
            CXX_aarch64_linux_android=str(tools / "clang++.exe"),
            CXXFLAGS_aarch64_linux_android=env["CFLAGS_aarch64_linux_android"],
            CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS=(
                f"-C link-arg=--target={TARGET}{MIN_SDK}"
            ),
        )
    else:
        env["CXX_aarch64_linux_android"] = f"{clang}++"
    return env
