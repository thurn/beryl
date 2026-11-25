"""The Android SDK tools and native targets selected by the phone commands."""

import subprocess
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import pytest

from beryl_tools import android_sdk, mobile_sdk, system
from beryl_tools.client import phone


@pytest.mark.parametrize(
    ("machine", "abi", "abis"),
    [
        ("AMD64", "x86_64", ("arm64-v8a", "x86_64")),
        ("x86_64", "x86_64", ("arm64-v8a", "x86_64")),
        ("arm64", "arm64-v8a", ("arm64-v8a",)),
        ("aarch64", "arm64-v8a", ("arm64-v8a",)),
    ],
)
def test_system_image_and_apk_use_the_host_abi(
    machine: str, abi: str, abis: tuple[str, ...], monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(android_sdk.platform, "machine", lambda: machine)

    assert (
        mobile_sdk.android_system_image()
        == f"system-images;android-36;google_apis;{abi}"
    )
    assert android_sdk.build_abis() == abis


def test_windows_setup_installs_the_full_emulator_sdk(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sdk = tmp_path / "Android SDK"
    manager = sdk / "cmdline-tools/latest/bin/sdkmanager.bat"
    manager.parent.mkdir(parents=True)
    manager.touch()
    java = tmp_path / "JDK 21"
    (java / "bin").mkdir(parents=True)
    (java / "bin" / "java.exe").touch()
    monkeypatch.setenv("JAVA_HOME", str(java))
    monkeypatch.setattr(mobile_sdk, "ANDROID_HOME", sdk)
    monkeypatch.setattr(system, "WINDOWS", True)
    monkeypatch.setattr(android_sdk.platform, "machine", lambda: "AMD64")

    def rust_targets(targets: tuple[str, ...]) -> int:
        assert targets == ("aarch64-linux-android", "x86_64-linux-android")
        return 0

    calls: list[list[str]] = []

    def run(command: list[str], **_kwargs: Any) -> subprocess.CompletedProcess[str]:
        calls.append(command)
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(mobile_sdk, "rust_targets", rust_targets)
    monkeypatch.setattr(subprocess, "run", run)

    assert mobile_sdk.android_setup() == 0
    command = [str(manager), f"--sdk_root={sdk}"]
    assert calls == [
        [*command, "--licenses"],
        [
            *command,
            "cmdline-tools;latest",
            "platform-tools",
            "emulator",
            "platforms;android-36",
            "build-tools;36.0.0",
            f"ndk;{android_sdk.NDK_VERSION}",
            "system-images;android-36;google_apis;x86_64",
        ],
    ]

    # SDK Manager installs next to manually unpacked command-line tools.
    for package in calls[-1][2:]:
        folder = sdk.joinpath(*package.split(";"))
        if folder == manager.parent.parent:
            folder = folder.with_name("latest-2")
        folder.mkdir(parents=True)
        (folder / "package.xml").touch()
    calls.clear()

    assert mobile_sdk.android_setup() == 0
    assert not calls


def test_windows_native_toolchain_targets_the_x86_64_emulator(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("ANDROID_HOME", str(tmp_path))
    monkeypatch.setattr(android_sdk, "sys", SimpleNamespace(platform="win32"))
    env = android_sdk.native_env("x86_64-linux-android")
    tools = (
        tmp_path
        / f"ndk/{android_sdk.NDK_VERSION}/toolchains/llvm/prebuilt/windows-x86_64/bin"
    )

    assert Path(env["CC_x86_64_linux_android"]) == tools / "clang.exe"
    assert Path(env["CXX_x86_64_linux_android"]) == tools / "clang++.exe"
    assert Path(env["AR_x86_64_linux_android"]) == tools / "llvm-ar.exe"
    assert env["CFLAGS_x86_64_linux_android"] == "--target=x86_64-linux-android24"
    assert env["CXXFLAGS_x86_64_linux_android"] == env["CFLAGS_x86_64_linux_android"]
    assert (
        env["CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER"]
        == env["CC_x86_64_linux_android"]
    )
    assert env["CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS"] == (
        "-C link-arg=--target=x86_64-linux-android24"
    )


def test_windows_avd_boot_uses_the_tools_under_the_sdk(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sdk = tmp_path / "Android SDK"
    monkeypatch.setattr(mobile_sdk, "ANDROID_HOME", sdk)
    monkeypatch.setattr(system, "WINDOWS", True)
    monkeypatch.setattr(android_sdk.platform, "machine", lambda: "AMD64")
    calls: list[list[str]] = []

    def run(command: list[str], **_kwargs: Any) -> subprocess.CompletedProcess[Any]:
        calls.append(command)
        return subprocess.CompletedProcess(
            command, 0, b"1\n" if "shell" in command else "", b""
        )

    proc: object = object()

    def launch(command: list[str], **_kwargs: Any) -> object:
        calls.append(command)
        return proc

    monkeypatch.setattr(subprocess, "run", run)
    monkeypatch.setattr(subprocess, "Popen", launch)

    assert phone.start_emulator(gles=True) is proc
    emulator = str(sdk / "emulator/emulator.exe")
    assert calls[0] == [emulator, "-list-avds"]
    assert calls[1][0] == str(sdk / "cmdline-tools/latest/bin/avdmanager.bat")
    assert calls[1][calls[1].index("--package") + 1] == (
        "system-images;android-36;google_apis;x86_64"
    )
    assert calls[2][:3] == [emulator, "-avd", phone.AVD]
    assert calls[2][-2:] == ["-feature", "-Vulkan"]
    assert calls[3][:3] == [str(sdk / "platform-tools/adb.exe"), "-s", phone.SERIAL]


def test_adb_output_uses_unix_line_endings(monkeypatch: pytest.MonkeyPatch) -> None:
    # adb on Windows returns the device's lines with CRLF endings.
    dump: bytes = (
        b"Input Dispatcher State:\r\n  FocusedWindows:\r\n    name='ca.thurn.beryl'\r\n  FocusRequests:\r\n"
    )

    def run(command: list[str], **_kwargs: Any) -> subprocess.CompletedProcess[Any]:
        return subprocess.CompletedProcess(command, 0, dump, b"")

    monkeypatch.setattr(subprocess, "run", run)

    output = phone.adb("shell", "dumpsys", "input").stdout
    assert output == dump.decode().replace("\r\n", "\n")
    assert "\n  FocusedWindows:\n" in output


def test_crash_reports_keep_only_the_app() -> None:
    log = "\n".join(
        [
            "--------- beginning of crash",
            "11-24 20:12:31.624  2250  2424 F libc    : Fatal signal 6 (SIGABRT), code -6"
            " in tid 2424 (EventBus0), pid 2250 (earchbox:search)",
            "11-24 20:12:37.662  2554  2554 F DEBUG   : Cmdline:"
            " com.google.android.googlequicksearchbox:search",
            "11-24 20:12:37.662  2554  2554 F DEBUG   :       #00 pc 0000000000062b0f",
            "11-24 20:13:01.100  3001  3020 F libc    : Fatal signal 11 (SIGSEGV), code 1"
            " in tid 3020 (main), pid 3001 (ca.thurn.beryl)",
            "11-24 20:13:02.200  3050  3050 F DEBUG   : Cmdline: ca.thurn.beryl",
            "11-24 20:13:02.200  3050  3050 F DEBUG   :       #00 pc 00000000001234ab",
        ]
    )

    reports = phone.app_crash_reports(log)

    assert len(reports) == 2
    assert "SIGSEGV" in reports[0]
    assert reports[1].endswith("#00 pc 00000000001234ab")
    assert "earchbox" not in "\n".join(reports)
