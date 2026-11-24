"""Phone report transfers keep local triage authoritative and never remove device files."""

from __future__ import annotations

import argparse
import io
import json
import tarfile
from pathlib import Path
from subprocess import CompletedProcess

import pytest

from beryl_tools import bugs, mobile_bugs, mobile_sdk
from beryl_tools.client import bug, phone
from beryl_tools import cli

ID = "20261113-102030-phone-report"


def report(root: Path) -> Path:
    folder = root / ID
    folder.mkdir(parents=True)
    (folder / "report.md").write_text(f"---\nid: {ID}\nstatus: open\n---\nphone report")
    (folder / "window.png").write_bytes(b"PNG")
    return folder


def test_adopts_once_and_skips_a_done_report(tmp_path: Path) -> None:
    source = tmp_path / "phone"
    original = report(source)
    root = tmp_path / "local"
    assert mobile_bugs.copy_reports(source, root) == (1, 0)
    assert original.joinpath("report.md").is_file()
    assert mobile_bugs.copy_reports(source, root) == (0, 1)
    (root / "done").mkdir()
    (root / ID).rename(root / "done" / ID)
    assert mobile_bugs.copy_reports(source, root) == (0, 1)
    assert bugs.load(root / "done" / ID) is not None


def test_skips_a_symlinked_report_folder(tmp_path: Path) -> None:
    source = tmp_path / "phone"
    source.mkdir()
    outside = report(tmp_path / "outside")
    (source / ID).symlink_to(outside, target_is_directory=True)
    assert mobile_bugs.copy_reports(source, tmp_path / "local") == (0, 0)


def test_android_tar_uses_run_as_and_adopts_report(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = report(tmp_path / "emulator")
    invalid = source.parent / "20261113-102031-empty-report"
    invalid.mkdir()
    (invalid / "report.md").write_text("")
    payload: io.BytesIO = io.BytesIO()
    with tarfile.open(fileobj=payload, mode="w") as archive:
        for folder in (source, invalid):
            for file in folder.iterdir():
                archive.add(file, arcname=f"./{folder.name}/{file.name}")

    def adb(*args: str) -> CompletedProcess[str]:
        output = "device" if args == ("get-state",) else "./files/bugs\n"
        return CompletedProcess[str](args, 0, output, "")

    monkeypatch.setattr(phone, "adb", adb)
    commands: list[list[str]] = []

    def fake_run(command: list[str], **_kwargs: object) -> CompletedProcess[bytes]:
        commands.append(command)
        return CompletedProcess[bytes](command, 0, payload.getvalue(), b"")

    monkeypatch.setattr(mobile_bugs.subprocess, "run", fake_run)
    assert bug.android_reports(tmp_path / "local") == (1, 0)
    assert commands[0] == [
        str(phone.adb_binary()),
        "-s",
        phone.SERIAL,
        "exec-out",
        "run-as",
        phone.BUNDLE,
        "tar",
        "c",
        "-C",
        bug.ANDROID_REPORTS,
        ".",
    ]
    assert (tmp_path / "local" / ID / "window.png").read_bytes() == b"PNG"
    assert source.joinpath("report.md").is_file()


@pytest.mark.parametrize("nested", [False, True])
def test_device_copy_command_is_only_used_on_explicit_path(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, nested: bool
) -> None:
    monkeypatch.setattr(mobile_sdk, "paired_device", lambda: "paired-id")
    commands: list[list[str]] = []

    def fake_run(command: list[str], **_kwargs: object) -> CompletedProcess[str]:
        commands.append(command)
        destination = Path(command[-1])
        report(destination / "bugs" if nested else destination)
        return CompletedProcess[str](command, 0, "", "")

    monkeypatch.setattr(mobile_bugs.subprocess, "run", fake_run)
    assert mobile_bugs.device_reports(
        tmp_path / "local", phone.BUNDLE, bug.IOS_REPORTS
    ) == (1, 0)
    assert commands[0][:7] == [
        "xcrun",
        "devicectl",
        "device",
        "copy",
        "from",
        "--device",
        "paired-id",
    ]
    assert commands[0][-6:-2] == [
        "--domain-identifier",
        phone.BUNDLE,
        "--source",
        "Documents/bugs",
    ]


def test_shared_zip_uses_existing_import(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    bundle = tmp_path / "shared.zip"
    bundles: list[Path] = []

    def fake_import(args: argparse.Namespace) -> int:
        bundles.append(args.bundle)
        return 0

    monkeypatch.setattr(bugs, "import_cmd", fake_import)
    assert cli.main(["mobile-bugs", str(bundle)]) == 0
    assert bundles == [bundle]


def test_one_source_failure_does_not_stop_other_sources(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls: list[str] = []
    monkeypatch.setattr(mobile_bugs.sys, "platform", "darwin")
    monkeypatch.setattr(bugs, "bugs_dir", lambda: tmp_path)

    def failed_simulator(_bundle: str) -> Path:
        calls.append("simulator")
        raise RuntimeError("simulator copy failed")

    def working_android(_root: Path) -> tuple[int, int]:
        calls.append("android")
        return (1, 0)

    monkeypatch.setattr(mobile_bugs, "simulator_container", failed_simulator)
    monkeypatch.setattr(bug, "android_reports", working_android)

    def working_device(_root: Path, _bundle: str, _source: Path) -> tuple[int, int]:
        calls.append("device")
        return (1, 0)

    monkeypatch.setattr(mobile_bugs, "device_reports", working_device)
    assert cli.main(["mobile-bugs", "--device"]) == 1
    assert set(calls) == {"simulator", "android", "device"}


def test_native_mobile_bugs_adopts_from_the_simulator_documents_folder(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    container: Path = tmp_path / "container"
    original = report(container / "Documents" / "bugs")
    local = tmp_path / "local"
    monkeypatch.setattr(mobile_bugs.sys, "platform", "darwin")
    monkeypatch.setattr(bugs, "bugs_dir", lambda: local)

    def simctl(*args: str) -> CompletedProcess[str]:
        if args[2] == "list":
            devices = {
                "devices": {
                    "runtime": [
                        {"name": mobile_sdk.SIMULATOR, "udid": "sim", "state": "Booted"}
                    ]
                }
            }
            return CompletedProcess(args, 0, json.dumps(devices), "")
        assert args == (
            "xcrun",
            "simctl",
            "get_app_container",
            "sim",
            "ca.thurn.beryl",
            "data",
        )
        return CompletedProcess(args, 0, str(container), "")

    monkeypatch.setattr(mobile_sdk, "run", simctl)
    monkeypatch.setattr(
        "beryl_tools.client.phone.adb",
        lambda *args: CompletedProcess(args, 1, "", ""),
    )
    assert cli.main(["client", "mobile-bugs"]) == 0
    assert (local / ID / "window.png").read_bytes() == b"PNG"
    assert (original / "report.md").is_file()


def test_non_macos_skips_ios_sources(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls: list[str] = []
    monkeypatch.setattr(mobile_bugs.sys, "platform", "win32")
    monkeypatch.setattr(bugs, "bugs_dir", lambda: tmp_path)

    def fail_if_called(*_args: object, **_kwargs: object) -> None:
        raise AssertionError("iOS source must not run off macOS")

    def working_android(_root: Path) -> tuple[int, int]:
        calls.append("android")
        return (0, 0)

    monkeypatch.setattr(mobile_bugs, "simulator_container", fail_if_called)
    monkeypatch.setattr(mobile_bugs, "device_reports", fail_if_called)
    monkeypatch.setattr(bug, "android_reports", working_android)
    assert cli.main(["mobile-bugs", "--device"]) == 0
    assert calls == ["android"]
