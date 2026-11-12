"""`by prune-caches`: removes the cache folders of checkouts that are gone. The screenshots,
recorded fixtures and spec output go to `$BERYL_CACHE/<kind>/<checkout name>`, and every
slow-CI tree, Enact worktree and coverage tree is a checkout of its own, so each leaves its
folders behind when it is removed: 70 GB of screenshots alone on 2026-11-10. The nightly
(nightly.py) runs this first, so its own work has the room.

A folder goes when no worktree of the repository has its name and nothing in it changed for
[`MIN_AGE`]: a tree that is being made, or one git no longer lists while its run finishes,
keeps its output.

Low-disk mode: with less than [`LOW_DISK`] free on the cache's volume, the age falls to
[`LOW_DISK_AGE`], and the shared target loses what a clone can do without ([`spare`]). Incident
by-64fx: Arq's hourly snapshot keeps every deleted file that predates it for hours, while each
worktree writes about 15 GB of rust/target, so the disk filled between nightlies. warm.py runs
this before a new checkout's first clone, so it frees space without waiting for the nightly.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from datetime import timedelta
from pathlib import Path
from typing import IO

from . import paths, warm

# The cache folders that hold one folder per checkout, named for it (`paths.REPO.name`):
# shots.py's, specs.py's and motion.py's output.
PER_CHECKOUT: tuple[str, ...] = (
    "ui-shots",
    "ui-test",
    "ui-views",
    "ui-live",
    "spec-shots",
    "spec-film",
    "spec-motion",
    "spec-drive",
    "motion",
)
MIN_AGE = timedelta(days=1)
# Low-disk mode's threshold and age. An hour still spares the output of a run whose tree git
# stopped listing while it finishes.
LOW_DISK = 60_000_000_000
LOW_DISK_AGE = timedelta(hours=1)


@dataclass(frozen=True)
class Removed:
    path: Path
    size: int


def size(path: Path) -> int:
    """The bytes `path` holds on disk: its blocks, or on Windows, which counts none, its
    files' sizes."""
    total = 0
    for root, folders, files in os.walk(path):
        for name in folders + files:
            try:
                stat = os.lstat(os.path.join(root, name))
            except OSError:
                continue  # Removed while we looked.
            blocks = getattr(stat, "st_blocks", None)
            total += stat.st_size if blocks is None else blocks * 512
    return total


def modified(path: Path) -> float:
    """When `path` or an entry directly in it last changed. A run adds or replaces entries at
    the top of its folder, so this is enough without walking the whole tree."""
    times = [path.lstat().st_mtime]
    with os.scandir(path) as entries:
        for entry in entries:
            try:
                times.append(entry.stat(follow_symlinks=False).st_mtime)
            except OSError:
                continue
    return max(times)


def orphans(cache: Path, live: set[str], now: float, age: timedelta) -> list[Path]:
    """The per-checkout folders under `cache` whose checkout is not in `live` and that have
    not changed for `age`, as of `now` (seconds since the epoch)."""
    found: list[Path] = []
    for kind in PER_CHECKOUT:
        root = cache / kind
        if not root.is_dir():
            continue
        for folder in sorted(root.iterdir()):
            if folder.name in live or folder.is_symlink() or not folder.is_dir():
                continue
            try:
                changed = modified(folder)
            except FileNotFoundError:
                continue  # Removed while we looked.
            if now - changed >= age.total_seconds():
                found.append(folder)
    return found


def prune(
    cache: Path,
    live: set[str],
    now: float,
    age: timedelta = MIN_AGE,
    dry_run: bool = False,
) -> list[Removed]:
    """Removes [`orphans`] (unless `dry_run`) and returns them with their sizes. Refuses an
    empty `live`: git failed to list the worktrees, and every folder would look orphaned.
    """
    if not live:
        raise ValueError("no live checkouts: git could not list the worktrees")
    removed: list[Removed] = []
    for folder in orphans(cache, live, now, age):
        removed.append(Removed(folder, size(folder)))
        if not dry_run:
            shutil.rmtree(folder, ignore_errors=True)
    return removed


def spare(target: Path) -> list[Path]:
    """What the shared `target` can lose in low-disk mode and still seed a worktree: the debug
    profile's incremental sessions, which a clone rebuilds its workspace crates without, and
    the other targets' directories (the Tauri CLI's, the phones'), which only the app and the
    phone builds use."""
    found = [
        target / "debug" / "incremental",
        target / warm.TAURI,
        *sorted(target.glob("aarch64-*")),
    ]
    return [path for path in found if path.is_dir() and not path.is_symlink()]


def prune_target(target: Path, dry_run: bool = False) -> list[Removed] | None:
    """Removes [`spare`] (unless `dry_run`) and returns it with its sizes; None, touching
    nothing, while a clone or a save holds the warm lock (warm.py), for a clone could copy a
    half-removed folder."""
    with warm.try_locked() as held:
        if not held:
            return None
        removed: list[Removed] = []
        for path in spare(target):
            removed.append(Removed(path, size(path)))
            if not dry_run:
                shutil.rmtree(path, ignore_errors=True)
        return removed


def free_space(path: Path) -> int:
    """The bytes free on the volume that holds `path`."""
    return shutil.disk_usage(path).free


def gigabytes(count: int) -> str:
    return f"{count / 1e9:.1f} GB"


def report(removed: list[Removed], out: IO[str], dry_run: bool, what: str) -> None:
    verb = "would remove" if dry_run else "removed"
    for r in removed:
        out.write(f"prune-caches: {verb} {r.path} ({gigabytes(r.size)})\n")
    total = sum(r.size for r in removed)
    out.write(f"prune-caches: {verb} {len(removed)} {what}, {gigabytes(total)}\n")


def run(
    out: IO[str],
    dry_run: bool = False,
    threshold: int = LOW_DISK,
    only_when_low: bool = False,
) -> int:
    """Prunes the cache, in low-disk mode when its volume has less than `threshold` free;
    with `only_when_low`, does nothing otherwise (warm.py)."""
    cache = paths.cache_root()
    free = free_space(cache)
    low = free < threshold
    if only_when_low and not low:
        return 0
    if low:
        out.write(f"prune-caches: {gigabytes(free)} free: low-disk mode\n")
    live = set(paths.worktree_names())
    if not live:
        out.write("prune-caches: git could not list the worktrees; nothing removed\n")
        return 1
    age = LOW_DISK_AGE if low else MIN_AGE
    removed = prune(cache, live, time.time(), age, dry_run=dry_run)
    report(removed, out, dry_run, "folders of checkouts that are gone")
    if low:
        target = prune_target(cache / "target", dry_run=dry_run)
        if target is None:
            out.write("prune-caches: a clone holds the shared target; left it\n")
        else:
            report(target, out, dry_run, "folders of the shared target")
    return 0


def prune_step(commit: str, out: IO[str]) -> int:
    """The nightly step (nightly.py), in a process of its own as the others are, so a failure
    here fails the step and not the night. The folders do not depend on `commit`."""
    return subprocess.run(
        [sys.executable, "-m", "beryl_tools.cli", "prune-caches"],
        stdout=out,
        stderr=subprocess.STDOUT,
        stdin=subprocess.DEVNULL,
        check=False,
    ).returncode


def prune_command(args: argparse.Namespace) -> int:
    return run(sys.stdout, dry_run=args.dry_run, threshold=int(args.low_disk * 1e9))


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "prune-caches",
        help="remove the cache folders of checkouts that are gone, unchanged for a day "
        "(an hour, with the shared target's spare folders, when the disk is low)",
    )
    p.add_argument(
        "--dry-run", action="store_true", help="list what would go, removing nothing"
    )
    p.add_argument(
        "--low-disk",
        type=float,
        default=LOW_DISK / 1e9,
        metavar="GB",
        help="prune harder below this much free space (default %(default).0f)",
    )
    p.set_defaults(func=prune_command)
