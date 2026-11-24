"""Build the native phone hosts and package their Rust libraries."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
from pathlib import Path
from typing import Any

from .. import android_sdk, mobile_sdk, system
from . import paths

CRATE: Path = paths.WORKSPACE / "crates" / "beryl_client"
IOS_SIMULATOR = "iphonesimulator"
ANDROID: Path = CRATE / "android"
XCODE_LIMIT_S = 600


def build_dir(platform: str) -> Path:
    """Keep host projects and Cargo artifacts from different checkouts apart."""
    path = paths.cache() / paths.build_lock().stem / platform
    path.mkdir(parents=True, exist_ok=True)
    return path


def worker_limit(flag: str) -> list[str]:
    cores = os.environ.get("IGNITE_CORES")
    return [flag, cores] if cores else []


def library(target: str, crate_type: str, release: bool, env: dict[str, str]) -> Path:
    """Builds beryl_client's library for `target` as a `staticlib` or `cdylib`, and returns the
    file. Cargo leaves a crate type the manifest does not declare in `deps`, so its path is
    read from cargo's messages."""
    command = [
        *["cargo", "rustc", "-p", "beryl_client", "--lib", "--crate-type", crate_type],
        *[
            "--locked",
            "--target",
            target,
            "--message-format",
            "json-render-diagnostics",
        ],
        *(["--release"] if release else []),
    ]
    started = time.monotonic()
    with system.file_lock(paths.build_lock()):
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
            and message.get("target", {}).get("name") == "beryl_client"
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


def ios_project(root: Path) -> Path:
    project = root / "project"
    project.mkdir(exist_ok=True)
    generated = mobile_sdk.run(
        "xcodegen",
        "generate",
        "--spec",
        str(CRATE / "ios" / "project.yml"),
        "--project",
        str(project),
    )
    if generated.returncode != 0:
        raise SystemExit(f"xcodegen failed: {generated.stdout}{generated.stderr}")
    return project / "Beryl.xcodeproj"


def simulator_ready() -> bool:
    root = build_dir("ios")
    with system.file_lock(root / "build.lock"):
        return mobile_sdk.can_build_for_simulator(ios_project(root), "Beryl")


def ios_bundle(sdk: str) -> Path:
    return (
        build_dir("ios")
        / "derived"
        / "Build"
        / "Products"
        / f"Debug-{sdk}"
        / "Beryl.app"
    )


def android_bundle() -> Path:
    return (
        build_dir("android")
        / "build"
        / "app"
        / "outputs"
        / "apk"
        / "debug"
        / "app-debug.apk"
    )


def ios_app(sdk: str, release: bool, sign: bool, tests: bool = False) -> Path:
    """Builds the library and the Swift host for `sdk` (iphonesimulator or iphoneos), and
    returns the app. With `tests`, also builds the UI test bundle, and the `.xctestrun` file
    that runs it is in the app's products folder's parent."""
    root = build_dir("ios")
    with system.file_lock(root / "build.lock"):
        target = (
            "aarch64-apple-ios-sim" if sdk == IOS_SIMULATOR else "aarch64-apple-ios"
        )
        env = {**os.environ, "IPHONEOS_DEPLOYMENT_TARGET": "15.0"}
        built = library(target, "staticlib", release, env)
        lib = root / sdk / "lib"
        lib.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(built, lib / "libberyl_client.a")
        project = ios_project(root)
        signing = ["-allowProvisioningUpdates"] if sign else ["CODE_SIGNING_ALLOWED=NO"]
        platform = "iOS Simulator" if sdk == IOS_SIMULATOR else "iOS"
        started = time.monotonic()
        code = subprocess.run(
            [
                *["xcodebuild", "-quiet", "-project", str(project)],
                *["-scheme", "Beryl", "-sdk", sdk, "-configuration", "Debug"],
                *["-destination", f"generic/platform={platform}"],
                *["-derivedDataPath", str(root / "derived"), *signing],
                *worker_limit("-jobs"),
                f"BERYL_CLIENT_LIB_DIR={lib}",
                f"BERYL_CLIENT_CRATE_DIR={CRATE}",
                "build-for-testing" if tests else "build",
            ],
            check=False,
            timeout=XCODE_LIMIT_S,
        ).returncode
        if code != 0:
            raise SystemExit("xcodebuild failed")
        print(f"ios: host built in {time.monotonic() - started:.0f} s")
        return ios_bundle(sdk)


def android_apk() -> Path:
    """Builds the library and the Kotlin host, and returns the APK."""
    root = build_dir("android")
    with system.file_lock(root / "build.lock"):
        prebuilt = android_sdk.ndk_tools().parent
        built = library(android_sdk.TARGET, "cdylib", False, android_sdk.native_env())
        jni = root / "jniLibs" / "arm64-v8a"
        jni.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(built, jni / "libberyl_client.so")
        # The texture encoders (ctt) are C++ and link the NDK's shared C++ runtime, which an app
        # must bundle beside its library (docs/beryl-3d.md, "Preparation").
        runtime = prebuilt / "sysroot" / "usr" / "lib" / "aarch64-linux-android"
        shutil.copyfile(runtime / "libc++_shared.so", jni / "libc++_shared.so")
        started = time.monotonic()
        wrapper = "gradlew.bat" if system.WINDOWS else "gradlew"
        code = subprocess.run(
            [
                *[
                    str(ANDROID / wrapper),
                    "--quiet",
                    "-p",
                    str(ANDROID),
                    "assembleDebug",
                ],
                *worker_limit("--max-workers"),
                *["--project-cache-dir", str(root / "gradle")],
                f"-Pberyl.buildDir={root / 'build'}",
                f"-Pberyl.jniLibs={root / 'jniLibs'}",
                f"-Pkotlin.project.persistent.dir={root / 'kotlin'}",
            ],
            env=mobile_sdk.android_env(),
            check=False,
        ).returncode
        if code != 0:
            raise SystemExit("gradle failed")
        print(f"android: host built in {time.monotonic() - started:.0f} s")
        return android_bundle()
