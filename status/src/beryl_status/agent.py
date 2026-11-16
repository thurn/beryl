"""`just status install`: a launchd agent that runs `just status publish` every ten minutes, at
background priority, so the GitHub Pages site stays current while this Mac is awake. The
pattern is the nightly agent's (tools/src/beryl_tools/nightly.py), copied because nothing here
imports the tools."""

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
INTERVAL_SECONDS = 600


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
        "StartInterval": INTERVAL_SECONDS,
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
        f"status install: {path} loaded: `just status publish` from {main} every "
        f"{INTERVAL_SECONDS // 60} minutes; its log is {log_path()}; "
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
