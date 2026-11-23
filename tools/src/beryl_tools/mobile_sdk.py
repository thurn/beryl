"""SDK setup and device discovery shared by the phone hosts."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from collections.abc import Callable, Mapping
from pathlib import Path
from typing import Any

from . import android_sdk, paths, system
from .android_sdk import NDK_VERSION as NDK_VERSION

# The simulator the recipes create and use, so they never disturb one of the user's.
SIMULATOR = "Beryl iPhone"
SIMULATOR_TYPE = "com.apple.CoreSimulator.SimDeviceType.iPhone-17"
IOS_TARGETS = ("aarch64-apple-ios", "aarch64-apple-ios-sim")

# The Android SDK, where Android Studio would put it; recipes set its variables themselves.
ANDROID_HOME: Path = android_sdk.sdk_root()
ANDROID_API = "36"
ANDROID_SYSTEM_IMAGE: str = f"system-images;android-{ANDROID_API};google_apis;arm64-v8a"
ANDROID_BUILD_PACKAGES: tuple[str, ...] = (
    f"platforms;android-{ANDROID_API}",
    "build-tools;36.0.0",
    f"ndk;{NDK_VERSION}",
)
ANDROID_PACKAGES: tuple[str, ...] = (
    "cmdline-tools;latest",
    "platform-tools",
    "emulator",
    *ANDROID_BUILD_PACKAGES,
    ANDROID_SYSTEM_IMAGE,
)
ANDROID_TARGETS: tuple[str] = (android_sdk.TARGET,)
# The Android Gradle Plugin runs on a JDK it supports, not the newest.
JDK_FORMULA = "openjdk@21"


def run(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, cwd=cwd, capture_output=True, text=True, check=False)


def rust_targets(targets: tuple[str, ...]) -> int:
    """Adds the Rust `targets` missing from the toolchain rust-toolchain.toml pins."""
    # From the repository, so rustup acts on the pinned toolchain.
    installed = run("rustup", "target", "list", "--installed", cwd=paths.REPO)
    missing = [t for t in targets if t not in installed.stdout.split()]
    if not missing:
        return 0
    print(f"mobile-setup: rustup target add {' '.join(missing)}")
    return subprocess.run(
        ["rustup", "target", "add", *missing], cwd=paths.REPO, check=False
    ).returncode


def ios_setup(formulae: tuple[str, ...], simulator_ready: Callable[[], bool]) -> int:
    """Installs what an iOS build needs, skipping what is there."""
    code = rust_targets(IOS_TARGETS)
    if code != 0:
        return code
    missing = [f for f in formulae if shutil.which(formula_command(f)) is None]
    if missing:
        print(f"mobile-setup: brew install {' '.join(missing)}")
        code = subprocess.run(["brew", "install", *missing], check=False).returncode
        if code != 0:
            return code
    if not simulator_ready():
        print("mobile-setup: xcodebuild -downloadPlatform iOS (about 8 GB)")
        code = subprocess.run(
            ["xcodebuild", "-downloadPlatform", "iOS"], check=False
        ).returncode
        if code != 0:
            return code
    print("mobile-setup: iOS ready")
    return 0


def can_build_for_simulator(project: Path, scheme: str) -> bool:
    """Whether Xcode offers an iOS Simulator to build the app for. It may not while a
    simulator runtime of its SDK's version is installed: it wants the build of that runtime
    it shipped with, which no version number names, and says "iOS 26.5 is not installed".
    """
    listed = run(
        "xcodebuild",
        "-showdestinations",
        "-scheme",
        scheme,
        "-project",
        str(project),
    )
    eligible = listed.stdout.split("Ineligible destinations")[0]
    return "platform:iOS Simulator" in eligible


def java_home() -> Path:
    if system.WINDOWS:
        return Path(os.environ.get("JAVA_HOME", ""))
    prefix = run("brew", "--prefix", JDK_FORMULA).stdout.strip()
    return Path(prefix) / "libexec" / "openjdk.jdk" / "Contents" / "Home"


def android_env(base: Mapping[str, str] | None = None) -> dict[str, str]:
    """The environment for the Android SDK, NDK and JDK tools."""
    tools = [
        ANDROID_HOME / "platform-tools",
        ANDROID_HOME / "emulator",
        ANDROID_HOME / "cmdline-tools" / "latest" / "bin",
    ]
    env = os.environ if base is None else base
    return {
        **env,
        "ANDROID_HOME": str(ANDROID_HOME),
        "NDK_HOME": str(ANDROID_HOME / "ndk" / NDK_VERSION),
        "JAVA_HOME": str(java_home()),
        "PATH": os.pathsep.join([*map(str, tools), env.get("PATH", "")]),
    }


def android_setup() -> int:
    """Installs what an Android build needs, skipping what is there, the SDK's licenses
    accepted."""
    code = rust_targets(ANDROID_TARGETS)
    if code != 0:
        return code
    if system.WINDOWS:
        java = os.environ.get("JAVA_HOME", "")
        if not java or not (Path(java) / "bin" / "java.exe").is_file():
            print("mobile-setup: set JAVA_HOME to your JDK 21", file=sys.stderr)
            return 2
    elif not (java_home() / "bin" / "java").exists():
        print(f"mobile-setup: brew install {JDK_FORMULA}")
        code = subprocess.run(["brew", "install", JDK_FORMULA], check=False).returncode
        if code != 0:
            return code
    # A package `a;b;c` installs into the SDK's a/b/c.
    packages = ANDROID_BUILD_PACKAGES if system.WINDOWS else ANDROID_PACKAGES
    missing = [
        p
        for p in packages
        if not (ANDROID_HOME.joinpath(*p.split(";")) / "package.xml").exists()
    ]
    if missing:
        manager = "sdkmanager.bat" if system.WINDOWS else "sdkmanager"
        sdkmanager = ANDROID_HOME / "cmdline-tools" / "latest" / "bin" / manager
        if not sdkmanager.exists():
            if system.WINDOWS:
                print(
                    f"mobile-setup: install the Android command-line tools in {sdkmanager.parent.parent}",
                    file=sys.stderr,
                )
                return 2
            # Homebrew's command-line tools bootstrap the SDK's own.
            if shutil.which("sdkmanager") is None:
                print("mobile-setup: brew install --cask android-commandlinetools")
                code = subprocess.run(
                    ["brew", "install", "--cask", "android-commandlinetools"],
                    check=False,
                ).returncode
                if code != 0:
                    return code
            sdkmanager = Path(shutil.which("sdkmanager") or "sdkmanager")
        command = [str(sdkmanager), f"--sdk_root={ANDROID_HOME}"]
        env = None if system.WINDOWS else {**os.environ, "JAVA_HOME": str(java_home())}
        print(f"mobile-setup: sdkmanager {' '.join(missing)}")
        subprocess.run(
            [*command, "--licenses"],
            input="y\n" * 20,
            text=True,
            env=env,
            stdout=subprocess.DEVNULL,
            check=False,
        )
        code = subprocess.run([*command, *missing], env=env, check=False).returncode
        if code != 0:
            return code
    print("mobile-setup: Android ready")
    return 0


def formula_command(formula: str) -> str:
    return "pod" if formula == "cocoapods" else formula


def sdk_version() -> str:
    """The iOS Simulator SDK this Xcode builds with, as `26.5`."""
    sdk = run("xcrun", "--sdk", "iphonesimulator", "--show-sdk-version")
    return sdk.stdout.strip()


def simulator_runtime() -> str | None:
    """The identifier of the installed iOS Simulator runtime of Xcode's SDK's version."""
    listed = run("xcrun", "simctl", "list", "runtimes", "--json")
    if listed.returncode != 0:
        return None
    sdk = sdk_version()
    runtimes: list[dict[str, Any]] = json.loads(listed.stdout)["runtimes"]
    matching = [
        r
        for r in runtimes
        if r.get("platform") == "iOS"
        and r.get("isAvailable")
        and str(r.get("version", "")).startswith(sdk)
    ]
    # The newest build of that version.
    matching.sort(key=lambda r: str(r.get("buildversion", "")))
    return str(matching[-1]["identifier"]) if matching else None


def simulator() -> tuple[str, bool]:
    """The UDID of the Beryl simulator, created if missing, and whether it was booted
    already."""
    runtime = simulator_runtime()
    if runtime is None:
        raise SystemExit("ios: no iOS Simulator runtime: run `just mobile-setup`")
    listed = run("xcrun", "simctl", "list", "devices", "--json")
    devices: list[dict[str, Any]] = json.loads(listed.stdout)["devices"].get(
        runtime, []
    )
    for device in devices:
        if device.get("name") == SIMULATOR and device.get("isAvailable"):
            return str(device["udid"]), device.get("state") == "Booted"
    created = run("xcrun", "simctl", "create", SIMULATOR, SIMULATOR_TYPE, runtime)
    if created.returncode != 0:
        raise SystemExit(f"ios: simctl create failed: {created.stderr.strip()}")
    return created.stdout.strip(), False


def boot(udid: str) -> None:
    # `bootstatus -b` boots it if needed and waits until it can launch apps.
    subprocess.run(
        ["xcrun", "simctl", "bootstatus", udid, "-b"],
        stdout=subprocess.DEVNULL,
        check=True,
    )


def install(udid: str, bundle: Path) -> None:
    subprocess.run(["xcrun", "simctl", "install", udid, str(bundle)], check=True)


def alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    return True


def paired_device() -> str | None:
    """The identifier of the first iPhone paired with this Mac: devicectl connects to it on
    demand, over USB or the network."""
    out = paths.cache_root() / "devicectl.json"
    listed = run("xcrun", "devicectl", "list", "devices", "--json-output", str(out))
    if listed.returncode != 0 or not out.exists():
        return None
    devices: list[dict[str, Any]] = json.loads(out.read_text())["result"]["devices"]
    for d in devices:
        props: dict[str, Any] = d.get("hardwareProperties", {})
        conn: dict[str, Any] = d.get("connectionProperties", {})
        if props.get("platform") == "iOS" and conn.get("pairingState") == "paired":
            return str(d["identifier"])
    return None
