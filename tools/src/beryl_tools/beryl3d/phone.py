"""Beryl 3D's phone spike (docs/beryl-3d.md, 'Hosts'): `just b3d ios-smoke` and
`just b3d android-smoke` build the app's library and its small Swift or Kotlin host, install it
in the headless iOS Simulator or Android emulator, replay a recording, wait for its frame log
to say the first frame was shown and the recording's last script ended, save a PNG of the
screen and exit. `just b3d ios-smoke --touch` instead runs a UI test that touches and rotates a
live game, and reads the frame log for proof that each event arrived and that the app stopped
drawing while idle. `just b3d ios-device` does the same on the user's paired iPhone, and only when
he asked for it.

A phone has no recordings pack and no engine to build one, so each recording is written to a
file on the Mac (`beryl3d --fixture <name> --write-fixture <path>`) and copied into the app's
data folder, where the app reads it and writes its frame log.

These commands are in neither the lane nor the sweep: milestone 4 adds them.
"""

from __future__ import annotations

import argparse
import importlib
import json
import os
import shutil
import subprocess
import sys
import time
from collections.abc import Callable, Iterator
from contextlib import contextmanager, nullcontext
from pathlib import Path
from typing import Any

from .. import checks, mobile, paths as repo_paths, system
from . import app, paths
from .command import Command
from .phone_log import (
    android_touch_faults,
    lifecycle_faults,
    lines,
    logged_inputs,
    rotated_and_returned,
    surface_sizes,
)

CRATE: Path = paths.WORKSPACE / "crates" / "beryl_3d"
# Different from the Tauri app's ca.thurn.beryl, so both can be installed.
BUNDLE = "ca.thurn.beryl3d"
ACTIVITY = f"{BUNDLE}/.MainActivity"
FIXTURE = "verbs-zones"
# What `ios-device` replays, in order: the recording and the largest gated board.
DEVICE_FIXTURES = ("verbs-zones", "stress-255")
# How long a replay may take before it counts as hung, and how long a device step may wait on
# a prompt nobody answers (signing, trust, unlock) before it counts as not reachable.
REPLAY_LIMIT_S = 600
DEVICE_LIMIT_S = 600
# The library's error log in the app's data folder (`ERRORS` in beryl_3d's src/ffi), and the
# log of the warnings wgpu reports through the `log` crate.
ERRORS = "beryl3d.log"
LIBRARY_LOG = "wgpu.log"
# The Android emulator of these commands: an AVD and a port of their own, so that they never
# disturb the one `just android-smoke` uses, and so that each run can boot it with or without
# Vulkan.
AVD = "beryl3d_phone"
PORT = 5586
SERIAL = f"emulator-{PORT}"
# The emulator option that hides Vulkan from the system image, so wgpu falls back to OpenGL
# ES 3.0.
NO_VULKAN = ("-feature", "-Vulkan")
# The Tauri app's Gradle wrapper, which the Android host's project uses too.
GRADLEW: Path = repo_paths.APP / "gen" / "android" / "gradlew"


def out_dir(kind: str) -> Path:
    """`$BERYL_CACHE/b3d/<kind>`, where builds and runs leave their files."""
    path = paths.cache() / kind
    path.mkdir(parents=True, exist_ok=True)
    return path


def run_name() -> str:
    """A name no other run shares: the start time and this process."""
    return f"{int(time.time() * 1000)}-{os.getpid()}"


def library(target: str, crate_type: str, release: bool, env: dict[str, str]) -> Path:
    """Builds beryl_3d's library for `target` as a `staticlib` or `cdylib`, and returns the
    file. Cargo leaves a crate type the manifest does not declare in `deps`, so its path is
    read from cargo's messages."""
    from .cli import warm_target  # cli imports this module.

    warm_target()
    command = [
        *["cargo", "rustc", "-p", "beryl_3d", "--lib", "--crate-type", crate_type],
        *["--target", target, "--message-format", "json-render-diagnostics"],
        *(["--release"] if release else []),
    ]
    started = time.monotonic()
    built = subprocess.run(
        command, cwd=paths.WORKSPACE, env=env, stdout=subprocess.PIPE, text=True
    )
    if built.returncode != 0:
        raise SystemExit(f"cargo could not build the library for {target}")
    suffix = ".a" if crate_type == "staticlib" else ".so"
    found = None
    for line in built.stdout.splitlines():
        message: dict[str, Any] = json.loads(line)
        if (
            message.get("reason") == "compiler-artifact"
            and message.get("target", {}).get("name") == "beryl_3d"
        ):
            found = next((f for f in message["filenames"] if f.endswith(suffix)), found)
    if found is None:
        raise SystemExit(f"cargo built no {suffix} for {target}")
    size = Path(found).stat().st_size / 1e6
    print(
        f"library: {target} {crate_type}, {size:.1f} MB,"
        f" built in {time.monotonic() - started:.0f} s",
        flush=True,
    )
    return Path(found)


def write_fixtures(names: tuple[str, ...], folder: Path) -> list[Path]:
    """Writes each recording to `<folder>/<name>.json` with the desktop app."""
    if app.build() != 0:
        raise SystemExit("the desktop app, which writes the recordings, did not build")
    written = []
    for name in names:
        path = folder / f"{name}.json"
        command = [str(app.BINARY), "--fixture", name, "--write-fixture", str(path)]
        if subprocess.run(command, check=False).returncode != 0:
            raise SystemExit(f"the recording {name} could not be written")
        written.append(path)
    return written


def wait_for_log(
    read: Callable[[], str | None],
    alive: Callable[[], bool],
    errors: Callable[[], str],
    label: str,
    limit_s: int = REPLAY_LIMIT_S,
    pause: Callable[[float], None] = time.sleep,
    event: str = "summary",
    ready: Callable[[list[dict[str, Any]]], bool] | None = None,
) -> str | None:
    """Polls until the log contains `event` or meets `ready`. By default a replay's
    summary marks its end. Returns None if the app stops or the wait times out."""
    deadline = time.monotonic() + limit_s
    named = False
    while time.monotonic() < deadline:
        text = read()
        parsed = lines(text) if text else []
        if parsed and not named and "run" in parsed[0]:
            named = True
            print(f"{label}: adapter {parsed[0]['run']['adapter']}")
        reached = ready(parsed) if ready else any(event in line for line in parsed)
        if reached:
            return text
        # The app writes its error log only for what stops it drawing: a missing recording,
        # no surface, a panic.
        written = errors()
        if written:
            print(f"{label}: FAILED: the app reported an error")
            print(written)
            return None
        if not alive():
            print(f"{label}: FAILED: the app exited before the recording ended")
            print(errors() or f"{label}: the app wrote no error")
            return None
        pause(1)
    print(f"{label}: FAILED: no {event} in the log after {limit_s} s")
    print(errors() or f"{label}: the app wrote no error")
    return None


def finish(label: str, log: str | None, png: Path) -> int:
    """Saves the log beside the PNG, prints its report, and says whether the run passed."""
    if log is None:
        print(f"{label}: FAILED")
        return 1
    saved = png.with_suffix(".jsonl")
    saved.write_text(log, encoding="utf-8")
    summary = next(line["summary"] for line in lines(log) if "summary" in line)
    print(app.report(saved))
    print(f"{label}: screen {png}")
    passed = summary.get("first_shown_ms") is not None
    print(f"{label}: {'passed' if passed else 'FAILED: no frame was shown'}")
    return 0 if passed else 1


# The iOS Simulator and the iPhone.


def ios_app(sdk: str, release: bool, sign: bool, tests: bool = False) -> Path:
    """Builds the library and the Swift host for `sdk` (iphonesimulator or iphoneos), and
    returns the app. With `tests`, also builds the UI test bundle, and the `.xctestrun` file
    that runs it is in the app's products folder's parent."""
    target = (
        "aarch64-apple-ios-sim" if sdk == "iphonesimulator" else "aarch64-apple-ios"
    )
    env = {**os.environ, "IPHONEOS_DEPLOYMENT_TARGET": "15.0"}
    built = library(target, "staticlib", release, env)
    root = out_dir("ios")
    lib = root / sdk / "lib"
    lib.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(built, lib / "libberyl_3d.a")
    project = root / "project"
    project.mkdir(exist_ok=True)
    generated = mobile.run(
        "xcodegen",
        "generate",
        "--spec",
        str(CRATE / "ios" / "project.yml"),
        "--project",
        str(project),
    )
    if generated.returncode != 0:
        raise SystemExit(f"xcodegen failed: {generated.stdout}{generated.stderr}")
    signing = ["-allowProvisioningUpdates"] if sign else ["CODE_SIGNING_ALLOWED=NO"]
    platform = "iOS Simulator" if sdk == "iphonesimulator" else "iOS"
    started = time.monotonic()
    code = subprocess.run(
        [
            *["xcodebuild", "-quiet", "-project", str(project / "Beryl3D.xcodeproj")],
            *["-scheme", "Beryl3D", "-sdk", sdk, "-configuration", "Debug"],
            *["-destination", f"generic/platform={platform}"],
            *["-derivedDataPath", str(root / "derived"), *signing],
            f"BERYL3D_LIB_DIR={lib}",
            f"BERYL3D_CRATE_DIR={CRATE}",
            "build-for-testing" if tests else "build",
        ],
        check=False,
        timeout=DEVICE_LIMIT_S,
    ).returncode
    if code != 0:
        raise SystemExit("xcodebuild failed")
    print(f"ios: host built in {time.monotonic() - started:.0f} s")
    return root / "derived" / "Build" / "Products" / f"Debug-{sdk}" / "Beryl3D.app"


def ios_device_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--user-asked",
        action="store_true",
        help="let an agent install: the user asked in chat for the app on their phone",
    )


def devicectl(*args: str) -> subprocess.CompletedProcess[str]:
    """`xcrun devicectl` with `args`, ended after `DEVICE_LIMIT_S` when it waits on a prompt
    nobody answers."""
    try:
        return subprocess.run(
            ["xcrun", "devicectl", *args],
            capture_output=True,
            text=True,
            check=False,
            timeout=DEVICE_LIMIT_S,
        )
    except subprocess.TimeoutExpired:
        raise SystemExit(
            f"ios-device: devicectl {args[0]} {args[1]} waited {DEVICE_LIMIT_S} s, so the"
            " phone counts as not reachable (locked, or a trust prompt nobody answered)"
        ) from None


def run_ios_device(args: argparse.Namespace) -> int:
    if repo_paths.is_agent() and not args.user_asked:
        print(
            "ios-device: agents install to the phone only when the user asks"
            " (--user-asked)",
            file=sys.stderr,
        )
        return 2
    paired = mobile.paired_device()
    if paired is None:
        print("ios-device: no iPhone is paired with this Mac, so none is reachable")
        return 1
    udid: str = paired
    try:
        bundle = ios_app("iphoneos", release=True, sign=True)
    except subprocess.TimeoutExpired:
        print(f"ios-device: signing waited {DEVICE_LIMIT_S} s; not reachable")
        return 1
    run = out_dir("phone") / f"{run_name()}-iphone"
    run.mkdir()
    fixtures = write_fixtures(DEVICE_FIXTURES, run)
    installed = devicectl("device", "install", "app", "--device", udid, str(bundle))
    if installed.returncode != 0:
        print(f"ios-device: install failed: {installed.stderr.strip()}")
        return 1
    container: list[str] = [
        *["--domain-type", "appDataContainer", "--domain-identifier", BUNDLE]
    ]
    failed = 0
    for fixture in fixtures:
        name: str = fixture.stem
        copied = devicectl(
            *["device", "copy", "to", "--device", udid, *container],
            *["--source", str(fixture), "--destination", f"Documents/{fixture.name}"],
        )
        if copied.returncode != 0:
            print(f"ios-device: copying {name} failed: {copied.stderr.strip()}")
            return 1
        launched = devicectl(
            *["device", "process", "launch", "--device", udid, "--terminate-existing"],
            *["--environment-variables", json.dumps({"BERYL3D_FIXTURE": name}), BUNDLE],
        )
        if launched.returncode != 0:
            print(f"ios-device: launch failed: {launched.stderr.strip()}")
            return 1
        pulled = run / f"{name}-documents"

        def read(pulled: Path = pulled, started: float = time.time()) -> str | None:
            shutil.rmtree(pulled, ignore_errors=True)
            devicectl(
                *["device", "copy", "from", "--device", udid, *container],
                *["--source", "Documents", "--destination", str(pulled)],
            )
            logs = [
                path
                for path in pulled.rglob("*.jsonl")
                if int(path.name.split("-", 1)[0]) >= started * 1000 - 60_000
                and (lines(path.read_text(encoding="utf-8")) or [{}])[0]
                .get("run", {})
                .get("fixture")
                == name
            ]
            return max(logs).read_text(encoding="utf-8") if logs else None

        log = wait_for_log(read, lambda: True, lambda: "", f"ios-device {name}")
        failed |= finish(f"ios-device {name}", log, run / f"{name}.png")
    return failed


# The Android emulator.


def adb(*args: str, stdin: Path | None = None) -> subprocess.CompletedProcess[str]:
    with stdin.open("rb") if stdin else open(os.devnull, "rb") as source:
        done = subprocess.run(
            [str(mobile.ANDROID_HOME / "platform-tools" / "adb"), "-s", SERIAL, *args],
            stdin=source,
            capture_output=True,
            check=False,
        )
    return subprocess.CompletedProcess(
        done.args,
        done.returncode,
        done.stdout.decode(errors="replace"),
        done.stderr.decode(errors="replace"),
    )


def android_apk() -> Path:
    """Builds the library and the Kotlin host, and returns the APK."""
    prebuilt = (
        mobile.ANDROID_HOME
        / "ndk"
        / mobile.NDK_VERSION
        / "toolchains"
        / "llvm"
        / "prebuilt"
        / "darwin-x86_64"
        / "bin"
    )
    clang = prebuilt / f"aarch64-linux-android{mobile.MIN_SDK}-clang"
    env = {
        **os.environ,
        "CC_aarch64_linux_android": str(clang),
        "CXX_aarch64_linux_android": f"{clang}++",
        "AR_aarch64_linux_android": str(prebuilt / "llvm-ar"),
        "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER": str(clang),
    }
    built = library("aarch64-linux-android", "cdylib", False, env)
    root = out_dir("android")
    jni = root / "jniLibs" / "arm64-v8a"
    jni.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(built, jni / "libberyl_3d.so")
    # The texture encoders (ctt) are C++ and link the NDK's shared C++ runtime, which an app
    # must bundle beside its library (docs/beryl-3d.md, "Preparation").
    runtime = prebuilt.parent / "sysroot" / "usr" / "lib" / "aarch64-linux-android"
    shutil.copyfile(runtime / "libc++_shared.so", jni / "libc++_shared.so")
    started = time.monotonic()
    code = subprocess.run(
        [
            *[str(GRADLEW), "--quiet", "-p", str(CRATE / "android"), "assembleDebug"],
            *["--project-cache-dir", str(root / "gradle")],
            f"-Pberyl3d.buildDir={root / 'build'}",
            f"-Pberyl3d.jniLibs={root / 'jniLibs'}",
            f"-Pkotlin.project.persistent.dir={root / 'kotlin'}",
        ],
        env=mobile.android_env(),
        check=False,
    ).returncode
    if code != 0:
        raise SystemExit("gradle failed")
    print(f"android: host built in {time.monotonic() - started:.0f} s")
    return root / "build" / "app" / "outputs" / "apk" / "debug" / "app-debug.apk"


def ensure_avd() -> None:
    emulator = str(mobile.ANDROID_HOME / "emulator" / "emulator")
    listed = subprocess.run(
        [emulator, "-list-avds"], capture_output=True, text=True, check=True
    )
    if AVD in listed.stdout.split():
        return
    subprocess.run(
        [
            str(
                mobile.ANDROID_HOME / "cmdline-tools" / "latest" / "bin" / "avdmanager"
            ),
            *["create", "avd", "--name", AVD, "--package", mobile.ANDROID_SYSTEM_IMAGE],
            *["--device", "pixel_8"],
        ],
        input="no\n",  # No custom hardware profile.
        text=True,
        env=mobile.android_env(),
        stdout=subprocess.DEVNULL,
        check=True,
    )


def start_emulator(gles: bool) -> subprocess.Popen[bytes]:
    """Cold-boots the emulator headless, without Vulkan when `gles`, and waits for it."""
    ensure_avd()
    started = time.monotonic()
    proc = subprocess.Popen(
        [
            *[str(mobile.ANDROID_HOME / "emulator" / "emulator"), "-avd", AVD],
            *["-port", str(PORT), "-no-window", "-no-audio", "-no-boot-anim"],
            "-no-snapshot",
            *(NO_VULKAN if gles else ()),
        ],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        env=mobile.android_env(),
    )
    while adb("shell", "getprop", "sys.boot_completed").stdout.strip() != "1":
        if proc.poll() is not None:
            raise SystemExit(f"android: the emulator exited ({proc.returncode})")
        if time.monotonic() - started > REPLAY_LIMIT_S:
            proc.kill()
            raise SystemExit(
                f"android: the emulator did not boot in {REPLAY_LIMIT_S} s"
            )
        time.sleep(1)
    print(f"android: emulator booted in {time.monotonic() - started:.0f} s")
    return proc


def adb_checked(*args: str) -> str:
    result = adb(*args)
    if result.returncode:
        raise SystemExit(f"adb {' '.join(args)}: {result.stderr or result.stdout}")
    return result.stdout.strip()


@contextmanager
def locked_rotation() -> Iterator[None]:
    """Start in the emulator's natural orientation and restore its settings on exit."""
    settings = ("accelerometer_rotation", "user_rotation")
    previous = {
        name: adb_checked("shell", "settings", "get", "system", name)
        for name in settings
    }
    try:
        adb_checked("shell", "settings", "put", "system", "accelerometer_rotation", "0")
        set_rotation("0")
        yield
    finally:
        for name in reversed(settings):
            value = previous[name]
            if value == "null":
                adb_checked("shell", "settings", "delete", "system", name)
            else:
                adb_checked("shell", "settings", "put", "system", name, value)


def set_rotation(rotation: str) -> None:
    adb_checked("shell", "settings", "put", "system", "user_rotation", rotation)


def last_frame_callback(pid: str) -> tuple[int, int]:
    """The debug host logs each callback's serial and wake, even when Rust draws nothing."""
    trace = adb_checked(
        "logcat", "-d", "--pid", pid, "-v", "raw", "-s", "Beryl3dFrames:D"
    )
    for line in reversed(trace.splitlines()):
        fields = line.split()
        if len(fields) == 2:
            return int(fields[0]), int(fields[1])
    return 0, 0


def android_input_focused() -> bool:
    # Android can resume and draw the activity before its window receives input focus.
    state = adb_checked("shell", "dumpsys", "input")
    focused = state.partition("\n  FocusedWindows:\n")[2].partition(
        "\n  FocusRequests:"
    )[0]
    return f"{BUNDLE}/{BUNDLE}.MainActivity" in focused


def android_touch_smoke(
    read: Callable[[], str | None],
    errors: Callable[[], str],
    label: str,
) -> tuple[str | None, list[str]]:
    pid: str = adb("shell", "pidof", BUNDLE).stdout.strip()
    if not pid:
        return read(), [errors() or "the Android host did not start"]

    def alive() -> bool:
        return adb("shell", "pidof", BUNDLE).stdout.strip() == pid

    log = wait_for_log(read, alive, errors, label, event="first_shown_ms")
    if log is None:
        return None, ["the first frame was never shown"]
    deadline = time.monotonic() + REPLAY_LIMIT_S
    while not android_input_focused():
        if not alive() or errors() or time.monotonic() >= deadline:
            return read(), [errors() or "the Android window never received input focus"]
        time.sleep(1)

    surface = logged_inputs(lines(log), "surface")[-1]
    original = (surface["width"], surface["height"])
    # The emulator's status bar can extend below y=100.
    tap_y = max(100, round((surface["top"] + 32) * surface["scale"]))
    adb_checked("shell", "input", "tap", "100", str(tap_y))
    adb_checked("shell", "input", "swipe", "100", "300", "200", "300", "500")
    adb_checked("shell", "input", "keyevent", "KEYCODE_BACK")
    activities = adb_checked("shell", "dumpsys", "activity", "activities")
    if not alive() or not any(
        "topResumedActivity" in line and ACTIVITY in line
        for line in activities.splitlines()
    ):
        return log, ["Back took the activity out of the foreground"]

    set_rotation("1")
    swapped = (original[1], original[0])
    if (
        wait_for_log(
            read,
            alive,
            errors,
            label,
            event="rotated surface",
            ready=lambda parsed: swapped in surface_sizes(parsed),
        )
        is None
    ):
        return read(), ["the surface did not rotate"]

    set_rotation("0")
    log = wait_for_log(
        read,
        alive,
        errors,
        label,
        event="replay summary and restored surface",
        ready=lambda parsed: rotated_and_returned(parsed, original)
        and any("summary" in line for line in parsed),
    )
    if log is None:
        return read(), [
            "the replay or return to the original orientation did not finish"
        ]

    deadline = time.monotonic() + REPLAY_LIMIT_S
    while time.monotonic() < deadline and alive():
        callback, wake = last_frame_callback(pid)
        if wake == -1:
            time.sleep(2)
            end, _ = last_frame_callback(pid)
            log = read()
            faults = android_touch_faults(log or "", original, callback, end)
            reported = errors()
            if reported or not alive():
                faults.append(reported or "the app stopped during the idle check")
            return log, faults
        time.sleep(1)
    return read(), ["the host did not become idle"]


def android_smoke_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--fixture", default=FIXTURE, help="the recording to replay")
    scenario = parser.add_mutually_exclusive_group()
    scenario.add_argument(
        "--touch",
        action="store_true",
        help="send touch and Back input, rotate the surface, and verify that frames stop when idle",
    )
    scenario.add_argument(
        "--lifecycle",
        action="store_true",
        help="suspend with Home, resume on a new surface, and check low memory and platform requests",
    )
    parser.add_argument(
        "--gles",
        action="store_true",
        help="boot the emulator without Vulkan, so wgpu draws with OpenGL ES 3.0",
    )


def run_android_smoke(args: argparse.Namespace) -> int:
    from . import android_lifecycle  # Uses the emulator helpers above.

    label = (
        "android-smoke"
        + (" --touch" if args.touch else "")
        + (" --lifecycle" if args.lifecycle else "")
        + (" --gles" if args.gles else "")
    )
    run = out_dir("phone") / f"{run_name()}-android{'-gles' if args.gles else ''}"
    run.mkdir()
    (fixture,) = write_fixtures((args.fixture,), run)
    with system.file_lock(paths.cache() / "android-phone.lock"):
        apk = android_apk()
        if adb("get-state").stdout.strip() == "device":
            # A run that was killed left its emulator behind, booted with options this run
            # may not want.
            adb("emu", "kill")
            time.sleep(5)
        emulator = start_emulator(args.gles)
        try:
            installed = adb("install", "--no-streaming", "-r", str(apk))
            if installed.returncode != 0:
                raise SystemExit(f"android: install failed: {installed.stderr}")
            shell: list[str] = ["shell", "run-as", BUNDLE, "sh", "-c"]
            adb(
                *shell,
                f"'mkdir -p files && rm -f files/*.jsonl files/{ERRORS} files/{LIBRARY_LOG} files/host.log'",
            )
            adb(*shell, f"'cat > files/{fixture.name}'", stdin=fixture)

            def read() -> str | None:
                text = adb(*shell, "'cat files/*.jsonl 2>/dev/null'").stdout
                return text or None

            def alive() -> bool:
                return bool(adb("shell", "pidof", BUNDLE).stdout.strip())

            def errors() -> str:
                # wgpu's warnings only count beside an error, which alone stops the app.
                both = f"files/{ERRORS} files/{LIBRARY_LOG}"
                text = adb(
                    *shell, f"'test -s files/{ERRORS} && cat {both} 2>/dev/null'"
                ).stdout
                if args.lifecycle and not text:
                    text = adb(
                        *shell, f"'grep ^ERROR files/{LIBRARY_LOG} 2>/dev/null'"
                    ).stdout
                crash = adb("logcat", "-d", "-b", "crash").stdout
                events = adb("logcat", "-d", "-b", "events", "-s", "am_anr:I").stdout
                anr = "\n".join(line for line in events.splitlines() if BUNDLE in line)
                # The head of a crash holds the signal, the abort message and the top frames.
                return "\n".join(part for part in (text, crash[:4000], anr) if part)

            with locked_rotation() if args.touch else nullcontext():
                adb_checked(
                    *["shell", "am", "start", "-W", "-S", "-n", ACTIVITY],
                    *["--es", "fixture", args.fixture],
                    *(["--ez", "traceFrames", "true"] if args.touch else []),
                )
                if args.touch:
                    log, faults = android_touch_smoke(read, errors, label)
                elif args.lifecycle:
                    log, faults = android_lifecycle.exercise(read, errors, label)
                else:
                    log, faults = wait_for_log(read, alive, errors, label), []
            png = run / "screen.png"
            png.write_bytes(
                subprocess.run(
                    [
                        *[str(mobile.ANDROID_HOME / "platform-tools" / "adb")],
                        *["-s", SERIAL, "exec-out", "screencap", "-p"],
                    ],
                    capture_output=True,
                    check=False,
                ).stdout
            )
            if args.lifecycle:
                if not faults:
                    fault = android_lifecycle.request("open-url")
                    if fault:
                        faults.append(fault)
                    log = read()
                host = android_lifecycle.host_log()
                (run / "host.log").write_text(host, encoding="utf-8")
                faults.extend(lifecycle_faults(log or "", host))
                if reported := errors():
                    faults.append(reported)
                library_log = adb(*shell, f"'cat files/{LIBRARY_LOG}'").stdout
                (run / LIBRARY_LOG).write_text(library_log, encoding="utf-8")
        finally:
            adb("emu", "kill")
            emulator.wait(timeout=REPLAY_LIMIT_S)
    if faults:
        if log is not None:
            png.with_suffix(".jsonl").write_text(log, encoding="utf-8")
        for fault in faults:
            print(f"{label}: FAILED: {fault}")
        print(f"{label}: screen {png}")
        return 1
    return finish(label, log, png)


def checked(
    run: Callable[[argparse.Namespace], int],
) -> Callable[[argparse.Namespace], int]:
    """`run`, with a build or tool that cannot run reported as `checks.CANNOT_RUN`."""

    def wrapped(args: argparse.Namespace) -> int:
        try:
            return run(args)
        except SystemExit as error:
            if isinstance(error.code, str):
                print(error.code, file=sys.stderr)
                return checks.CANNOT_RUN
            raise

    return wrapped


def _later(module: str, name: str) -> Any:
    """`name` of the sibling `module`, looked up when called: those modules import this one."""

    def call(*args: Any) -> Any:
        return getattr(importlib.import_module(f".{module}", __package__), name)(*args)

    return call


COMMANDS: tuple[Command, ...] = (
    Command(
        "ios-smoke",
        "replays a recording in the app's iOS host in the headless Simulator, and saves the screen",
        (
            (
                "just b3d ios-smoke --touch",
                "touch, rotate and idle a live game through a UI test",
            ),
            (
                "just b3d ios-smoke --lifecycle",
                "suspend and resume a replay, trim memory and exercise platform requests",
            ),
        ),
        checked(_later("ios_smoke", "run_ios_smoke")),
        _later("ios_smoke", "ios_smoke_arguments"),
    ),
    Command(
        "android-smoke",
        "replays a recording in the app's Android host in a headless emulator, on Vulkan or GLES",
        (
            (
                "just b3d android-smoke --touch",
                "replay verbs-zones on Vulkan and check touch, Back, rotation and idle",
            ),
            (
                "just b3d android-smoke --lifecycle --gles",
                "check Android lifecycle and requests on OpenGL ES 3.0",
            ),
        ),
        checked(run_android_smoke),
        android_smoke_arguments,
    ),
    Command(
        "phone-search",
        "plays a live game against mcts-hidden on the iOS Simulator and the Android emulator, and"
        " compares late frames while the AI searched and while it did not",
        (
            ("just b3d phone-search", "iOS, then Android on Vulkan"),
            (
                "just b3d phone-search --gles",
                "iOS, then Android on Vulkan and on OpenGL ES 3.0",
            ),
        ),
        checked(_later("phone_search", "run_phone_search")),
        _later("phone_search", "phone_search_arguments"),
    ),
    Command(
        "ios-device",
        "installs the app's iOS host on the paired iPhone and replays two recordings, when asked",
        (
            (
                "just b3d ios-device --user-asked",
                "the user asked for it: install and replay",
            ),
            ("just b3d ios-device", "a person's run, which needs no flag"),
        ),
        checked(run_ios_device),
        ios_device_arguments,
    ),
)
