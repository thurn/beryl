"""What a command needs installed in the checkout before it runs: `ui/node_modules`.

`ui/node_modules` is installed with `npm ci` when it is missing or was installed from a
different `ui/package-lock.json`. A stamp file inside it records the lock file's hash. Cargo
builds `rust/target` itself, so nothing here prepares it. Ignite's build trees install the same
folder as the `ui-deps` product (`ignite/checks/ui.toml`), so in a tree this costs one stat.
"""

from __future__ import annotations

import argparse
import hashlib
import shutil
import subprocess
import sys
from pathlib import Path

from .paths import REPO
from .system import WINDOWS

STAMP = ".beryl-lock"

# The exit code with which a step says it could not run on this machine, such as a missing
# simulator or browser, as distinct from a failure. Ignite's adapters report it as incomplete.
CANNOT_RUN = 125


def lock_key(checkout: Path) -> str:
    """A short hash of the checkout's `ui/package-lock.json`."""
    lock = checkout / "ui" / "package-lock.json"
    data = lock.read_bytes() if lock.exists() else b"<missing>"
    return hashlib.sha256(data).hexdigest()[:16]


def ensure_node_modules(checkout: Path = REPO) -> bool:
    """Installs `ui/node_modules` with `npm ci` unless it is current. Whether it installed."""
    node_modules = checkout / "ui" / "node_modules"
    key = lock_key(checkout)
    stamp = node_modules / STAMP
    if stamp.is_file() and stamp.read_text().strip() == key:
        return False
    print("npm ci", file=sys.stderr)
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
    stamp.write_text(key + "\n")
    return True


def run(args: argparse.Namespace) -> int:
    ensure_node_modules()
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "deps", help="Install ui/node_modules with npm ci unless it is current"
    )
    p.set_defaults(func=run)
