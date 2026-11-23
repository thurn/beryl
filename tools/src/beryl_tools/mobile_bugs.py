"""Pulls phone bug reports into the Mac's local bugs folder without deleting device data."""

from __future__ import annotations

import argparse
import io
import json
import shutil
import subprocess
import sys
import tarfile
import tempfile
from collections.abc import Callable
from pathlib import Path
from typing import Any

from . import bugs, mobile_sdk

ReportCounts = tuple[int, int]


def copy_reports(source: Path, root: Path) -> tuple[int, int]:
    """Stages and adopts report folders in a mounted simulator or device container."""
    brought = already = 0
    if not source.is_dir():
        return brought, already
    root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=root, prefix=".mobile-bugs-") as temp:
        for folder in sorted(source.iterdir()):
            if (
                folder.is_symlink()
                or not folder.is_dir()
                or not bugs.REPORT_ID.fullmatch(folder.name)
            ):
                continue
            staged = Path(temp) / folder.name
            staged.mkdir()
            for file in folder.iterdir():
                if (
                    file.name in bugs.BUNDLE_FILES
                    and file.is_file()
                    and not file.is_symlink()
                ):
                    shutil.copy2(file, staged / file.name)
            try:
                adopted = bugs.adopt_report(staged, root)
            except ValueError as error:
                print(f"mobile-bugs: skipping {folder.name}: {error}", file=sys.stderr)
                continue
            if adopted:
                brought += 1
            else:
                already += 1
    return brought, already


def simulator_container(bundle: str) -> Path | None:
    """Returns only an already booted Beryl simulator's app data container."""
    listed = mobile_sdk.run("xcrun", "simctl", "list", "devices", "--json")
    if listed.returncode != 0:
        return None
    devices: dict[str, Any] = json.loads(listed.stdout).get("devices", {})
    for runtime in devices.values():
        for device in runtime:
            if (
                device.get("name") == mobile_sdk.SIMULATOR
                and device.get("state") == "Booted"
            ):
                data = mobile_sdk.run(
                    "xcrun",
                    "simctl",
                    "get_app_container",
                    str(device["udid"]),
                    bundle,
                    "data",
                )
                if data.returncode == 0:
                    return Path(data.stdout.strip())
    return None


def pull_android_archive(
    root: Path, adb: str, serial: str, package: str, source_dir: str
) -> ReportCounts:
    command = [
        adb,
        "-s",
        serial,
        "exec-out",
        "run-as",
        package,
        "tar",
        "c",
        "-C",
        source_dir,
        ".",
    ]
    archived = subprocess.run(command, capture_output=True, check=False)
    if archived.returncode != 0:
        raise RuntimeError(archived.stderr.decode(errors="replace"))
    root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=root, prefix=".mobile-bugs-") as temp:
        staging = Path(temp)
        with tarfile.open(fileobj=io.BytesIO(archived.stdout), mode="r:") as archive:
            members = archive.getmembers()
            if (
                len(members) > 10_000
                or sum(member.size for member in members) > 1024**3
            ):
                raise ValueError("oversized Android report archive")
            for member in members:
                parts = Path(member.name).parts
                if parts and parts[0] == ".":
                    parts = parts[1:]
                if len(parts) != 2 or not member.isfile():
                    continue
                key, name = parts
                if not bugs.REPORT_ID.fullmatch(key) or name not in bugs.BUNDLE_FILES:
                    continue
                destination = staging / key / name
                destination.parent.mkdir(exist_ok=True)
                source = archive.extractfile(member)
                if source is None:
                    raise ValueError(f"missing Android report file {member.name}")
                with source, destination.open("wb") as target:
                    shutil.copyfileobj(source, target)
        brought = already = 0
        for folder in sorted(staging.iterdir()):
            try:
                adopted = bugs.adopt_report(folder, root)
            except ValueError as error:
                print(f"mobile-bugs: skipping {folder.name}: {error}", file=sys.stderr)
                continue
            if adopted:
                brought += 1
            else:
                already += 1
        return brought, already


def device_reports(root: Path, bundle: str, source: Path) -> ReportCounts | None:
    """Copies from a paired iPhone only when the caller explicitly asked for it."""
    device = mobile_sdk.paired_device()
    if device is None:
        return None
    root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=root, prefix=".mobile-bugs-device-") as temp:
        destination = Path(temp) / "bugs"
        copied = subprocess.run(
            [
                "xcrun",
                "devicectl",
                "device",
                "copy",
                "from",
                "--device",
                device,
                "--domain-type",
                "appDataContainer",
                "--domain-identifier",
                bundle,
                "--source",
                source.as_posix(),
                "--destination",
                str(destination),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        if copied.returncode != 0:
            raise RuntimeError(copied.stderr.strip())
        # devicectl versions may place the received directory at the destination or inside it.
        received = (
            destination / "bugs" if (destination / "bugs").is_dir() else destination
        )
        return copy_reports(received, root)


def simulator_reports(root: Path, bundle: str, source: Path) -> ReportCounts | None:
    container = simulator_container(bundle)
    return copy_reports(container / source, root) if container is not None else None


def pull(
    args: argparse.Namespace,
    simulator: Callable[[Path], ReportCounts | None],
    android: Callable[[Path], ReportCounts | None],
    device: Callable[[Path], ReportCounts | None],
) -> int:
    if args.bundle is not None:
        return bugs.import_cmd(argparse.Namespace(bundle=args.bundle))
    root = bugs.bugs_dir()
    sources = [
        ("iOS Simulator", simulator, "no booted Beryl simulator or installed app"),
        ("Android emulator", android, "not running or Beryl is not installed"),
    ]
    if args.device:
        sources.append(("iPhone", device, "no paired device"))
    failed = False
    for label, fetch, missing in sources:
        try:
            counts = fetch(root)
            if counts is None:
                print(f"{label}: {missing}")
            else:
                print_counts(label, counts)
        except (OSError, ValueError, RuntimeError, tarfile.TarError) as error:
            print(f"mobile-bugs: {label}: {error}", file=sys.stderr)
            failed = True
    return int(failed)


def print_counts(source: str, counts: tuple[int, int]) -> None:
    print(f"{source}: {counts[0]} brought, {counts[1]} already saved")


def arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("bundle", nargs="?", type=Path, help="shared report ZIP")
    parser.add_argument(
        "--device", action="store_true", help="also pull from a paired iPhone"
    )
