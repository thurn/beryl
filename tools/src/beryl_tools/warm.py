"""Warm build caches across worktrees, and Enact's worktree remove hook.

A fresh worktree has no `rust/target` (a cold Tauri build) and no `ui/node_modules`. Each one
gets its own, seeded from the per-machine cache:

- `ensure` APFS-clones (copy-on-write, seconds) the cached `rust/target` into a checkout that
  has none, else the main checkout's; cargo rebuilds only what differs. It clones the
  `ui/node_modules` installed from the same package-lock.json, else runs `npm ci`.
- `save` moves both into the cache by rename, which is instant.

Enact runs `python3 warm.py remove` (`.enact.toml`) to save them before it deletes a worktree.
Standard library only: the hook may run from a copy of this file.
"""

from __future__ import annotations

import argparse
import fcntl
import hashlib
import os
import shutil
import subprocess
import sys
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

# tools/src/beryl_tools/warm.py: the checkout is four up.
REPO: Path = Path(__file__).resolve().parents[3]
STAMP = ".beryl-lock"
KEEP_NODE_MODULES = 3


def cache_root() -> Path:
    root = os.environ.get("BERYL_CACHE")
    path = Path(root) if root else Path.home() / "Library" / "Caches" / "beryl"
    path.mkdir(parents=True, exist_ok=True)
    return path


def log(message: str) -> None:
    print(f"warm: {message}", file=sys.stderr)


@contextmanager
def locked() -> Iterator[None]:
    with open(cache_root() / "warm.lock", "w") as handle:
        fcntl.flock(handle, fcntl.LOCK_EX)
        try:
            yield
        finally:
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


def clone_into(source: Path, dest: Path) -> bool:
    """APFS-clones `source` to the missing `dest` through a temporary name."""
    tmp = dest.with_name(f".{dest.name}.tmp-{os.getpid()}")
    shutil.rmtree(tmp, ignore_errors=True)
    dest.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    run = subprocess.run(
        ["cp", "-c", "-R", str(source), str(tmp)], capture_output=True, check=False
    )
    if run.returncode != 0:
        shutil.rmtree(tmp, ignore_errors=True)
        return False
    tmp.rename(dest)
    log(f"cloned {source} in {time.monotonic() - started:.1f} s")
    return True


def ensure_target(checkout: Path) -> None:
    target = checkout / "rust" / "target"
    if target.exists():
        return
    main = main_checkout(checkout)
    for source in (
        cache_root() / "target",
        main / "rust" / "target" if main else None,
    ):
        if source and source.is_dir() and clone_into(source, target):
            return


def ensure_node_modules(checkout: Path) -> None:
    node_modules = checkout / "ui" / "node_modules"
    key = lock_key(checkout)
    if stamped(node_modules, key):
        return
    if not node_modules.exists():
        main = main_checkout(checkout)
        for source in (
            cache_root() / "node_modules" / key,
            main / "ui" / "node_modules" if main else None,
        ):
            if source and stamped(source, key) and clone_into(source, node_modules):
                return
    log("npm ci")
    subprocess.run(
        ["npm", "ci", "--no-audit", "--no-fund", "--loglevel=error"],
        cwd=checkout / "ui",
        stdin=subprocess.DEVNULL,
        check=True,
    )
    (node_modules / STAMP).write_text(key + "\n")


def ensure(checkout: Path = REPO) -> None:
    """Gives the checkout a `rust/target` and an up-to-date `ui/node_modules`."""
    with locked():
        ensure_target(checkout)
        ensure_node_modules(checkout)


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


def save(checkout: Path) -> None:
    """Moves the checkout's caches into the per-machine cache."""
    with locked():
        target = checkout / "rust" / "target"
        if target.is_dir() and replace(target, cache_root() / "target"):
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
        save(path)

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
