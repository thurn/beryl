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
from pathlib import Path
from typing import Any

from . import android, bugs, mobile

BUGS_RELATIVE: Path = Path("Library/Application Support") / mobile.BUNDLE_ID / "bugs"


def copy_reports(source: Path, root: Path) -> tuple[int, int]:
    """Stages and adopts report folders in a mounted simulator or device container."""
    brought = already = 0
    if not source.is_dir():
        return brought, already
    root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=root, prefix=".mobile-bugs-") as temp:
        for folder in sorted(source.iterdir()):
            if not folder.is_dir() or not bugs.REPORT_ID.fullmatch(folder.name):
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


def simulator_container() -> Path | None:
    """Returns only an already booted Beryl simulator's app data container."""
    listed = mobile.run("xcrun", "simctl", "list", "devices", "--json")
    if listed.returncode != 0:
        return None
    devices: dict[str, Any] = json.loads(listed.stdout).get("devices", {})
    for runtime in devices.values():
        for device in runtime:
            if (
                device.get("name") == mobile.SIMULATOR
                and device.get("state") == "Booted"
            ):
                data = mobile.run(
                    "xcrun",
                    "simctl",
                    "get_app_container",
                    str(device["udid"]),
                    mobile.BUNDLE_ID,
                    "data",
                )
                if data.returncode == 0:
                    return Path(data.stdout.strip())
    return None


def android_reports(root: Path) -> tuple[int, int] | None:
    if not android.running():
        return None
    found = android.adb(
        "shell", "run-as", android.PACKAGE, "find", ".", "-type", "d", "-name", "bugs"
    )
    if found.returncode != 0:
        return None
    folders = [
        line.strip()
        for line in found.stdout.splitlines()
        if line.strip().endswith("/bugs")
    ]
    if not folders:
        return (0, 0)
    command = [
        android.tool("platform-tools", "adb"),
        "-s",
        android.SERIAL,
        "exec-out",
        "run-as",
        android.PACKAGE,
        "tar",
        "c",
        "-C",
        folders[0],
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


def device_reports(root: Path) -> tuple[int, int] | None:
    """Copies from a paired iPhone only when the caller explicitly asked for it."""
    device = mobile.paired_device()
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
                mobile.BUNDLE_ID,
                "--source",
                str(BUGS_RELATIVE),
                "--destination",
                str(destination),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        if copied.returncode != 0:
            raise RuntimeError(copied.stderr.strip())
        return copy_reports(destination, root)


def run(args: argparse.Namespace) -> int:
    if args.bundle is not None:
        return bugs.import_cmd(argparse.Namespace(bundle=args.bundle))
    root = bugs.bugs_dir()
    try:
        source = simulator_container()
        if source is None:
            print("iOS Simulator: no booted Beryl simulator or installed app")
        else:
            print_counts("iOS Simulator", copy_reports(source / BUGS_RELATIVE, root))
        android_count = android_reports(root)
        if android_count is None:
            print("Android emulator: not running or Beryl is not installed")
        else:
            print_counts("Android emulator", android_count)
        if args.device:
            device_count = device_reports(root)
            if device_count is None:
                print("iPhone: no paired device")
            else:
                print_counts("iPhone", device_count)
    except (OSError, ValueError, RuntimeError, tarfile.TarError) as error:
        print(f"mobile-bugs: {error}", file=sys.stderr)
        return 1
    return 0


def print_counts(source: str, counts: tuple[int, int]) -> None:
    print(f"{source}: {counts[0]} brought, {counts[1]} already saved")


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    parser = sub.add_parser(
        "mobile-bugs", help="pull phone reports or import a shared ZIP"
    )
    parser.add_argument("bundle", nargs="?", type=Path, help="shared report ZIP")
    parser.add_argument(
        "--device", action="store_true", help="also pull from a paired iPhone"
    )
    parser.set_defaults(func=run)
