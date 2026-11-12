"""Warm build caches across worktrees, and Enact's worktree remove hook.

A fresh worktree has no `rust/target` (a cold Tauri build) and no `ui/node_modules`. Each one
gets its own, seeded from the per-machine cache:

- `ensure` APFS-clones (copy-on-write, seconds) the cached `rust/target` into a checkout that
  has none, else the main checkout's (on Windows, copies it with robocopy); cargo rebuilds only what differs. It leaves out
  `target/tauri`, the Tauri CLI's own target directory, unless asked for it (`app.py`). It clones
  the `ui/node_modules` installed from the same package-lock.json, else runs `npm ci`.
- `save` prunes what builds no longer use from `rust/target`, then moves both into the cache by
  rename, which is instant.

A clone takes time in proportion to the number of files, so the cache must not grow with every
save: see `prune`.

Enact runs `python3 warm.py remove` (`.enact.toml`) to save them before it deletes a worktree.
Standard library only: the hook may run from a copy of this file.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import time
from collections.abc import Iterator
from contextlib import contextmanager, suppress
from dataclasses import dataclass, field
from pathlib import Path

# tools/src/beryl_tools/warm.py: the checkout is four up.
REPO: Path = Path(__file__).resolve().parents[3]
STAMP = ".beryl-lock"
KEEP_NODE_MODULES = 3
# The Tauri CLI's target directory inside rust/target (`paths.TAURI_TARGET`).
TAURI = "tauri"
# What a target's crate builds may hold (`BERYL_TARGET_BUDGET_GB` overrides it), and how long
# one may go unused. Each worktree builds the workspace crates under new hashes, and nothing
# removed the old ones: on 2026-11-11 the cache held 45 GB, of which the last build used 5.5,
# and a copy of it in each Windows worktree filled the disk.
BUDGET_GB = 15.0
UNUSED_DAYS = 7
DAY = 86400.0
# A unit used this recently belongs to the build that just ran: the budget never evicts it.
RECENT = 3600.0
# `<crate>-<hash>` and what cargo names after it: `lib<crate>-<hash>.rlib`, `.d`, `.exe`,
# an incremental unit's `<crate>-<hash>.<cgu>.<build>.rcgu.o`, a build/ or .fingerprint/ folder.
UNIT_NAME: re.Pattern[str] = re.compile(r"^(?:lib)?(.+-[0-9a-f]{16})(?:\..+)?$")


WINDOWS: bool = sys.platform == "win32"


def cache_root() -> Path:
    root = os.environ.get("BERYL_CACHE")
    local = os.environ.get("LOCALAPPDATA")
    if root:
        path = Path(root)
    elif WINDOWS and local:
        path = Path(local) / "beryl"
    else:
        path = Path.home() / "Library" / "Caches" / "beryl"
    path.mkdir(parents=True, exist_ok=True)
    return path


def log(message: str) -> None:
    print(f"warm: {message}", file=sys.stderr)


@contextmanager
def locked() -> Iterator[None]:
    with open(cache_root() / "warm.lock", "w") as handle:
        if WINDOWS:
            import msvcrt

            # LK_LOCK gives up after ten tries a second apart: a clone can take longer.
            while True:
                try:
                    # pyre-ignore[16]: Windows only
                    msvcrt.locking(handle.fileno(), msvcrt.LK_LOCK, 1)
                    break
                except OSError:
                    continue
            try:
                yield
            finally:
                handle.seek(0)
                # pyre-ignore[16]: Windows only
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
            return
        import fcntl

        fcntl.flock(handle, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(handle, fcntl.LOCK_UN)


@contextmanager
def try_locked() -> Iterator[bool]:
    """Holds the lock [`locked`] takes if nobody holds it, yielding whether it got it: for work
    that must not run beside a clone or a save but need not wait for one (`caches.py`).
    """
    with open(cache_root() / "warm.lock", "w") as handle:
        try:
            if WINDOWS:
                import msvcrt

                # pyre-ignore[16]: Windows only
                msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl

                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            yield False
            return
        try:
            yield True
        finally:
            if WINDOWS:
                handle.seek(0)
                # pyre-ignore[16]: Windows only
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(handle, fcntl.LOCK_UN)


def lock_key(checkout: Path) -> str:
    lock = checkout / "ui" / "package-lock.json"
    data = lock.read_bytes() if lock.exists() else b"<missing>"
    return hashlib.sha256(data).hexdigest()[:16]


def stamped(node_modules: Path, key: str) -> bool:
    stamp = node_modules / STAMP
    return stamp.is_file() and stamp.read_text().strip() == key


def main_checkout(checkout: Path) -> Path | None:
    run = subprocess.run(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        cwd=checkout,
        capture_output=True,
        text=True,
        check=False,
    )
    main = Path(run.stdout.strip()).parent if run.returncode == 0 else None
    return main if main and main.resolve() != checkout.resolve() else None


def robocopy(source: Path, dest: Path, leave_out: str | None) -> list[str]:
    """Windows has no `cp -c`: robocopy copies on many threads, which makes a copy of
    rust/target take a minute rather than many. Its exit codes under 8 mean success, which a
    wrapper turns into 0."""
    exclude = ["/XD", str(source / leave_out)] if leave_out else []
    script = (
        "import subprocess, sys; sys.exit(subprocess.run(sys.argv[1:]).returncode >= 8)"
    )
    return [
        sys.executable,
        "-c",
        script,
        "robocopy",
        str(source),
        str(dest),
        "/E",
        "/MT:32",
        "/NFL",
        "/NDL",
        "/NJH",
        "/NJS",
        "/NP",
        *exclude,
    ]


def clone_into(source: Path, dest: Path, leave_out: str | None = None) -> bool:
    """APFS-clones `source`, but for its entry `leave_out`, to the missing `dest` through a
    temporary name."""
    tmp = dest.with_name(f".{dest.name}.tmp-{os.getpid()}")
    shutil.rmtree(tmp, ignore_errors=True)
    dest.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    if WINDOWS:
        command = robocopy(source, tmp, leave_out)
    elif leave_out is None:
        command = ["cp", "-c", "-R", str(source), str(tmp)]
    else:
        tmp.mkdir()
        # Into the existing directory: cp copies each entry under its own name.
        entries = [str(e) for e in source.iterdir() if e.name != leave_out]
        command = ["cp", "-c", "-R", *entries, str(tmp)]
    run = subprocess.run(command, capture_output=True, check=False)
    if run.returncode != 0:
        shutil.rmtree(tmp, ignore_errors=True)
        return False
    tmp.rename(dest)
    log(f"cloned {source} in {time.monotonic() - started:.1f} s")
    return True


def ensure_target(checkout: Path, tauri: bool) -> bool:
    """Whether it left cargo to build `rust/target` cold: there was none and nothing to clone
    it from."""
    target = checkout / "rust" / "target"
    if target.exists() and (not tauri or (target / TAURI).exists()):
        return False
    main = main_checkout(checkout)
    sources = [
        source
        for source in (
            cache_root() / "target",
            main / "rust" / "target" if main else None,
        )
        if source and source.is_dir()
    ]
    if not target.exists():
        leave_out = None if tauri else TAURI
        if not any(clone_into(source, target, leave_out) for source in sources):
            return True
    if tauri and not (target / TAURI).exists():
        for source in sources:
            if (source / TAURI).is_dir() and clone_into(source / TAURI, target / TAURI):
                return False
        return True
    return False


def ensure_node_modules(checkout: Path) -> bool:
    """Whether it had to install `ui/node_modules` cold (`npm ci`), with none to clone."""
    node_modules = checkout / "ui" / "node_modules"
    key = lock_key(checkout)
    if stamped(node_modules, key):
        return False
    if not node_modules.exists():
        main = main_checkout(checkout)
        for source in (
            cache_root() / "node_modules" / key,
            main / "ui" / "node_modules" if main else None,
        ):
            if source and stamped(source, key) and clone_into(source, node_modules):
                return False
    log("npm ci")
    subprocess.run(
        [
            (shutil.which("npm") or "npm") if WINDOWS else "npm",
            "ci",
            "--no-audit",
            "--no-fund",
            "--loglevel=error",
        ],
        cwd=checkout / "ui",
        stdin=subprocess.DEVNULL,
        check=True,
    )
    (node_modules / STAMP).write_text(key + "\n")
    return True


def make_room() -> None:
    """Before a new checkout's first clone, frees what `by prune-caches` can when the disk is
    nearly full (`caches.py`, low-disk mode), so a burst of worktrees does not wait for the
    nightly. Best effort, outside the lock, which the prune takes for the shared target only if
    no clone or save holds it. Imported here: the remove hook, which never gets here, runs this
    file alone."""
    from . import caches

    try:
        caches.run(sys.stderr, only_when_low=True)
    except (OSError, ValueError) as error:
        log(f"could not prune the caches: {error}")


def ensure(checkout: Path = REPO, tauri: bool = False) -> bool:
    """Gives the checkout a `rust/target`, with the Tauri CLI's target directory if `tauri`, and
    an up-to-date `ui/node_modules`. Whether either was built cold, with nothing to clone: a run
    over budget then reruns once (checks.py, `slow`)."""
    if not (checkout / "rust" / "target").exists():
        make_room()
    with locked():
        cold_target = ensure_target(checkout, tauri)
        return ensure_node_modules(checkout) or cold_target


def replace(source: Path, dest: Path) -> bool:
    """Moves `source` to `dest` by rename, replacing whatever is there."""
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_name(f"{dest.name}.tmp-{os.getpid()}")
    try:
        source.rename(tmp)
    except OSError:
        return False  # another volume: not worth copying gigabytes
    old = dest.with_name(f"{dest.name}.old-{os.getpid()}")
    if dest.exists():
        dest.rename(old)
    tmp.rename(dest)
    os.utime(dest)
    shutil.rmtree(old, ignore_errors=True)
    return True


def build_token(name: str) -> str | None:
    """The build an incremental unit's loose object belongs to: rustc names them
    `<unit>.<codegen unit>.<build>.rcgu.o`, with a new <build> each time it rebuilds the unit.
    None for other files, such as a non-incremental unit's `<unit>.<crate>.<hash>-cgu.<n>.rcgu.o`,
    which each build overwrites."""
    parts = name.split(".")
    return parts[2] if len(parts) == 5 and name.endswith(".rcgu.o") else None


def prune_objects(deps: Path) -> int:
    """Deletes the loose objects of every incremental unit's earlier builds. On macOS a debug
    build leaves each unit's codegen-unit objects in deps/ for the debugger to read (unpacked
    debug info), and a rebuild writes a new set beside the old ones instead of replacing them.
    """
    builds: dict[str, dict[str, list[Path]]] = {}
    for entry in deps.iterdir():
        if token := build_token(entry.name):
            unit = entry.name.split(".", 1)[0]
            builds.setdefault(unit, {}).setdefault(token, []).append(entry)
    pruned = 0
    for sets in builds.values():
        newest = max(sets, key=lambda t: max(p.stat().st_mtime for p in sets[t]))
        for token, objects in sets.items():
            if token != newest:
                for path in objects:
                    path.unlink(missing_ok=True)
                pruned += len(objects)
    return pruned


def session_time(name: str) -> int:
    """The creation time encoded in an incremental session directory's name, `s-<time>-...`."""
    return int(name.split("-")[1], 36)


def prune_sessions(incremental: Path) -> int:
    """Deletes each crate's incremental sessions but the newest finished one, the only one
    rustc builds on."""
    pruned = 0
    for crate in incremental.iterdir():
        if not crate.is_dir():
            continue
        finished = sorted(
            (
                s
                for s in crate.iterdir()
                if s.is_dir()
                and s.name.startswith("s-")
                and not s.name.endswith("-working")
            ),
            key=lambda s: session_time(s.name),
        )
        for session in finished[:-1]:
            shutil.rmtree(session, ignore_errors=True)
            session.with_name(f"{session.name.rsplit('-', 1)[0]}.lock").unlink(
                missing_ok=True
            )
            pruned += 1
    return pruned


@dataclass
class Unit:
    """Artifacts of one crate build that go and stay together: its files in deps/ and its
    build/ and .fingerprint/ folders, all named `<crate>-<hash>`, or one crate's incremental/
    folder. `used` is when any file in it was last read or written."""

    paths: list[Path] = field(default_factory=list)
    size: int = 0
    used: float = 0.0


def usage(path: Path) -> tuple[int, float]:
    """The bytes under `path` and the latest access or modification time of any file there.
    Access times count because a build reads the dependencies it does not rebuild: Windows
    and macOS keep them, to the hour or so. Where a volume does not, this is the last write.
    """
    stat = path.lstat()
    if not path.is_dir() or path.is_symlink():
        return stat.st_size, max(stat.st_atime, stat.st_mtime)
    # Not the folder's own time: creating it, or a file in it, is no use of the files.
    size, used, files = 0, 0.0, 0
    folders = [path]
    while folders:
        # On Windows scandir's entries carry their times and sizes: no call per file.
        with suppress(OSError), os.scandir(folders.pop()) as entries:
            for entry in entries:
                if entry.is_dir(follow_symlinks=False):
                    folders.append(Path(entry.path))
                    continue
                with suppress(OSError):
                    s = entry.stat(follow_symlinks=False)
                    size += s.st_size
                    used = max(used, s.st_atime, s.st_mtime)
                    files += 1
    return size, used if files else stat.st_mtime


def units(profiles: list[Path]) -> list[Unit]:
    """The [`Unit`]s of each profile's deps/ folder in `profiles`, with their sizes and times.
    Files that match no unit (the profile's own binaries, which each build overwrites) are
    left out."""
    found: dict[tuple[Path, str], Unit] = {}
    for deps in profiles:
        profile = deps.parent
        for kind in ("deps", "build", ".fingerprint"):
            folder = profile / kind
            if folder.is_dir():
                for entry in folder.iterdir():
                    if match := UNIT_NAME.match(entry.name):
                        key = (profile, match.group(1))
                        found.setdefault(key, Unit()).paths.append(entry)
        incremental = profile / "incremental"
        if incremental.is_dir():
            for crate in incremental.iterdir():
                found[(profile, f"incremental/{crate.name}")] = Unit([crate])
    for unit in found.values():
        for path in unit.paths:
            with suppress(OSError):
                size, used = usage(path)
                unit.size += size
                unit.used = max(unit.used, used)
    return list(found.values())


def budget() -> int:
    """The bytes a target's units may hold: `BERYL_TARGET_BUDGET_GB` gigabytes, else
    [`BUDGET_GB`]."""
    with suppress(ValueError):
        return int(float(os.environ.get("BERYL_TARGET_BUDGET_GB", BUDGET_GB)) * 1e9)
    return int(BUDGET_GB * 1e9)


def evict(profiles: list[Path], now: float, limit: int) -> tuple[int, int]:
    """Deletes the units unused for [`UNUSED_DAYS`], then the least recently used until the
    rest fit in `limit` bytes, never one used within [`RECENT`] of `now`: the build that just
    ran needs it. A unit still in use costs a rebuild of that crate, nothing worse. Returns the
    units and bytes deleted."""
    found = sorted(units(profiles), key=lambda u: u.used)
    total = sum(u.size for u in found)
    count = freed = 0
    for unit in found:
        idle = now - unit.used
        if idle < UNUSED_DAYS * DAY and (total <= limit or idle < RECENT):
            break  # Sorted by use: every later unit is as recent.
        for path in unit.paths:
            if path.is_dir() and not path.is_symlink():
                shutil.rmtree(path, ignore_errors=True)
            else:
                path.unlink(missing_ok=True)
        total -= unit.size
        freed += unit.size
        count += 1
    return count, freed


def prune(target: Path) -> None:
    """Deletes what earlier builds left in `target` and no builds use any more, then evicts
    what has gone unused longest until the rest fits the budget, so that the cache, the disk
    each worktree's copy takes and the time to clone it do not grow with every save. Best
    effort: it runs in Enact's remove hook, which must not fail."""
    profiles = [*target.glob("*/deps"), *target.glob(f"{TAURI}/*/deps")]
    try:
        objects = sum(prune_objects(deps) for deps in profiles)
        sessions = sum(
            prune_sessions(deps.with_name("incremental"))
            for deps in profiles
            if deps.with_name("incremental").is_dir()
        )
        limit = budget()
        evicted, freed = evict(profiles, time.time(), limit)
    except (OSError, ValueError, IndexError) as error:
        log(f"could not prune {target}: {error}")
        return
    log(
        f"pruned {objects} stale objects and {sessions} incremental sessions; evicted"
        f" {evicted} unused builds ({freed / 1e9:.1f} GB) to fit {limit / 1e9:.0f} GB"
    )


def save(checkout: Path) -> None:
    """Moves the checkout's caches into the per-machine cache."""
    with locked():
        target = checkout / "rust" / "target"
        cached = cache_root() / "target"
        if target.is_dir():
            if not (target / TAURI).exists() and (cached / TAURI).is_dir():
                # This checkout never ran the Tauri CLI: keep the cache's build of it. A
                # failed rename means another volume, where `replace` fails too.
                with suppress(OSError):
                    (cached / TAURI).rename(target / TAURI)
            prune(target)
            if replace(target, cached):
                log(f"saved {target}")
        node_modules = checkout / "ui" / "node_modules"
        key = lock_key(checkout)
        folder = cache_root() / "node_modules"
        if stamped(node_modules, key) and replace(node_modules, folder / key):
            log(f"saved {node_modules}")
            entries = sorted(
                (p for p in folder.iterdir() if p.is_dir() and "." not in p.name),
                key=lambda p: p.stat().st_mtime,
                reverse=True,
            )
            for stale in entries[KEEP_NODE_MODULES:]:
                shutil.rmtree(stale, ignore_errors=True)


def remove_hook() -> int:
    """Enact's remove hook: save the worktree's caches, then remove it as Enact would (the
    hook replaces its removal)."""
    path = Path(os.environ["ENACT_WORKTREE_PATH"])
    repo: Path = Path(os.environ["ENACT_REPO"])
    branch = os.environ.get("ENACT_BRANCH", "")
    if path.is_dir():
        # A full disk fails the save (even the lock file): remove the worktree all the same,
        # or its copy of rust/target stays behind and keeps the disk full.
        try:
            save(path)
        except OSError as error:
            log(f"could not save {path}'s caches: {error}")

    def git(*args: str) -> str:
        run = subprocess.run(
            ["git", "-C", str(repo), *args], capture_output=True, text=True, check=False
        )
        return run.stdout

    git("worktree", "remove", "--force", str(path))
    # Leftovers a still-running build wrote meanwhile, once git no longer tracks the tree.
    if path.exists() and f"worktree {path.resolve()}\n" not in git(
        "worktree", "list", "--porcelain"
    ):
        shutil.rmtree(path, ignore_errors=True)
    if branch:
        git("branch", "-D", branch)
    git("worktree", "prune")
    return 0


def warm_command(args: argparse.Namespace) -> int:
    if args.action == "save":
        save(REPO)
    else:
        ensure(REPO)
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser("warm", help="restore (or save) rust/target and ui/node_modules")
    p.add_argument("action", nargs="?", choices=["ensure", "save"], default="ensure")
    p.set_defaults(func=warm_command)


if __name__ == "__main__":
    if sys.argv[1:] != ["remove"]:
        print("usage: python3 warm.py remove (Enact's hook)", file=sys.stderr)
        sys.exit(2)
    sys.exit(remove_hook())
