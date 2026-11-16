"""Sweeps the crate builds that newer builds replaced out of a build directory, such as a
worktree's `rust/target` or `rust/beryl3d/target` (`caches.tidy_target`).

Enact reuses its worktree slots for days, and each change on staging to a crate's
dependencies, features or profile builds that crate, and every crate above it, under a new
hash beside the old one. Nothing deleted the old ones: on 2026-11-15 the Beryl 3D targets of
the slots held 12-47 GB each, one of them 28 builds of `beryl_scene` from that day. The budget
in warm.py (`prune`) evicts by last access time, but APFS records only the first read after a
file is written, so a unit every build reads looks as old as one replaced hours ago.

So `sweep` marks every unit unread, as a clone does (warm.py, `mark_unread`), and at the next
sweep deletes each build that no build read since while a build of the same unit, with other
dependencies, was read or written: cargo reads a unit's fingerprint file whenever a build
includes it.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from collections.abc import Callable, Hashable
from contextlib import suppress
from pathlib import Path

from . import system, warm

# The file whose modification time is when `sweep` last marked every unit of a target unread,
# and how long after that it next sweeps the units that no build read since.
MARK = ".beryl-marked"
SWEEP_AFTER = 6 * 3600.0
# The folders in a target that a sweep moves what it deletes into, for a process of its own.
TRASH = ".swept-"
# What a fingerprint's JSON records besides the unit's dependencies, its source path and the
# toolchain: two builds that differ only in those are the same unit built at two commits.
UNIT_FIELDS = ("features", "profile", "target", "compile_kind", "rustflags")


def mark(target: Path) -> None:
    """Marks every unit of `target` unread ([`warm.mark_unread`]) and records when, in [`MARK`], so
    that a later [`sweep`] can tell which units no build read since. A clone carries its
    source's mark, which is older than this clone's [`warm.mark_unread`]: writing it afresh keeps
    the first sweep of a new worktree from taking every unit its first build skipped."""
    warm.mark_unread(target)
    with suppress(OSError):
        # touch, not a write: truncating an empty file leaves its modification time.
        (target / MARK).touch()


def sweep_due(target: Path, now: float) -> bool:
    """Whether `target` has no [`MARK`] or one older than [`SWEEP_AFTER`]. One stat."""
    try:
        return now - (target / MARK).stat().st_mtime >= SWEEP_AFTER
    except OSError:
        return True


def crate_of(name: str) -> str:
    """The crate of a unit's or incremental cache's folder name, `<crate>-<hash>`."""
    return name.rsplit("-", 1)[0]


def unit_kind(folder: Path) -> Hashable:
    """What the unit whose fingerprint folder is `folder` builds, apart from its dependencies:
    its crate, its kind (the fingerprint's file name, such as `lib-serde`, `test-lib-serde` or
    `test-integration-test-play`) and the [`UNIT_FIELDS`] of its JSON, which tell a build from
    clippy's check of the same crate and a dev build from a test build. Cargo gives such a
    unit a new hash whenever a crate it depends on changes its version, features or profile.
    """
    kinds: list[str] = []
    fields: list[str] = []
    with suppress(OSError), os.scandir(folder) as files:
        for f in files:
            if f.name.endswith(".json"):
                with suppress(OSError, ValueError):
                    record = json.loads(Path(f.path).read_text())
                    fields = [str(record.get(k)) for k in UNIT_FIELDS]
            elif not f.name.startswith(("dep-", "output-", "invoked.")):
                kinds.append(f.name)
    return (crate_of(folder.name), tuple(sorted(kinds)), tuple(fields))


def read_since(folder: Path, marked: float) -> bool:
    """Whether a build read or wrote any file in the unit folder `folder` since `marked`, the
    time of the last [`mark`]. A mark leaves each file's access time a second before its
    modification time, and APFS moves the access time forward on the next read."""
    with suppress(OSError), os.scandir(folder) as files:
        for f in files:
            with suppress(OSError):
                s = f.stat(follow_symlinks=False)
                if s.st_atime >= s.st_mtime or s.st_mtime >= marked:
                    return True
    return False


def newest_write(folder: Path) -> float:
    """The latest modification time of `folder`'s entries, or 0 for none."""
    newest = 0.0
    with suppress(OSError), os.scandir(folder) as entries:
        for entry in entries:
            with suppress(OSError):
                newest = max(newest, entry.stat(follow_symlinks=False).st_mtime)
    return newest


def superseded(
    folders: list[Path], used: Callable[[Path], bool], kind: Callable[[Path], Hashable]
) -> set[str]:
    """The names of the folders in `folders` that are not `used` while another folder of the
    same `kind` is: builds that a newer build of the same unit replaced. A unit none of whose
    builds is used keeps them all, so a target nobody built in since the mark loses nothing.
    """
    found: dict[Hashable, list[tuple[str, bool]]] = {}
    for folder in folders:
        # `used` first: `kind` may read the folder's files, which counts as a use.
        was_used = used(folder)
        found.setdefault(kind(folder), []).append((folder.name, was_used))
    return {
        name
        for builds in found.values()
        if any(u for _, u in builds)
        for name, u in builds
        if not u
    }


def doomed(target: Path, marked: float) -> list[Path]:
    """What a sweep of `target` deletes: each replaced unit's files in deps/, build/ and
    .fingerprint/ of every profile, and each replaced incremental cache."""
    profiles = {
        folder.parent
        for profile in warm.PROFILES
        for kind in (".fingerprint", "incremental")
        for folder in target.glob(f"{profile}/{kind}")
    }
    found: list[Path] = []
    for profile in sorted(profiles):
        gone = superseded(
            [p for p in (profile / ".fingerprint").glob("*") if p.is_dir()],
            lambda p: read_since(p, marked),
            unit_kind,
        )
        for kind in ("deps", "build", ".fingerprint"):
            for entry in (profile / kind).glob("*"):
                match = warm.UNIT_NAME.match(entry.name)
                if match and match.group(1) in gone:
                    found.append(entry)
        # Incremental caches are named for their crate with another hash than its units', so
        # they are told apart only by crate. Rustc writes a session into one each time it
        # compiles its unit, and an edit to a crate recompiles every live unit of it, so a
        # cache left unwritten while another of its crate was written is most likely a replaced
        # build's. If it was not, the next change to that unit compiles it once without one.
        found += [
            profile / "incremental" / name
            for name in superseded(
                [p for p in (profile / "incremental").glob("*") if p.is_dir()],
                lambda p: newest_write(p) >= marked,
                lambda p: crate_of(p.name),
            )
        ]
    return found


def delete_later(paths: list[Path], trash: Path) -> None:
    """Moves `paths` into the folder `trash`, which takes a rename each, and deletes it at low
    priority in a process of its own: deleting 23,000 files took 80 s at load 30, too long to
    hold the cargo lock and keep the next build waiting."""
    trash.mkdir(exist_ok=True)
    for n, path in enumerate(paths):
        with suppress(OSError):
            path.rename(trash / f"{n}-{path.name}")
    script = "import shutil, sys\nfor p in sys.argv[1:]: shutil.rmtree(p, ignore_errors=True)"
    leftovers = [str(p) for p in trash.parent.glob(f"{TRASH}*")]
    subprocess.Popen(
        system.utility([sys.executable, "-c", script, *leftovers]),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )


def sweep(target: Path) -> int:
    """Deletes the crate builds in `target` that no build read since the last [`mark`] while a
    newer build of the same unit was read or written ([`superseded`]), then marks the target
    again; how many folders and files it deleted. Run it no more often than [`SWEEP_AFTER`]
    ([`sweep_due`]), with the target's cargo lock held.

    The newest build of each unit and every build a command read since the mark stay, so the
    next build compiles nothing more than it would have: after a sweep took a fresh worktree's
    Beryl 3D target from 18 GB to 6.9 GB, its test build compiled nothing."""
    marked = 0.0
    with suppress(OSError):
        marked = (target / MARK).stat().st_mtime
    paths = doomed(target, marked) if marked else []
    if paths:
        delete_later(paths, target / f"{TRASH}{os.getpid()}-{int(time.time())}")
    mark(target)
    warm.log(f"swept {len(paths)} replaced build files and folders from {target}")
    return len(paths)
