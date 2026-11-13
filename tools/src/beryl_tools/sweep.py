"""`by sweep`: the browser sweep, which runs the full `shots`, `ui-test` and `mobile` lanes on
origin/staging every [`EVERY`] from a launchd agent (`just sweep-install`), at utility priority
as the nightly's steps run (nightly.py).

These lanes are most of what slow CI used to spend, and where its timeouts and flaky failures
came from. Most of their value for a change is caught before the push, where the fast check
plays the browser checks the change can break (selection.py). What a full sweep adds is the
break no selection foresaw: a change to shared stage or engine code that breaks a verb or a
screen the selection did not pick. That need not run on every staging tip, nor block master, so
slow CI keeps only a few browser checks (`checks.SMOKE_TITLES`) and this sweep runs the rest.
Every 3 hours, at about 8 commits an hour, a failure leaves about 24 commits to search, where
a nightly would leave about 190.

Slow CI starts a sweep in the background after a run when one is due ([`start_background`]),
so no agent need be installed while staging moves. A firing skips when origin/staging has not moved since the last sweep, when the machine is
overloaded (`nightly.OVERLOAD_PER_CORE`) or when another sweep still runs. Otherwise it runs
`by sweep-run` in a detached worktree of the tip with that commit's own tools
(`coverage.in_tree`). A sweep that passes starts a coverage run of its commit in the background
(`coverage.refresh`), so the fast check's choice of browser checks has coverage at most a few
hours old.

A sweep failure never blocks master. `sweep-run` bisects each failed check over the commits
since the last sweep that passed (bisect.py, in a tree of its own), and files one P1 Steward
bead per lane, labeled [`LABEL`], naming the failed checks and the culprit commit or range. A
check that an open sweep bead already names gets a comment instead. Its record is this
machine's, in `$BERYL_CACHE/sweep`: `log.jsonl` (one line per firing) and `last-output.txt`.
"""

from __future__ import annotations

import argparse
import json
import os
import plistlib
import shlex
import shutil
import subprocess
import sys
import time
from collections.abc import Callable
from datetime import datetime, timedelta, timezone
from pathlib import Path

from . import (
    bisect,
    checks,
    coverage,
    mobile,
    nightly,
    nightly_failures,
    paths,
    system,
    verdicts,
)
from .changes import git, run_git

LABEL = "sweep-failure"
AGENT = "ca.thurn.beryl.sweep"
SCRIPT = "sweep"
EVERY = timedelta(hours=3)
# The lanes the sweep runs in full, which slow CI leaves to it (slow_ci.SWEPT).
LANES: tuple[str, ...] = ("shots", "ui-test", "mobile")
# The step of each browser lane, whose record (`<lane>:<step>`) stands for the whole step when it
# failed without naming a check (runlog.Run.records).
STEPS: dict[str, str] = {"shots": "ui-shots", "ui-test": "ui-test"}
# The wall seconds a sweep's bisection may take. Nothing waits on it: the next firing skips
# while it runs.
SEARCH = 1800.0
# The hang limit of the sweep's steps (hang.py's `$CHECK_HANG_LIMIT_S`). At utility priority on
# a loaded machine a full `ui-test` can take many times its slow-CI time, and a sweep blocks
# nothing, so only a step stuck for this long counts as hung.
HANG_LIMIT_S = 7200
PRIORITY = "1"


def folder() -> Path:
    """Where the sweep's record goes: `$BERYL_CACHE/sweep`."""
    path = paths.cache_root() / "sweep"
    path.mkdir(parents=True, exist_ok=True)
    return path


def log_path() -> Path:
    return folder() / "log.jsonl"


def plist_path() -> Path:
    return Path.home() / "Library" / "LaunchAgents" / f"{AGENT}.plist"


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


def append(record: dict[str, object]) -> None:
    with open(log_path(), "a") as handle:
        handle.write(json.dumps(record, separators=(",", ":")) + "\n")


def last_swept(records: list[dict[str, object]]) -> str:
    """The commit the last sweep that ran (passed or failed) tested, or empty."""
    for r in reversed(records):
        if r.get("outcome") in ("ok", "failed") and r.get("commit"):
            return str(r["commit"])
    return ""


def skip_reason(tip: str, swept: str, load: float, cores: int) -> str | None:
    """Why this firing should not run, or None: staging has not moved since the last sweep, or
    the machine is overloaded. The agent fires again in [`EVERY`], so a skip costs nothing.
    """
    if tip == swept:
        return f"origin/staging is still {tip[:10]}, swept already"
    if load > nightly.OVERLOAD_PER_CORE * cores:
        return f"load average {load:.1f} over {nightly.OVERLOAD_PER_CORE:.0f} x {cores} cores"
    return None


def run_in_tree(commit: str, since: str, output: Path) -> int:
    """`by sweep-run` in a detached worktree of `commit` at utility priority, its output to
    `output`."""
    command = [sys.executable, "-m", "beryl_tools.cli", "sweep-run"]
    command += ["--since", since] if since else []
    with open(output, "w") as out:
        return coverage.in_tree(commit, system.utility(command), out)


def run_sweep(
    now: Callable[[], datetime] = lambda: datetime.now(timezone.utc),
    load: Callable[[], float] = lambda: system.loadavg()[1],
    cores: Callable[[], int] = lambda: os.cpu_count() or 1,
    tip: Callable[[], str | None] = nightly.fetch_tip,
    run: Callable[[str, str, Path], int] = run_in_tree,
    refresh: Callable[[str], str] = coverage.refresh,
) -> dict[str, object]:
    """One firing: skips, or sweeps origin/staging's tip. Appends the outcome to the log and
    returns it: `ok`, `failed` or `skipped` with a `reason`."""
    stamp = now().isoformat(timespec="seconds")
    commit = tip()
    if commit is None:
        return logged(
            {
                "time": stamp,
                "outcome": "failed",
                "reason": "could not fetch origin/staging",
            }
        )
    records = read_log()
    reason = skip_reason(commit, last_swept(records), load(), cores())
    if reason is not None:
        return logged(
            {"time": stamp, "commit": commit, "outcome": "skipped", "reason": reason}
        )
    began = time.monotonic()
    try:
        with system.file_lock(folder() / ".lock", timeout=0):
            code = run(commit, last_passed(records), folder() / "last-output.txt")
    except system.LockHeld:
        reason = "another sweep is in progress"
        return logged({"time": stamp, "outcome": "skipped", "reason": reason})
    return logged(
        {
            "time": stamp,
            "commit": commit,
            "seconds": round(time.monotonic() - began, 1),
            "outcome": "failed" if code else "ok",
            "code": code,
            # The fast check's browser selection reads coverage of a passing commit.
            "coverage": "" if code else refresh(commit),
        }
    )


def logged(record: dict[str, object]) -> dict[str, object]:
    """`record`, appended to the log."""
    append(record)
    return record


def last_began(records: list[dict[str, object]]) -> datetime | None:
    """When the last sweep that ran (passed or failed) began, or None."""
    for r in reversed(records):
        if r.get("outcome") in ("ok", "failed") and r.get("commit"):
            try:
                return datetime.fromisoformat(str(r["time"]))
            except (KeyError, ValueError):
                continue
    return None


def start_background(
    now: Callable[[], datetime] = lambda: datetime.now(timezone.utc),
    start: Callable[[list[str], Path], bool] = coverage.detached,
) -> str:
    """Starts `by sweep` in the background from the main checkout's tools when one is due: no
    sweep has run in [`EVERY`] and none is running. Returns a line saying which. Slow CI calls
    this after each run on the Mac, so the sweep keeps its interval while staging moves, which
    is when it has something to find, with no agent installed; `just sweep-install` adds the
    launchd agent for the hours when nothing is pushed."""
    if sys.platform != "darwin":
        return "sweep: not started: the Mac runs the browser sweep"
    began = last_began(read_log())
    if began is not None and now() - began < EVERY:
        due = (began + EVERY - now()) // timedelta(minutes=1)
        return f"sweep: next due in {due} min"
    try:
        with system.file_lock(folder() / ".lock", timeout=0):
            pass
    except system.LockHeld:
        return "sweep: one is running"
    log = folder() / "background.log"
    if not start(["sweep"], log):
        return "sweep: not started: needs the main checkout and uv"
    return f"sweep: started in the background ({log})"


def last_passed(records: list[dict[str, object]]) -> str:
    """The commit of the last sweep that passed, or empty."""
    for r in reversed(records):
        if r.get("outcome") == "ok" and r.get("commit"):
            return str(r["commit"])
    return ""


# `by sweep-run`, in the tree of the commit swept.


def lanes() -> list[checks.Lane]:
    return [checks.shots_lane(), checks.ui_test_lane(), mobile.lane()]


def commits_since(since: str) -> list[str]:
    """The commits since `since` up to HEAD, oldest first, on the first-parent line: none when
    `since` is empty or HEAD does not descend from it."""
    if not since or run_git("merge-base", "--is-ancestor", since, "HEAD") is None:
        return []
    return git("rev-list", "--first-parent", "--reverse", f"{since}..HEAD").split()


def reproduce(lane: str, failed: list[str]) -> str:
    """The command that replays a lane's failed checks: by title or shot name, or the whole
    lane when its step failed as a whole (`<lane>:<step>`)."""
    if lane == "mobile":
        return "just mobile-check"
    recipe = "just ui-shots" if lane == "shots" else "just ui-test"
    if any(c in (f"{lane}:lane", f"{lane}:{STEPS.get(lane)}") for c in failed):
        return recipe
    # `shots:<name>` and `ui-test:<title>` by name or title; a verb or shape check by its id.
    names = [c.removeprefix(f"{lane}:") for c in failed]
    return " ".join([recipe, *(shlex.quote(n) for n in names)])


def body(
    commit: str, lane: str, failed: list[str], since: str, found: list[str], tail: str
) -> str:
    listed = "\n".join(f"- `{c}`" for c in failed)
    origin = (
        f"The last sweep that passed was at {since}."
        if since
        else "No sweep has passed on record, so there was nothing to bisect against."
    )
    searched = "\n".join(found) or "(no bisection: see above)"
    return (
        f"The browser sweep (`by sweep`, tools/src/beryl_tools/sweep.py) failed {len(failed)} "
        f"{lane} check(s) on origin/staging at {commit}. {origin}\n\n{listed}\n\n"
        f"Bisection over the commits since that sweep:\n```\n{searched}\n```\n\n"
        f"Replay them with `{reproduce(lane, failed)}`. Slow CI does not run these lanes, so "
        "`just ci` passing proves nothing here; the fast check runs only the checks a change "
        "can break.\n\n"
        "Before deleting a check, ask: could a fast, deterministic test (a Rust unit test, a "
        "Node test with no browser, a check against a recorded fixture) keep most of what this "
        "check protects? If so, write that test. Fix the product when the check caught a real "
        "break; a check that failed without one is deleted or rewritten.\n\n"
        f"Output tail:\n```\n{tail}\n```"
    )


def file(
    commit: str,
    failed: dict[str, list[str]],
    since: str,
    found: list[str],
    output: str,
    store: verdicts.Beads | None = None,
) -> list[str]:
    """Files the sweep's failed checks: a comment on each open sweep bead that names one of
    them already, and a new bead per lane for the rest. Returns a line per bead touched.
    """
    store = store or verdicts.STORE
    known = nightly_failures.open_beads(store, LABEL) if failed else {}
    tail = nightly_failures.tail(output)
    lines: list[str] = []
    for lane, names in sorted(failed.items()):
        mine = [line for line in found if any(n in line for n in names)] or found
        for bead, theirs in sorted(known.items()):
            again = [c for c in names if c in theirs]
            if again:
                text = body(commit, lane, again, since, mine, tail)
                code, _ = store.bd("comments", "add", bead, text)
                lines.append(
                    f"sweep: {bead}: still failing: {', '.join(again)}"
                    if code == 0
                    else f"sweep: {bead}: comment not added"
                )
        new = [c for c in names if not any(c in t for t in known.values())]
        if not new:
            continue
        more = f" and {len(new) - 1} more" if len(new) > 1 else ""
        code, out = store.bd(
            "create",
            "--title",
            f"Fix sweep {lane} failure: {new[0]}{more}",
            "--description",
            body(commit, lane, new, since, mine, tail),
            "--acceptance",
            f"`{reproduce(lane, new)}` passes at the staging tip.",
            "--priority",
            PRIORITY,
            "--labels",
            f"role:steward,{LABEL}",
            "--metadata",
            json.dumps(
                {
                    nightly_failures.CHECKS_KEY: "\n".join(new),
                    "enact_ci_host": nightly_failures.host(),
                }
            ),
            "--silent",
        )
        lines.append(
            f"sweep: {out.strip()}: filed {', '.join(new)}"
            if code == 0
            else f"sweep: {lane}: bead not filed for {', '.join(new)}"
        )
    return lines


def sweep_run(args: argparse.Namespace) -> int:
    """Runs the sweep's lanes at HEAD; on a failure, bisects it over the commits since
    `--since` and files it."""
    os.environ.setdefault("CHECK_HANG_LIMIT_S", str(HANG_LIMIT_S))
    commit = git("rev-parse", "HEAD")
    began = datetime.now(timezone.utc).replace(microsecond=0)
    report = checks.run_all(lanes(), SCRIPT, int(EVERY.total_seconds()))
    failed = [o for o in report.outcomes if not o.ok]
    if not failed:
        return 0
    commits = commits_since(args.since)
    found: list[str] = []
    if commits:
        found = bisect.bisect_since(
            failed, commits, SEARCH, paths.REPO, "the last passing sweep", SCRIPT
        )
        for line in found:
            print(line)
    lanes_failed = {o.lane for o in failed}
    checks_failed = {
        lane: names
        for lane, names in nightly_failures.failed_checks(commit, began, SCRIPT).items()
        if lane in lanes_failed
    }
    for o in failed:
        checks_failed.setdefault(o.lane, [f"{o.lane}:lane"])
    output = "\n".join(o.log for o in failed)
    for line in file(commit, checks_failed, args.since, found, output):
        print(line, file=sys.stderr)
    return report.code


# The launchd agent.


def agent_plist(main: Path, uv: str) -> dict[str, object]:
    """The launchd agent: `by sweep` from the main checkout's tools every [`EVERY`]."""
    log = folder() / "launchd.log"
    return {
        "Label": AGENT,
        "ProgramArguments": [uv, "run", "--quiet", "--project"]
        + [(main / "tools").as_posix(), "by", "sweep"],
        "WorkingDirectory": main.as_posix(),
        "StartInterval": int(EVERY.total_seconds()),
        "EnvironmentVariables": nightly.agent_environment(),
        "StandardOutPath": log.as_posix(),
        "StandardErrorPath": log.as_posix(),
    }


def sweep_command(args: argparse.Namespace) -> int:
    record = run_sweep()
    print(json.dumps(record))
    return 1 if record["outcome"] == "failed" else 0


def install_command(args: argparse.Namespace) -> int:
    if nightly.unsupported():
        return 0
    main = paths.main_checkout()
    uv = shutil.which("uv")
    if main is None or uv is None:
        print("sweep-install: needs the main checkout and uv", file=sys.stderr)
        return 1
    path = plist_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(plistlib.dumps(agent_plist(main, uv)))
    nightly.launchctl("bootout", f"{nightly.domain()}/{AGENT}")
    loaded = nightly.launchctl("bootstrap", nightly.domain(), str(path))
    if loaded.returncode:
        print(f"sweep-install: launchctl: {loaded.stderr.strip()}", file=sys.stderr)
        return 1
    hours = EVERY.total_seconds() / 3600
    print(
        f"sweep-install: {path} loaded: `by sweep` from {main} every {hours:.0f} h; its log"
        f" is {log_path()}; `just sweep-uninstall` removes it"
    )
    return 0


def uninstall_command(args: argparse.Namespace) -> int:
    if nightly.unsupported():
        return 0
    nightly.launchctl("bootout", f"{nightly.domain()}/{AGENT}")
    plist_path().unlink(missing_ok=True)
    print(f"sweep-uninstall: {AGENT} unloaded, {plist_path()} removed")
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    hours = EVERY.total_seconds() / 3600
    p = sub.add_parser(
        "sweep",
        help="the browser sweep of origin/staging (shots, ui-test, mobile), unless staging "
        "has not moved or the machine is overloaded",
    )
    p.set_defaults(func=sweep_command)
    p = sub.add_parser(
        "sweep-run",
        help="the browser sweep's lanes at HEAD, bisecting and filing what fails",
    )
    p.add_argument(
        "--since", default="", help="the last commit a sweep passed at, to bisect from"
    )
    p.set_defaults(func=sweep_run)
    p = sub.add_parser(
        "sweep-install",
        help=f"load the launchd agent that runs `by sweep` every {hours:.0f} h",
    )
    p.set_defaults(func=install_command)
    p = sub.add_parser("sweep-uninstall", help="unload and remove that agent")
    p.set_defaults(func=uninstall_command)
