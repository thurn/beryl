"""`by nightly`: the work that runs once a night on the development machine, from a launchd
agent (`just nightly-install`): today, removing the cache folders of checkouts that are gone
(`by prune-caches`), coverage of origin/staging (`by coverage`), the checks' verdicts the fast
check orders by (`by ci-history`) and every card's spec (`by specs --check`, the sweep the
tests sample). It costs no agent tokens and needs no session open. Its record is this
machine's, in `$BERYL_CACHE/nightly`: `log.jsonl` (one line per attempt, skipped or not) and
`last-output.txt` (the last run's output). More nightly work, such as the presentation track's
motion captures and the nightly fuzz, joins as another [`Step`] in [`STEPS`].

The agent fires at 03:00 and hourly until 08:00. Each firing runs only if the last success is
over [`FRESH_HOURS`] old and the machine is not busy, so the hourly retries cost nothing once a
night has succeeded.
"""

from __future__ import annotations

import argparse
import json
import os
import plistlib
import shutil
import subprocess
import sys
import time
from collections.abc import Callable
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import IO

from . import caches, coverage, paths, system

LABEL = "ca.thurn.beryl.nightly"
# The agent's first firing, and its last retry.
FIRST_HOUR = 3
LAST_HOUR = 8
# A success this recent skips the run.
FRESH_HOURS = 20
# Skip while the 5-minute load average is over this many times the core count.
BUSY_LOAD_PER_CORE = 2
# Coverage older than this many hours is flagged stale in `by ci-report`.
STALE_HOURS = 48


def folder() -> Path:
    """Where the nightly record goes: `$BERYL_CACHE/nightly`."""
    path = paths.cache_root() / "nightly"
    path.mkdir(parents=True, exist_ok=True)
    return path


def log_path() -> Path:
    return folder() / "log.jsonl"


def plist_path() -> Path:
    return Path.home() / "Library" / "LaunchAgents" / f"{LABEL}.plist"


@dataclass
class Step:
    """One piece of nightly work: `run(commit, out)` does it for the commit and returns an exit
    code, writing its output to `out`."""

    name: str
    run: Callable[[str, IO[str]], int]


def coverage_step(commit: str, out: IO[str]) -> int:
    """Per-check browser coverage of `commit`, at low priority (coverage.py)."""
    command = [sys.executable, "-m", "beryl_tools.cli", "coverage", "--commit", commit]
    return subprocess.run(
        system.utility(command),
        stdout=out,
        stderr=subprocess.STDOUT,
        stdin=subprocess.DEVNULL,
        check=False,
    ).returncode


def history_step(commit: str, out: IO[str]) -> int:
    """The checks' verdicts from the beads, kept for the fast check's ordering (verdicts.py)."""
    command = [sys.executable, "-m", "beryl_tools.cli", "ci-history"]
    return subprocess.run(
        command,
        stdout=out,
        stderr=subprocess.STDOUT,
        stdin=subprocess.DEVNULL,
        check=False,
    ).returncode


def specs_step(commit: str, out: IO[str]) -> int:
    """Every card's spec generated and listed, at low priority (`beryl_cli specs-check` with no
    card): the full sweep the tests, which take a bounded sample of cards, leave out."""
    command = [sys.executable, "-m", "beryl_tools.cli", "specs", "--check"]
    return coverage.in_tree(commit, system.utility(command), out)


STEPS: list[Step] = [
    Step("prune", caches.prune_step),
    Step("coverage", coverage_step),
    Step("history", history_step),
    Step("specs", specs_step),
]


def read_log() -> list[dict[str, object]]:
    path = log_path()
    if not path.exists():
        return []
    records: list[dict[str, object]] = []
    for line in path.read_text().splitlines():
        try:
            records.append(json.loads(line))
        except json.JSONDecodeError:
            continue  # A torn line, from a machine that went down mid-write.
    return records


def last_success(records: list[dict[str, object]]) -> datetime | None:
    for r in reversed(records):
        if r.get("outcome") == "ok":
            try:
                return datetime.fromisoformat(str(r["time"]))
            except (KeyError, ValueError):
                continue
    return None


def skip_reason(
    now: datetime, success: datetime | None, load: float, cores: int
) -> str | None:
    """Why this firing should not run, or None. The agent retries hourly, so a skip costs
    nothing: a run that is fresh enough is never repeated, and a busy machine is tried again.
    """
    if success is not None and now - success < timedelta(hours=FRESH_HOURS):
        hours = (now - success).total_seconds() / 3600
        return f"last success {hours:.1f} h ago, under {FRESH_HOURS} h"
    if load > BUSY_LOAD_PER_CORE * cores:
        return f"load average {load:.1f} over {BUSY_LOAD_PER_CORE} x {cores} cores"
    return None


def append(record: dict[str, object]) -> None:
    with open(log_path(), "a") as handle:
        handle.write(json.dumps(record, separators=(",", ":")) + "\n")


def fetch_tip() -> str | None:
    """Fetches origin and returns origin/staging's commit, or None if that failed."""
    fetched = subprocess.run(
        ["git", "fetch", "--quiet", "origin"],
        cwd=paths.REPO,
        capture_output=True,
        check=False,
    )
    if fetched.returncode:
        return None
    tip = subprocess.run(
        ["git", "rev-parse", "--verify", "origin/staging^{commit}"],
        cwd=paths.REPO,
        capture_output=True,
        text=True,
        check=False,
    )
    return tip.stdout.strip() if tip.returncode == 0 else None


def run_nightly(
    steps: list[Step],
    now: Callable[[], datetime] = lambda: datetime.now(timezone.utc),
    load: Callable[[], float] = lambda: system.loadavg()[1],
    cores: Callable[[], int] = lambda: os.cpu_count() or 1,
    tip: Callable[[], str | None] = fetch_tip,
) -> dict[str, object]:
    """One firing: skips, or runs every step on origin/staging's tip. Appends the outcome to
    the log and returns it: `ok` when every step exited 0, `failed` otherwise, `skipped` with a
    `reason`."""
    started = now()
    stamp = started.isoformat(timespec="seconds")
    reason = skip_reason(started, last_success(read_log()), load(), cores())
    record: dict[str, object]
    if reason is not None:
        record = {
            "time": stamp,
            "outcome": "skipped",
            "reason": reason,
        }
        append(record)
        return record
    try:
        with system.file_lock(folder() / ".lock", timeout=0):
            record = measure(steps, stamp, tip)
    except system.LockHeld:
        record = {
            "time": stamp,
            "outcome": "skipped",
            "reason": "another nightly run is in progress",
        }
    append(record)
    return record


def measure(
    steps: list[Step], stamp: str, tip: Callable[[], str | None]
) -> dict[str, object]:
    began = time.monotonic()
    commit = tip()
    if commit is None:
        return {
            "time": stamp,
            "outcome": "failed",
            "reason": "could not fetch origin/staging",
            "seconds": round(time.monotonic() - began, 1),
        }
    codes: dict[str, int] = {}
    with open(folder() / "last-output.txt", "w") as out:
        for step in steps:
            out.write(f"== {step.name} on {commit}\n")
            out.flush()
            codes[step.name] = step.run(commit, out)
    return {
        "time": stamp,
        "commit": commit,
        "seconds": round(time.monotonic() - began, 1),
        "outcome": "ok" if not any(codes.values()) else "failed",
        "steps": codes,
    }


def nightly_command(args: argparse.Namespace) -> int:
    record = run_nightly(STEPS)
    print(json.dumps(record))
    return 1 if record["outcome"] == "failed" else 0


def unsupported() -> bool:
    """Prints that the agent is unsupported here, if it is: launchd is macOS's."""
    if sys.platform == "darwin":
        return False
    print("nightly: the launchd agent is supported on macOS only; nothing to do")
    return True


def agent_environment() -> dict[str, str]:
    """What launchd's bare environment lacks: a PATH holding the tools coverage runs (uv, node,
    git, cargo, just), and the cache if it is not the default."""
    directories: list[str] = []
    for tool in ("uv", "node", "git", "cargo", "just"):
        found = shutil.which(tool)
        if found and str(Path(found).parent) not in directories:
            directories.append(str(Path(found).parent))
    directories += ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
    env = {"PATH": ":".join(directories), "HOME": str(Path.home())}
    if os.environ.get("BERYL_CACHE"):
        env["BERYL_CACHE"] = os.environ["BERYL_CACHE"]
    return env


def agent_plist(main: Path, uv: str) -> dict[str, object]:
    """The launchd agent: `by nightly` from the main checkout's tools at 03:00 and hourly
    until 08:00. Its paths are POSIX's, as launchd's are, wherever this runs."""
    log = folder() / "launchd.log"
    return {
        "Label": LABEL,
        "ProgramArguments": [
            uv,
            "run",
            "--quiet",
            "--project",
            (main / "tools").as_posix(),
        ]
        + ["by", "nightly"],
        "WorkingDirectory": main.as_posix(),
        "StartCalendarInterval": [
            {"Hour": hour, "Minute": 0} for hour in range(FIRST_HOUR, LAST_HOUR + 1)
        ],
        "EnvironmentVariables": agent_environment(),
        "StandardOutPath": log.as_posix(),
        "StandardErrorPath": log.as_posix(),
    }


def launchctl(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["launchctl", *args], capture_output=True, text=True, check=False
    )


def domain() -> str:
    return f"gui/{os.getuid()}"


def unload() -> None:
    """Unloads the agent if it is loaded."""
    launchctl("bootout", f"{domain()}/{LABEL}")


def install_command(args: argparse.Namespace) -> int:
    if unsupported():
        return 0
    main = paths.main_checkout()
    uv = shutil.which("uv")
    if main is None or uv is None:
        print("nightly-install: needs the main checkout and uv", file=sys.stderr)
        return 1
    path = plist_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(plistlib.dumps(agent_plist(main, uv)))
    unload()
    loaded = launchctl("bootstrap", domain(), str(path))
    if loaded.returncode:
        print(f"nightly-install: launchctl: {loaded.stderr.strip()}", file=sys.stderr)
        return 1
    print(
        f"nightly-install: {path} loaded: `by nightly` from {main} at "
        f"{FIRST_HOUR:02}:00, retried hourly until {LAST_HOUR:02}:00; its log is "
        f"{log_path()}; `just nightly-uninstall` removes it"
    )
    return 0


def uninstall_command(args: argparse.Namespace) -> int:
    if unsupported():
        return 0
    unload()
    plist_path().unlink(missing_ok=True)
    print(f"nightly-uninstall: {LABEL} unloaded, {plist_path()} removed")
    return 0


def coverage_age_line(now: datetime | None = None) -> str:
    """One line on the age of the newest coverage (`$BERYL_CACHE/ci/coverage/latest.json`,
    which coverage.py writes), so a nightly that stopped is visible."""
    latest = paths.cache_root() / "ci" / "coverage" / "latest.json"
    try:
        found = json.loads(latest.read_text())
        measured = datetime.fromisoformat(found["time"])
        commit = str(found["commit"])[:8]
    except (OSError, ValueError, KeyError):
        return "ci-report: no coverage recorded: `just nightly-install`, or `just coverage`"
    hours = ((now or datetime.now(timezone.utc)) - measured).total_seconds() / 3600
    age = f"{hours:.0f} h" if hours < 72 else f"{hours / 24:.0f} days"
    stale = f" (stale: see {log_path()})" if hours > STALE_HOURS else ""
    return f"ci-report: coverage of {commit} measured {age} ago{stale}"


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "nightly",
        help=f"the nightly work, unless one succeeded under {FRESH_HOURS} h ago or the "
        "machine is busy",
    )
    p.set_defaults(func=nightly_command)
    p = sub.add_parser(
        "nightly-install", help="load the launchd agent that runs `by nightly`"
    )
    p.set_defaults(func=install_command)
    p = sub.add_parser("nightly-uninstall", help="unload and remove that agent")
    p.set_defaults(func=uninstall_command)
