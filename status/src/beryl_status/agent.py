"""`just status install`: a launchd agent that runs `just status publish` when the beads
change, at background priority, so the GitHub Pages site stays current while this Mac is
awake.

launchd starts the job when one of the beads database's files changes (`WatchPaths`), and
never sooner than `THROTTLE_SECONDS` after its last start, so a busy hour of bead changes
publishes once every ten minutes: GitHub Pages builds a branch at most about ten times an
hour. An hourly start (`StartInterval`) covers a change launchd did not report and keeps the
changelog fill going while nothing changes. The pattern is the nightly agent's
(tools/src/beryl_tools/nightly.py), copied because nothing here imports the tools."""

from __future__ import annotations

import os
import plistlib
import shutil
import subprocess
import sys
from pathlib import Path

from . import paths

LABEL = "ca.thurn.beryl.status"
# GitHub Pages allows about ten builds an hour from a branch.
THROTTLE_SECONDS = 600
FALLBACK_SECONDS = 3600


def plist_path() -> Path:
    return Path.home() / "Library" / "LaunchAgents" / f"{LABEL}.plist"


def log_path() -> Path:
    return paths.folder() / "launchd.log"


def environment() -> dict[str, str]:
    """What launchd's bare environment lacks: a PATH holding uv, git, bd and gh, and the
    cache if it is not the default."""
    directories: list[str] = []
    for tool in ("uv", "git", "bd", "gh"):
        found = shutil.which(tool)
        if found and str(Path(found).parent) not in directories:
            directories.append(str(Path(found).parent))
    directories += ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
    env = {"PATH": ":".join(directories), "HOME": str(Path.home())}
    if os.environ.get("BERYL_CACHE"):
        env["BERYL_CACHE"] = os.environ["BERYL_CACHE"]
    return env


def agent_plist(main: Path, uv: str) -> dict[str, object]:
    log = log_path().as_posix()
    return {
        "Label": LABEL,
        "ProgramArguments": [
            uv,
            "run",
            "--quiet",
            "--no-dev",
            "--project",
            (main / "status").as_posix(),
            "beryl-status",
            "publish",
        ],
        "WorkingDirectory": main.as_posix(),
        "WatchPaths": [p.as_posix() for p in paths.store_files(main)],
        "ThrottleInterval": THROTTLE_SECONDS,
        "StartInterval": FALLBACK_SECONDS,
        "ProcessType": "Background",
        "LowPriorityIO": True,
        "EnvironmentVariables": environment(),
        "StandardOutPath": log,
        "StandardErrorPath": log,
    }


def launchctl(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["launchctl", *args], capture_output=True, text=True)


def domain() -> str:
    return f"gui/{os.getuid()}"


def install() -> int:
    if sys.platform != "darwin":
        print("status: the launchd agent is supported on macOS only; nothing to do")
        return 0
    main = paths.main_checkout()
    uv = shutil.which("uv")
    if uv is None:
        print("status install: needs uv on the PATH", file=sys.stderr)
        return 1
    path = plist_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(plistlib.dumps(agent_plist(main, uv)))
    launchctl("bootout", f"{domain()}/{LABEL}")
    loaded = launchctl("bootstrap", domain(), str(path))
    if loaded.returncode:
        print(f"status install: launchctl: {loaded.stderr.strip()}", file=sys.stderr)
        return 1
    print(
        f"status install: {path} loaded: `just status publish` from {main} when the "
        f"beads change, at most every {THROTTLE_SECONDS // 60} minutes; its log is "
        f"{log_path()}; "
        "`just status uninstall` removes it"
    )
    return 0


def uninstall() -> int:
    if sys.platform != "darwin":
        return 0
    launchctl("bootout", f"{domain()}/{LABEL}")
    plist_path().unlink(missing_ok=True)
    print(f"status uninstall: {LABEL} unloaded, {plist_path()} removed")
    return 0
