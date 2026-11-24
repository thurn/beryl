"""The pinned Android SDK and the C tools for Android devices and emulators."""

from __future__ import annotations

import os
import platform
import sys
from pathlib import Path

DEVICE_ABI = "arm64-v8a"
RUST_TARGETS: dict[str, str] = {
    DEVICE_ABI: "aarch64-linux-android",
    "x86_64": "x86_64-linux-android",
}
TARGET: str = RUST_TARGETS[DEVICE_ABI]
NDK_VERSION = "28.2.13676358"
# Both Android hosts declare this minSdk.
MIN_SDK = 24


def emulator_abi() -> str:
    """The Android ABI that the emulator can run with host CPU acceleration."""
    machine = platform.machine().lower()
    if machine in ("aarch64", "arm64"):
        return DEVICE_ABI
    if machine in ("amd64", "x86_64"):
        return "x86_64"
    raise SystemExit(f"android: no emulator ABI for host architecture {machine}")


def build_abis() -> tuple[str, ...]:
    """The ABIs in the APK, including the one used by physical devices."""
    emulator = emulator_abi()
    return (DEVICE_ABI,) if emulator == DEVICE_ABI else (DEVICE_ABI, emulator)


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


def ndk_env(target: str = TARGET) -> dict[str, str]:
    """The environment for Cargo build scripts that compile C for Android."""
    tools = ndk_tools()
    suffix = target.replace("-", "_")
    if sys.platform == "win32":
        # Invoke clang.exe directly with the API level. The target-prefixed entry
        # point on Windows is a batch file.
        return {
            **os.environ,
            f"CC_{suffix}": str(tools / "clang.exe"),
            f"CFLAGS_{suffix}": f"--target={target}{MIN_SDK}",
            f"AR_{suffix}": str(tools / "llvm-ar.exe"),
        }
    return {
        **os.environ,
        f"CC_{suffix}": str(tools / f"{target}{MIN_SDK}-clang"),
        f"AR_{suffix}": str(tools / "llvm-ar"),
    }


def native_env(target: str = TARGET) -> dict[str, str]:
    """Adds the C++ compiler and Rust linker for the native host's shared library."""
    env = ndk_env(target)
    tools = ndk_tools()
    suffix = target.replace("-", "_")
    cargo = f"CARGO_TARGET_{suffix.upper()}"
    clang = env[f"CC_{suffix}"]
    env[f"{cargo}_LINKER"] = clang
    if sys.platform == "win32":
        env[f"CXX_{suffix}"] = str(tools / "clang++.exe")
        env[f"CXXFLAGS_{suffix}"] = env[f"CFLAGS_{suffix}"]
        env[f"{cargo}_RUSTFLAGS"] = f"-C link-arg=--target={target}{MIN_SDK}"
    else:
        env[f"CXX_{suffix}"] = f"{clang}++"
    return env
