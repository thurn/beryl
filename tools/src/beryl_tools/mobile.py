"""The phone apps: `mobile-setup` installs their toolchains, `ios` builds the app for the iOS
Simulator and launches it there, `ios-smoke` does the same headless until the UI reports that
it loaded, and `ios-device` builds, signs and installs it on a paired iPhone. The Android
recipes are `android`'s."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import TYPE_CHECKING, Any

from . import app, paths, system

if TYPE_CHECKING:
    from .checks import Lane

BUNDLE_ID = "ca.thurn.beryl"
APPLE: Path = paths.APP / "gen" / "apple"
# The simulator the recipes create and use, so they never disturb one of the user's.
SIMULATOR = "Beryl iPhone"
SIMULATOR_TYPE = "com.apple.CoreSimulator.SimDeviceType.iPhone-17"
# What lib.rs's `ui_ready` journals in smoke mode on a phone.
READY = "ui ready"
IOS_TARGETS = ("aarch64-apple-ios", "aarch64-apple-ios-sim")
# Tauri's iOS project generation needs both; Homebrew installs them.
IOS_FORMULAE = ("cocoapods", "xcodegen")

# The Android SDK, where Android Studio would put it; recipes set its variables themselves.
ANDROID_HOME: Path = Path.home() / "Library" / "Android" / "sdk"
ANDROID_API = "36"
NDK_VERSION = "28.2.13676358"
# gen/android/app/build.gradle.kts's minSdk.
MIN_SDK = 24
ANDROID_SYSTEM_IMAGE: str = f"system-images;android-{ANDROID_API};google_apis;arm64-v8a"
ANDROID_PACKAGES: tuple[str, ...] = (
    "cmdline-tools;latest",
    "platform-tools",
    "emulator",
    f"platforms;android-{ANDROID_API}",
    "build-tools;36.0.0",
    f"ndk;{NDK_VERSION}",
    ANDROID_SYSTEM_IMAGE,
)
ANDROID_TARGETS = ("aarch64-linux-android",)
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


def ios_setup() -> int:
    """Installs what an iOS build needs, skipping what is there."""
    code = rust_targets(IOS_TARGETS)
    if code != 0:
        return code
    formulae = [f for f in IOS_FORMULAE if shutil.which(formula_command(f)) is None]
    if formulae:
        print(f"mobile-setup: brew install {' '.join(formulae)}")
        code = subprocess.run(["brew", "install", *formulae], check=False).returncode
        if code != 0:
            return code
    if not can_build_for_simulator():
        print("mobile-setup: xcodebuild -downloadPlatform iOS (about 8 GB)")
        code = subprocess.run(
            ["xcodebuild", "-downloadPlatform", "iOS"], check=False
        ).returncode
        if code != 0:
            return code
    print("mobile-setup: iOS ready")
    return 0


def can_build_for_simulator() -> bool:
    """Whether Xcode offers an iOS Simulator to build the app for. It may not while a
    simulator runtime of its SDK's version is installed: it wants the build of that runtime
    it shipped with, which no version number names, and says "iOS 26.5 is not installed".
    """
    listed = run(
        "xcodebuild",
        "-showdestinations",
        "-scheme",
        "beryl_app_iOS",
        "-project",
        str(APPLE / "beryl_app.xcodeproj"),
    )
    eligible = listed.stdout.split("Ineligible destinations")[0]
    return "platform:iOS Simulator" in eligible


def java_home() -> Path:
    prefix = run("brew", "--prefix", JDK_FORMULA).stdout.strip()
    return Path(prefix) / "libexec" / "openjdk.jdk" / "Contents" / "Home"


def android_env() -> dict[str, str]:
    """The Tauri CLI's environment with the Android SDK, NDK and JDK, whatever the user's
    shell profile sets."""
    tools = [
        ANDROID_HOME / "platform-tools",
        ANDROID_HOME / "emulator",
        ANDROID_HOME / "cmdline-tools" / "latest" / "bin",
    ]
    env = paths.tauri_env()
    return {
        **env,
        "ANDROID_HOME": str(ANDROID_HOME),
        "NDK_HOME": str(ANDROID_HOME / "ndk" / NDK_VERSION),
        "JAVA_HOME": str(java_home()),
        "PATH": os.pathsep.join([*map(str, tools), env.get("PATH", "")]),
    }


def ndk_env() -> dict[str, str]:
    """The environment for plain cargo building C for Android (ring's, under rustls) with
    the NDK's clang; Tauri's CLI sets it up itself."""
    prebuilt = ANDROID_HOME / "ndk" / NDK_VERSION / "toolchains" / "llvm" / "prebuilt"
    clang = prebuilt / "darwin-x86_64" / "bin" / f"aarch64-linux-android{MIN_SDK}-clang"
    return {
        **os.environ,
        "CC_aarch64_linux_android": str(clang),
        "AR_aarch64_linux_android": str(clang.with_name("llvm-ar")),
    }


def lane() -> Lane:
    """Type-checks the app crate for the iOS Simulator and Android (`by mobile-check`); the
    phone builds themselves take minutes, too long for the budget."""
    from .checks import BY, Lane  # It imports this module.

    return Lane("mobile", warm=True).step("mobile-check", [*BY, "mobile-check"])


def check(args: argparse.Namespace) -> int:
    """Type-checks the app crate for the iOS Simulator and Android: the native builds slow CI
    can afford (seconds on a warm cache)."""
    manifest = str(paths.RUST / "Cargo.toml")
    for target in ("aarch64-apple-ios-sim", "aarch64-linux-android"):
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
            env=ndk_env(),
            check=False,
        ).returncode
        print(f"mobile-check: {target} in {time.monotonic() - started:.0f} s")
        if code != 0:
            print("mobile-check: missing targets or NDK? `just mobile-setup`")
            return code
    return 0


def android_setup() -> int:
    """Installs what an Android build needs, skipping what is there, the SDK's licenses
    accepted."""
    code = rust_targets(ANDROID_TARGETS)
    if code != 0:
        return code
    if not (java_home() / "bin" / "java").exists():
        print(f"mobile-setup: brew install {JDK_FORMULA}")
        code = subprocess.run(["brew", "install", JDK_FORMULA], check=False).returncode
        if code != 0:
            return code
    # A package `a;b;c` installs into the SDK's a/b/c.
    missing = [
        p
        for p in ANDROID_PACKAGES
        if not (ANDROID_HOME.joinpath(*p.split(";")) / "package.xml").exists()
    ]
    if missing:
        sdkmanager = ANDROID_HOME / "cmdline-tools" / "latest" / "bin" / "sdkmanager"
        if not sdkmanager.exists():
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
        env = {**os.environ, "JAVA_HOME": str(java_home())}
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


def setup(args: argparse.Namespace) -> int:
    if sys.platform != "darwin":
        print("mobile-setup: the phone apps build on macOS", file=sys.stderr)
        return 2
    return ios_setup() or android_setup()


def build_simulator_app() -> int:
    """Builds the debug app for the iOS Simulator through Tauri's CLI ([`SIMULATOR_APP`])."""
    started = time.monotonic()
    code = app.tauri("ios", "build", "--debug", "--target", "aarch64-sim")
    print(f"ios: built in {time.monotonic() - started:.0f} s")
    return code


# Where `tauri ios build --target aarch64-sim` leaves the simulator app.
SIMULATOR_APP: Path = APPLE / "build" / "arm64-sim" / "Beryl.app"
# And where `tauri ios build --target aarch64` leaves the device app's archive.
DEVICE_IPA: Path = APPLE / "build" / "arm64" / "Beryl.ipa"


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


def alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    return True


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
    if install and paths.is_agent():
        # Agents verify in the simulator only; the user's phone is theirs.
        print(
            "ios-device: agents never install to a phone (--no-install, --no-sign)",
            file=sys.stderr,
        )
        return 2
    started = time.monotonic()
    sign = ["--no-sign"] if args.no_sign else []
    code = app.tauri("ios", "build", "--target", "aarch64", *sign)
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
  1. Xcode > Settings > Accounts: sign in with the Apple ID of team FYHM4Q335L.
  2. Connect the iPhone, trust this Mac, and turn on Settings > Privacy & Security >
     Developer Mode on the phone.
then run `just ios-device` again."""


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


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    sub.add_parser(
        "mobile-setup", help="install the phone apps' toolchains (idempotent)"
    ).set_defaults(func=setup)
    sub.add_parser(
        "mobile-check", help="cargo check the app for the iOS Simulator and Android"
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
    p.set_defaults(func=device)
