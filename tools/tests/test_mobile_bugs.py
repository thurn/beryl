"""Phone report transfers keep local triage authoritative and never remove device files."""

from __future__ import annotations

import argparse
import io
import tarfile
from pathlib import Path
from subprocess import CompletedProcess

import pytest

from beryl_tools import android, bugs, mobile, mobile_bugs

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
    monkeypatch.setattr(android, "running", lambda: True)
    monkeypatch.setattr(
        android, "adb", lambda *args: CompletedProcess(args, 0, f"./files/bugs\n", "")
    )
    commands: list[list[str]] = []

    def fake_run(command: list[str], **_kwargs: object) -> CompletedProcess[bytes]:
        commands.append(command)
        return CompletedProcess[bytes](command, 0, payload.getvalue(), b"")

    monkeypatch.setattr(mobile_bugs.subprocess, "run", fake_run)
    assert mobile_bugs.android_reports(tmp_path / "local") == (1, 0)
    assert commands[0] == [
        android.tool("platform-tools", "adb"),
        "-s",
        android.SERIAL,
        "exec-out",
        "run-as",
        android.PACKAGE,
        "tar",
        "c",
        "-C",
        "./files/bugs",
        ".",
    ]
    assert (tmp_path / "local" / ID / "window.png").read_bytes() == b"PNG"
    assert source.joinpath("report.md").is_file()


@pytest.mark.parametrize("nested", [False, True])
def test_device_copy_command_is_only_used_on_explicit_path(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, nested: bool
) -> None:
    monkeypatch.setattr(mobile, "paired_device", lambda: "paired-id")
    commands: list[list[str]] = []

    def fake_run(command: list[str], **_kwargs: object) -> CompletedProcess[str]:
        commands.append(command)
        destination = Path(command[-1])
        report(destination / "bugs" if nested else destination)
        return CompletedProcess[str](command, 0, "", "")

    monkeypatch.setattr(mobile_bugs.subprocess, "run", fake_run)
    assert mobile_bugs.device_reports(tmp_path / "local") == (1, 0)
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
        mobile.BUNDLE_ID,
        "--source",
        str(mobile_bugs.BUGS_RELATIVE),
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
    assert mobile_bugs.run(argparse.Namespace(bundle=bundle, device=False)) == 0
    assert bundles == [bundle]


def test_one_source_failure_does_not_stop_other_sources(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls: list[str] = []
    monkeypatch.setattr(bugs, "bugs_dir", lambda: tmp_path)

    def failed_simulator() -> Path:
        calls.append("simulator")
        raise RuntimeError("simulator copy failed")

    def working_android(_root: Path) -> tuple[int, int]:
        calls.append("android")
        return (1, 0)

    monkeypatch.setattr(mobile_bugs, "simulator_container", failed_simulator)
    monkeypatch.setattr(mobile_bugs, "android_reports", working_android)

    def working_device(_root: Path) -> tuple[int, int]:
        calls.append("device")
        return (1, 0)

    monkeypatch.setattr(mobile_bugs, "device_reports", working_device)
    assert mobile_bugs.run(argparse.Namespace(bundle=None, device=True)) == 1
    assert set(calls) == {"simulator", "android", "device"}
