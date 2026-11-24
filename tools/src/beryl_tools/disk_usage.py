"""Inventory Beryl's files and measure their physical storage on macOS APFS."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import tomllib

from . import bench, paths, system, video
from .client import vm

SCANNER: Path = Path(__file__).with_name("disk_usage_apfs.cc")
WORKTREES = "Ignite-managed worktrees"
MAIN = "Main checkout"
CHECKOUTS = "Other checkouts"
CACHE = "Beryl caches"
IGNITE = "Ignite state, backups, and logs"
ARENA = "Arena reference material"
BENCH = "Lima benchmark VMs"
MOBILE = "Mobile simulators and Xcode builds"
APP = "App data and exports"
SCRATCH = "Named temporary files"


@dataclass(frozen=True)
class Root:
    area: str
    name: str
    path: Path


def configured_path(variable: str, default: Path) -> Path:
    return Path(os.environ.get(variable) or default).expanduser().resolve()


def children(path: Path) -> list[Path]:
    try:
        return sorted(path.iterdir())
    except (FileNotFoundError, NotADirectoryError):
        return []


def files_under(area: str, path: Path) -> list[Root]:
    return [Root(area, item.name, item) for item in children(path)]


def registered_worktrees(checkout: Path) -> list[Path]:
    result = subprocess.check_output(
        ["git", "-C", str(checkout), "worktree", "list", "--porcelain", "-z"]
    )
    return [
        Path(os.fsdecode(field.removeprefix(b"worktree ")))
        for field in result.split(b"\0")
        if field.startswith(b"worktree ")
    ]


def checkout_roots(
    checkout: Path, managed: Path, registered: list[Path], home: Path
) -> list[Root]:
    # Git lists the main checkout itself. It is already represented by its children.
    roots = [
        root
        for root in files_under(MAIN, checkout)
        if root.path != managed and root.path not in managed.parents
    ]
    if managed.is_relative_to(checkout):
        # When the tree directory is nested, include its siblings along the way.
        for directory in managed.parents:
            if directory == checkout:
                break
            roots.extend(
                root
                for root in files_under(MAIN, directory)
                if root.path != managed and root.path not in managed.parents
            )
    roots.extend(files_under(WORKTREES, managed))
    roots.extend(
        Root(CHECKOUTS, tree.name, tree)
        for tree in registered
        if tree.resolve() != checkout and not tree.is_relative_to(managed)
    )
    for name in ("beryl-fixes", "beryl-hardening", "beryl-mobile-prototype"):
        roots.append(Root(CHECKOUTS, name, home / name))
    return roots


def cache_roots(cache: Path) -> list[Root]:
    roots = [root for root in files_under(CACHE, cache) if root.name != "b3d"]
    roots.extend(
        Root(CACHE, f"b3d/{path.name}", path) for path in children(cache / "b3d")
    )
    return roots


def downloads_roots(home: Path, arena: Path) -> list[Root]:
    try:
        downloads = children(home / "Downloads")
    except PermissionError:
        print("disk-usage: macOS denied access to ~/Downloads.", file=sys.stderr)
        return []
    roots = [
        Root(ARENA, "Indexed videos in Downloads", source)
        for source in downloads
        if source.suffix.lower() in video.RECORDINGS
        and (arena / video.video_id(source)).is_dir()
    ]
    roots.extend(
        Root(APP, path.name, path)
        for path in downloads
        if path.name.startswith("beryl")
    )
    return roots


def ignite_roots(home: Path, ignite: Path) -> list[Root]:
    roots = [root for root in files_under(IGNITE, ignite) if root.name != "hub"]
    roots.extend(
        Root(
            IGNITE, "hub/" + (path.name if path.is_dir() else "database and WAL"), path
        )
        for path in children(ignite / "hub")
    )
    roots.extend(Root(IGNITE, path.name, path) for path in home.glob("ignite-backup-*"))
    roots.extend(
        Root(IGNITE, "Logs", path)
        for path in (home / "Library/Logs").glob("ignite.*.log*")
    )
    roots.append(Root(IGNITE, "~/ignite", home / "ignite"))
    return roots


def benchmark_roots(home: Path) -> list[Root]:
    lima = configured_path("LIMA_HOME", home / ".lima")
    machines = [lima / name for name in (bench.VM, vm.NAME)]
    roots = [Root(BENCH, path.name, path) for path in machines]
    configs = "\n".join(
        path.read_text()
        for machine in machines
        if (path := machine / "lima.yaml").is_file()
    )
    base_images = home / "Library/Caches/lima/download/by-url-sha256"
    roots.extend(
        Root(BENCH, "Lima base image cache", url.parent)
        for url in base_images.glob("*/url")
        if (source := url.read_text().strip()) and source in configs
    )
    return roots


def mobile_roots(home: Path) -> list[Root]:
    library = home / "Library/Developer"
    avds = configured_path("ANDROID_AVD_HOME", home / ".android/avd")
    roots = [
        Root(MOBILE, avd.stem, avd)
        for avd in avds.glob("beryl*")
        if avd.suffix in (".ini", ".avd")
    ]
    roots.extend(
        Root(MOBILE, "Xcode DerivedData", path)
        for path in children(library / "Xcode/DerivedData")
        if path.name.casefold().startswith(("beryl-client-", "beryl3d-", "beryl_app-"))
    )
    for device in (library / "CoreSimulator/Devices").glob("*/device.plist"):
        try:
            with device.open("rb") as stream:
                name = str(plistlib.load(stream).get("name", ""))
        except (FileNotFoundError, plistlib.InvalidFileException):
            continue
        if name.casefold().startswith("beryl"):
            roots.append(Root(MOBILE, name, device.parent))
    return roots


def app_roots(home: Path) -> list[Root]:
    library = home / "Library"
    locations = {
        "Application Support": ("ca.thurn.beryl", "beryl"),
        "Caches": ("beryl-before", "ca.thurn.beryl"),
        "Logs": ("ca.thurn.beryl",),
        "WebKit": ("beryl", "beryl-before", "ca.thurn.beryl"),
        "Preferences": ("ca.thurn.beryl.plist", "beryl3d.plist"),
    }
    roots = [
        Root(APP, f"{area}/{name}", library / area / name)
        for area, names in locations.items()
        for name in names
    ]
    for reports in ("Logs/DiagnosticReports", "Application Support/CrashReporter"):
        roots.extend(
            Root(APP, "Diagnostic reports", path)
            for path in children(library / reports)
            if path.name.casefold().startswith("beryl")
        )
    return roots


def scratch_roots() -> list[Root]:
    directories = {Path("/private/tmp"), Path(tempfile.gettempdir()).resolve()}
    roots: list[Root] = []
    for directory in sorted(directories):
        for entry in children(directory):
            if not entry.name.casefold().startswith(
                ("beryl", "b3d", "ignite", "arena-")
            ):
                continue
            if entry.name.startswith("beryl-disk-usage-"):
                continue
            try:
                if entry.lstat().st_uid == os.getuid():
                    roots.append(Root(SCRATCH, entry.name, entry))
            except FileNotFoundError:
                continue
    return roots


def disjoint(roots: list[Root]) -> list[Root]:
    """Keep the parent when Git and a known location describe the same files."""
    seen: set[Path] = set()
    result: list[Root] = []
    existing = [
        Root(root.area, root.name, root.path.resolve())
        for root in roots
        if not root.path.is_symlink() and root.path.exists()
    ]
    for root in sorted(existing, key=lambda root: len(root.path.parts)):
        if root.path in seen or seen.intersection(root.path.parents):
            continue
        seen.add(root.path)
        result.append(root)
    return result


def inventory(checkout: Path, include_downloads: bool) -> list[Root]:
    home = Path.home()
    config = tomllib.loads((checkout / "ignite.toml").read_text())
    managed = (checkout / config["repository"]["trees"]).resolve()
    if checkout.is_relative_to(managed):
        raise ValueError("Ignite's trees directory must not contain the main checkout")
    arena = video.root().resolve()
    ignite = configured_path("IGNITE_HOME", home / ".config/ignite")
    cache = configured_path("BERYL_CACHE", paths.default_cache())
    return disjoint(
        [
            *checkout_roots(checkout, managed, registered_worktrees(checkout), home),
            *cache_roots(cache),
            Root(ARENA, "Indexes and screenshots", arena),
            Root(ARENA, "Videos", home / "Movies/arena"),
            *(downloads_roots(home, arena) if include_downloads else []),
            *ignite_roots(home, ignite),
            *benchmark_roots(home),
            *mobile_roots(home),
            *app_roots(home),
            *scratch_roots(),
        ]
    )


@dataclass(frozen=True)
class Usage:
    physical: int
    exclusive: int
    allocated: int
    unmapped: int
    unreadable: int
    vanished: int


@dataclass(frozen=True)
class Report:
    total: Usage
    areas: dict[str, Usage]
    details: dict[tuple[str, str], Usage]


def measure(roots: list[Root]) -> Report:
    areas = list(dict.fromkeys(root.area for root in roots))
    details = list(dict.fromkeys((root.area, root.name) for root in roots))
    area_index = {area: index for index, area in enumerate(areas)}
    detail_index = {detail: index for index, detail in enumerate(details)}
    manifest = b"".join(
        f"{area_index[root.area]}\0{detail_index[root.area, root.name]}\0".encode()
        + os.fsencode(root.path)
        + b"\0"
        for root in roots
    )
    cores = min(4, int(os.environ.get("IGNITE_CORES") or os.cpu_count() or 1))
    with tempfile.TemporaryDirectory(prefix="beryl-disk-usage-") as scratch:
        binary = str(Path(scratch) / "scan")
        subprocess.run(
            ["clang++", "-std=c++17", "-O2", "-Wall", str(SCANNER), "-o", binary],
            check=True,
        )
        command = [binary, str(max(1, cores))]
        if not os.environ.get("IGNITE_CORES"):
            command = system.utility(command)
        scan = subprocess.run(
            command, input=manifest, stdout=subprocess.PIPE, check=True
        )

    records: dict[tuple[str, int], Usage] = {}
    for line in scan.stdout.decode("ascii").splitlines():
        kind, index, physical, exclusive, allocated, unmapped, unreadable, vanished = (
            line.split()
        )
        records[kind, int(index)] = Usage(
            int(physical),
            int(exclusive),
            int(allocated),
            int(unmapped),
            int(unreadable),
            int(vanished),
        )
    return Report(
        records["total", 0],
        {name: records["area", index] for index, name in enumerate(areas)},
        {name: records["detail", index] for index, name in enumerate(details)},
    )


def print_report(report: Report, show_details: bool) -> None:
    total = report.total
    print(f"Physical APFS storage: {total.physical / 1e9:,.2f} GB")
    print(f"Summed file allocations before sharing: {total.allocated / 1e9:,.2f} GB")
    duplicate = max(0, total.allocated - total.physical - total.unmapped)
    print(f"Duplicate allocations found: {duplicate / 1e9:,.2f} GB")
    print()
    print(f"{'Area':<41} {'Physical GB':>12} {'Exclusive GB':>13}")
    for area, usage in sorted(
        report.areas.items(), key=lambda item: item[1].exclusive, reverse=True
    ):
        print(
            f"{area:<41} {usage.physical / 1e9:12,.2f} {usage.exclusive / 1e9:13,.2f}"
        )
        if show_details:
            for (owner, name), detail in sorted(
                report.details.items(), key=lambda item: item[1].physical, reverse=True
            ):
                if owner == area and detail.physical:
                    print(
                        f"  {name:<39} {detail.physical / 1e9:12,.2f}"
                        f" {detail.exclusive / 1e9:13,.2f}"
                    )
    shared = total.physical - sum(area.exclusive for area in report.areas.values())
    print(f"{'Shared across areas':<41} {'':>12} {shared / 1e9:13,.2f}")
    print()
    print(
        "Exclusive bytes are referenced only within that row among the scanned paths."
    )
    if total.unmapped or total.unreadable or total.vanished:
        print(
            f"Live scan gaps: {total.unmapped / 1e6:,.2f} MB of unmapped allocation, "
            f"{total.unreadable:,} unreadable entries, {total.vanished:,} vanished entries."
        )


def run(args: argparse.Namespace) -> int:
    if sys.platform != "darwin":
        print("disk-usage requires macOS and APFS.", file=sys.stderr)
        return 2
    try:
        checkout = paths.main_checkout()
        if checkout is None:
            print("disk-usage: could not locate the main checkout.", file=sys.stderr)
            return 1
        roots = inventory(checkout.resolve(), include_downloads=not args.no_downloads)
        print(
            f"Scanning {len(roots)} Beryl paths for physical APFS blocks…",
            file=sys.stderr,
            flush=True,
        )
        report = measure(roots)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"disk-usage: {error}", file=sys.stderr)
        return 1
    print_report(report, args.details)
    if args.no_downloads:
        print("Downloads discovery skipped.")
    return int(report.total.unreadable != 0)


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    parser = sub.add_parser(
        "disk-usage", help="measure Beryl's physical disk footprint on macOS APFS"
    )
    parser.add_argument(
        "--details",
        action="store_true",
        help="show individual worktrees and cache paths",
    )
    parser.add_argument(
        "--no-downloads",
        action="store_true",
        help="skip looking for Arena videos and Beryl exports in ~/Downloads",
    )
    parser.set_defaults(func=run)
