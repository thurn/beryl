"""Beryl 3D's phone spike (docs/beryl-3d.md, 'Hosts'): `just b3d ios-smoke` and
`just b3d android-smoke` build the app's library and its small Swift or Kotlin host, install it
in the headless iOS Simulator or Android emulator, replay a recording, wait for its frame log
to say the first frame was shown and the recording's last script ended, save a PNG of the
screen and exit. `just b3d ios-device` does the same on the user's paired iPhone, and only when
he asked for it.

A phone has no recordings pack and no engine to build one, so each recording is written to a
file on the Mac (`beryl3d --fixture <name> --write-fixture <path>`) and copied into the app's
data folder, where the app reads it and writes its frame log.

These commands are in neither the lane nor the sweep: milestone 4 adds them.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

from .. import checks, mobile, paths as repo_paths, system
from . import app, paths

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


def lines(text: str) -> list[dict[str, Any]]:
    """A frame log's lines, skipping a torn last line."""
    parsed = []
    for line in text.splitlines():
        try:
            parsed.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return parsed


def wait_for_log(
    read: Callable[[], str | None],
    alive: Callable[[], bool],
    errors: Callable[[], str],
    label: str,
    limit_s: int = REPLAY_LIMIT_S,
) -> str | None:
    """Polls the app's frame log with `read` until it holds a summary, which the app writes
    when the recording's last script has ended, and returns the log's text. Prints the GPU
    adapter once the log's first line names it. None, after printing why, when the app died or
    the limit passed."""
    deadline = time.monotonic() + limit_s
    named = False
    while time.monotonic() < deadline:
        text = read()
        parsed = lines(text) if text else []
        if parsed and not named and "run" in parsed[0]:
            named = True
            print(f"{label}: adapter {parsed[0]['run']['adapter']}")
        if any("summary" in line for line in parsed):
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
        time.sleep(1)
    print(f"{label}: FAILED: the recording did not end in {limit_s} s")
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


def ios_app(sdk: str, release: bool, sign: bool) -> Path:
    """Builds the library and the Swift host for `sdk` (iphonesimulator or iphoneos), and
    returns the app."""
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
            "build",
        ],
        check=False,
        timeout=DEVICE_LIMIT_S,
    ).returncode
    if code != 0:
        raise SystemExit("xcodebuild failed")
    print(f"ios: host built in {time.monotonic() - started:.0f} s")
    return root / "derived" / "Build" / "Products" / f"Debug-{sdk}" / "Beryl3D.app"


def ios_smoke_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--fixture", default=FIXTURE, help="the recording to replay")


def run_ios_smoke(args: argparse.Namespace) -> int:
    bundle = ios_app("iphonesimulator", release=False, sign=False)
    run = out_dir("phone") / f"{run_name()}-ios"
    run.mkdir()
    (fixture,) = write_fixtures((args.fixture,), run)
    # The simulator `just ios-smoke` uses, under the same lock, so that neither reinstalls an
    # app under the other or shuts the simulator down while the other runs.
    with system.file_lock(repo_paths.cache_root() / "ios-smoke.lock"):
        udid, booted = mobile.simulator()
        mobile.boot(udid)
        try:
            mobile.install(udid, bundle)
            container = mobile.run(
                "xcrun", "simctl", "get_app_container", udid, BUNDLE, "data"
            )
            documents: Path = Path(container.stdout.strip()) / "Documents"
            documents.mkdir(parents=True, exist_ok=True)
            for old in [
                *documents.glob("*.jsonl"),
                documents / ERRORS,
                documents / LIBRARY_LOG,
            ]:
                old.unlink(missing_ok=True)
            shutil.copyfile(fixture, documents / fixture.name)
            launched = subprocess.run(
                [
                    *["xcrun", "simctl", "launch", "--terminate-running-process"],
                    *[udid, BUNDLE],
                ],
                env={**os.environ, "SIMCTL_CHILD_BERYL3D_FIXTURE": args.fixture},
                capture_output=True,
                text=True,
                check=True,
            )
            pid = int(launched.stdout.rsplit(":", 1)[1])

            def read() -> str | None:
                logs = sorted(documents.glob("*.jsonl"))
                return logs[-1].read_text(encoding="utf-8") if logs else None

            def errors() -> str:
                path = documents / ERRORS
                if not path.exists():
                    return ""
                library_log = documents / LIBRARY_LOG
                warned = (
                    library_log.read_text(encoding="utf-8")
                    if library_log.exists()
                    else ""
                )
                return path.read_text(encoding="utf-8") + warned

            log = wait_for_log(read, lambda: mobile.alive(pid), errors, "ios-smoke")
            png = run / "screen.png"
            mobile.run("xcrun", "simctl", "io", udid, "screenshot", str(png))
        finally:
            mobile.run("xcrun", "simctl", "terminate", udid, BUNDLE)
            if not booted:
                mobile.run("xcrun", "simctl", "shutdown", udid)
    return finish("ios-smoke", log, png)


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
    started = time.monotonic()
    code = subprocess.run(
        [
            *[str(GRADLEW), "--quiet", "-p", str(CRATE / "android"), "assembleDebug"],
            *["--project-cache-dir", str(root / "gradle")],
            f"-Pberyl3d.buildDir={root / 'build'}",
            f"-Pberyl3d.jniLibs={root / 'jniLibs'}",
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


def android_smoke_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--fixture", default=FIXTURE, help="the recording to replay")
    parser.add_argument(
        "--gles",
        action="store_true",
        help="boot the emulator without Vulkan, so wgpu draws with OpenGL ES 3.0",
    )


def run_android_smoke(args: argparse.Namespace) -> int:
    apk = android_apk()
    label = "android-smoke" + (" --gles" if args.gles else "")
    run = out_dir("phone") / f"{run_name()}-android{'-gles' if args.gles else ''}"
    run.mkdir()
    (fixture,) = write_fixtures((args.fixture,), run)
    with system.file_lock(paths.cache() / "android-phone.lock"):
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
                f"'mkdir -p files && rm -f files/*.jsonl files/{ERRORS} files/{LIBRARY_LOG}'",
            )
            adb(*shell, f"'cat > files/{fixture.name}'", stdin=fixture)
            adb(
                *["shell", "am", "start", "-W", "-S", "-n", ACTIVITY],
                *["--es", "fixture", args.fixture],
            )

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
                crash = adb("logcat", "-d", "-b", "crash").stdout
                # The head of a crash holds the signal, the abort message and the top frames.
                return "\n".join(part for part in (text, crash[:4000]) if part)

            log = wait_for_log(read, alive, errors, label)
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
        finally:
            adb("emu", "kill")
            emulator.wait(timeout=REPLAY_LIMIT_S)
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
