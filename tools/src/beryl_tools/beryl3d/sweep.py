"""`just b3d sweep`: Beryl 3D's checks after a push (docs/beryl-3d.md, 'Checks and isolation').

Beryl 3D's checks are its own: a change to it runs only the fast check's `beryl3d` lane, and a
change to anything else runs none of them. So a change to the session, the fixture recorder or
the image cache can land and leave Beryl 3D broken without its author being told. This sweep
finds that afterwards. It is modelled on the browser sweep (beryl_tools/sweep.py), with three
differences: it runs only when something it depends on has changed ([`due`]), it runs on every
machine that runs slow CI, and it files its failures under Beryl 3D's epic for Beryl 3D's own
work to fix, never as Steward beads.

- **When.** Slow CI starts it in the background after each run ([`start_background`]) when
  staging holds a commit, since the last sweep on this machine, that changes one of
  [`inputs`]. A firing skips when the machine is overloaded (`nightly.OVERLOAD_PER_CORE`) or
  another sweep still runs.
- **What.** In a detached worktree of origin/staging's tip, with that commit's own tools, it
  runs every step of [`steps`] that runs on this machine: each step of the `beryl3d` lane in
  full (no time box, as the fast check would give it), then the sweep's own steps. Each step is
  a lane of its own (`b3d-<step>`), so one failing step does not stop the others.
- **Two parts.** The steps run in two parts, each in a tree of its own: the [`QUICK`] part (the
  lane's steps and `smoke`, minutes) and then the [`SLOW`] part (`gpu` and `frame-counts`, which
  build and count in the beryl3d-bench virtual machine and can take hours). The quick part
  saves the build directory and files its failures before the slow part starts. Each part has
  its own lock, so a later firing runs its quick part while an earlier one's slow part is still
  going, and skips its own slow part. The slow part runs only after a quick part that passed:
  when the build is broken, its failures would only repeat the quick part's.
- **Failures.** A failed step is bisected over the commits since the last sweep whose part
  passed, for at most [`SEARCH`] seconds per part (bisect.py, in a tree of its own), and filed
  as one P1 bead per step, labelled [`LABEL`], under the epic (`paths.EPIC`). A check that an
  open sweep bead already names gets a comment instead. Nothing here holds `master`.
- **Cost.** Its steps run at utility priority and take CPU tokens as the browser sweep's do, and
  are logged under the script [`SCRIPT`]. Slow CI's CPU budget counts only records of the script
  `ci` (check_value.slow_ci), so the sweep spends none of it.
- **Cache.** As soon as the quick part's build steps ([`BUILDS`]) pass, it saves the
  workspace's build directory to `$BERYL_CACHE/b3d/target` ([`save_target`]), which `just b3d`
  clones into a new worktree.

Its record is this machine's, in `$BERYL_CACHE/b3d/sweep`: `log.jsonl` (one line per firing,
with each part's seconds and exit code under `parts`) and each part's output,
`last-quick-output.txt` and `last-slow-output.txt`.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import shutil
import sys
import time
import tomllib
from collections.abc import Callable
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

from .. import (
    bisect,
    caches,
    checks,
    coverage,
    nightly,
    nightly_failures,
    system,
    verdicts,
    warm,
)
from .. import paths as repo_paths
from ..changes import git, run_git
from ..lanes import BY, Lane, Step
from ..reruns import whole
from . import lane, paths

SCRIPT = "b3d-sweep"
LABEL = "b3d-sweep"
PRIORITY = "1"
# Each step runs as a lane of its own, named this and the step's name.
LANE_PREFIX = "b3d-"
# The wall seconds a sweep's bisection may take.
SEARCH = 900.0
# The hang limit of the sweep's steps (hang.py's `$CHECK_HANG_LIMIT_S`). A cold build of the
# workspace at utility priority on a loaded machine can take many times its time in the fast
# check, and a sweep blocks nothing.
HANG_LIMIT_S = 7200
# The run's budget in load-adjusted seconds. A sweep blocks nothing, so it is never over.
BUDGET = 6 * 3600
MAC = "darwin"
WINDOWS = "win32"
# The machines that run slow CI, and so start sweeps: this Mac and Cygnus.
EVERY_MACHINE: tuple[str, ...] = (MAC, WINDOWS)
# The main workspace's crates whose changes can break Beryl 3D, with every crate they depend
# on by path ([`inputs`]): the session, the fixture recorder and the image cache.
LINKED: tuple[str, ...] = ("beryl_session", "beryl_cli", "beryl_art")
# The steps whose passing means the workspace built, so its build directory is worth saving.
BUILDS: tuple[str, ...] = ("clippy", "cargo-test")
# The two parts of a sweep, in the order they run.
QUICK = "quick"
SLOW = "slow"
PARTS: tuple[str, ...] = (QUICK, SLOW)


@dataclass(frozen=True)
class SweepStep:
    name: str
    cmd: tuple[str, ...]
    # The values of `sys.platform` it runs on.
    machines: tuple[str, ...] = EVERY_MACHINE
    # Which part of the sweep runs it.
    part: str = QUICK


# The sweep's own steps, run after the lane's. A check that only the sweep runs (every
# interface test on every tick, the counts in the virtual machine, the phone builds) is
# appended here, in the [`SLOW`] part when it takes more than a few minutes.
OWN: tuple[SweepStep, ...] = (
    # The app starts with no window and shows its first frame.
    SweepStep("smoke", (*BY, "b3d", "smoke")),
    # Every interface test with the checks applied on every tick: the exhaustive form of the
    # lane's `test` step.
    SweepStep("every-tick", (*BY, "b3d", "test", "--every-tick")),
    # The GPU's work: shader cost, and the fragments shaded counted in the beryl3d-bench
    # machine. Only this Mac's machine has been shown to count fragments repeatably.
    SweepStep("gpu", (*BY, "b3d", "gate", "--gpu"), (MAC,), SLOW),
    # Every recording's timeline against the web interface's, where the lane's step compares
    # only the recorded fixtures.
    SweepStep("timeline-parity-all", (*BY, "b3d", "timeline-parity")),
    # Instructions per frame under Callgrind in `beryl3d-bench`, an arm64 Linux machine that
    # only Apple silicon runs (counts.py).
    SweepStep("frame-counts", (*BY, "b3d", "gate", "--counts"), (MAC,), SLOW),
)


def steps() -> list[SweepStep]:
    """Every step of the sweep, in order: the `beryl3d` lane's, then [`OWN`]."""
    return [
        *(SweepStep(s.name, tuple(s.cmd)) for s in lane.steps()),
        *OWN,
    ]


def chosen(
    only: str | None, platform: str = sys.platform, part: str | None = None
) -> list[SweepStep]:
    """The steps a sweep runs on `platform`: those that run there, or only the step `only`;
    of the part `part` only, when it is given."""
    found = [s for s in steps() if platform in s.machines]
    if part is not None:
        found = [s for s in found if s.part == part]
    if only is None:
        return found
    return [s for s in found if s.name == only]


def lanes(found: list[SweepStep]) -> list[Lane]:
    """A lane for each step, so the steps run at once and each fails on its own. A failed step
    reruns once, whole, which also gives bisect.py a step to replay."""
    return [
        Lane(
            LANE_PREFIX + s.name,
            steps=[Step(s.name, list(s.cmd), repo_paths.REPO, whole)],
        )
        for s in found
    ]


def step_of(lane_name: str) -> str:
    return lane_name.removeprefix(LANE_PREFIX)


# When a sweep is due.


def linked_crates(rust: Path = repo_paths.RUST) -> list[str]:
    """The [`LINKED`] crates and every crate they depend on by path, as paths under `rust`,
    sorted. A crate not in this commit is left out."""
    found: set[Path] = set()
    todo = [rust / "crates" / name for name in LINKED]
    while todo:
        crate = todo.pop()
        manifest = crate / "Cargo.toml"
        if crate in found or not manifest.is_file():
            continue
        found.add(crate)
        tables = tomllib.loads(manifest.read_text())
        for table in ("dependencies", "build-dependencies"):
            for spec in tables.get(table, {}).values():
                if isinstance(spec, dict) and isinstance(spec.get("path"), str):
                    todo.append(Path(os.path.normpath(crate / spec["path"])))
    return sorted(c.relative_to(rust).as_posix() for c in found)


def inputs(rust: Path = repo_paths.RUST) -> re.Pattern[str]:
    """The paths a change to which makes a sweep due: Beryl 3D's own (its workspace, its data
    and its tools), the main workspace's manifest (whose lints Beryl 3D's must match), the
    [`linked_crates`], and the data files Beryl 3D reads from the rest of the project.
    """
    names = "|".join(re.escape(c) for c in linked_crates(rust))
    crates = rf"|^rust/({names})/" if names else ""
    return re.compile(
        r"^(rust/beryl3d|data/beryl3d|tools/src/beryl_tools/beryl3d)/"
        r"|^rust/Cargo\.toml$"
        rf"{crates}"
        r"|^data/ui-conventions\.json$|^data/card-store\.tsv$|^ui/fixtures\.json$"
        r"|^data/design-tokens\.json$"
    )


def due(changed: list[str], pattern: re.Pattern[str]) -> list[str]:
    """The `changed` paths that make a sweep due."""
    return [p for p in changed if pattern.search(p)]


def changed_since(since: str, commit: str) -> list[str] | None:
    """The paths changed from `since` to `commit`, or None when there is no `since` or
    `commit` does not descend from it, so nothing says what changed."""
    if not since or run_git("merge-base", "--is-ancestor", since, commit) is None:
        return None
    return git("diff", "--name-only", since, commit).splitlines()


def not_due(commit: str, swept: str, pattern: re.Pattern[str]) -> str | None:
    """Why `commit` needs no sweep, given the commit the last sweep checked; None when it does."""
    if not swept:
        return None
    if commit == swept:
        return f"{commit[:10]} was swept already"
    changed = changed_since(swept, commit)
    if changed is not None and not due(changed, pattern):
        return f"nothing since {swept[:10]} changes Beryl 3D or what it builds on"
    return None


# The record.


def folder() -> Path:
    """Where the sweep's record goes: `$BERYL_CACHE/b3d/sweep`."""
    path = paths.cache() / "sweep"
    path.mkdir(parents=True, exist_ok=True)
    return path


def log_path() -> Path:
    return folder() / "log.jsonl"


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


def logged(record: dict[str, object]) -> dict[str, object]:
    """`record`, appended to the log."""
    with open(log_path(), "a") as handle:
        handle.write(json.dumps(record, separators=(",", ":")) + "\n")
    return record


def last_commit(records: list[dict[str, object]], outcomes: tuple[str, ...]) -> str:
    """The commit of the last full sweep whose outcome is one of `outcomes`, or empty."""
    for r in reversed(records):
        if r.get("outcome") in outcomes and r.get("commit"):
            return str(r["commit"])
    return ""


def last_swept(records: list[dict[str, object]]) -> str:
    """The commit the last full sweep checked, whether it passed or failed."""
    return last_commit(records, ("ok", "failed"))


def last_passed(records: list[dict[str, object]], part: str) -> str:
    """The commit of the last full sweep whose part `part` ran and passed, or empty. A record
    from before the sweep had parts passed both when its outcome is `ok`."""
    for r in reversed(records):
        parts = r.get("parts")
        if isinstance(parts, dict):
            ran = parts.get(part)
            passed = isinstance(ran, dict) and ran.get("code") == 0
        else:
            passed = r.get("outcome") == "ok"
        if passed and r.get("commit"):
            return str(r["commit"])
    return ""


def lock(part: str) -> Path:
    """The lock a firing holds while it runs the part `part`: `.lock` for the quick part,
    which [`start_background`] also tests, and `.slow.lock` for the slow part."""
    return folder() / (".lock" if part == QUICK else f".{part}.lock")


# A firing, from the main checkout's tools.


@dataclass(frozen=True)
class Request:
    """What `just b3d sweep` was asked for."""

    # The commit to sweep; origin/staging's tip, fetched, when empty.
    commit: str = ""
    # Only this step, which also sweeps whether or not one is due.
    step: str | None = None
    # Only this part ([`PARTS`]), which also sweeps whether or not one is due.
    part: str | None = None
    # Print the beads it would file instead of filing them, and bisect nothing.
    dry_run: bool = False
    # Sweep even when none is due or the machine is overloaded.
    force: bool = False

    def full(self) -> bool:
        """Whether the run counts as this machine's sweep of its commit in the log."""
        return self.step is None and self.part is None and not self.dry_run

    def parts(self) -> list[str]:
        """The parts the run runs, in order: the step's own part, or the part asked for, or
        both. A step that runs on no machine has none."""
        if self.step is not None:
            return [s.part for s in steps() if s.name == self.step][:1]
        return [self.part] if self.part is not None else list(PARTS)


def run_command(request: Request, part: str, since: str) -> list[str]:
    """`just b3d sweep-run` for the part `part` of `request`, as the tree of the commit runs
    it."""
    command = [
        sys.executable,
        "-m",
        "beryl_tools.cli",
        "b3d",
        "sweep-run",
        "--part",
        part,
    ]
    command += ["--since", since] if since else []
    command += ["--step", request.step] if request.step else []
    return command + (["--dry-run"] if request.dry_run else [])


def run_in_tree(commit: str, command: list[str], output: Path, part: str) -> int:
    """`command` in a detached worktree of `commit` at utility priority, its output to `output`
    and to this process's. The tree needs no clone of the main workspace's build directory:
    Beryl 3D's workspace has its own."""
    # Not the sweep's own folder, whose lock the firing holds while this runs, and a folder per
    # part: coverage.in_tree holds the folder's lock for the whole run and keeps one tree per
    # folder, so two parts in one folder could not run at once.
    work = folder() / ("work" if part == QUICK else f"work-{part}")
    work.mkdir(exist_ok=True)
    with open(output, "w") as out:
        code = coverage.in_tree(
            commit, system.utility(command), out, place=work, warm_rust=False
        )
    print(output.read_text(), end="")
    return code


def run_sweep(
    request: Request,
    now: Callable[[], datetime] = lambda: datetime.now(timezone.utc),
    load: Callable[[], float] = lambda: system.loadavg()[1],
    cores: Callable[[], int] = lambda: os.cpu_count() or 1,
    tip: Callable[[], str | None] = nightly.fetch_tip,
    pattern: Callable[[], re.Pattern[str]] = inputs,
    run: Callable[[str, list[str], Path, str], int] = run_in_tree,
) -> dict[str, object]:
    """One firing: skips, or sweeps the commit, the quick part and then the slow part. A full
    sweep's outcome (`ok`, `failed` or `skipped` with a `reason`) is appended to the log, with
    each part's seconds and exit code, or why it was skipped, under `parts`; every outcome is
    returned."""
    stamp = now().isoformat(timespec="seconds")
    record: dict[str, object] = {"time": stamp, "outcome": "failed"}
    commit = git("rev-parse", "--verify", request.commit) if request.commit else tip()
    if not commit:
        record["reason"] = f"could not resolve {request.commit or 'origin/staging'}"
        return finish(request, record)
    record["commit"] = commit
    records = read_log()
    swept = last_swept(records)
    reason = None
    if not request.force and request.step is None and request.part is None:
        reason = not_due(commit, swept, pattern())
        if reason is None and load() > nightly.OVERLOAD_PER_CORE * cores():
            per = nightly.OVERLOAD_PER_CORE
            reason = f"load average {load():.1f} over {per:.0f} x {cores()} cores"
    if reason is not None:
        return finish(request, {**record, "outcome": "skipped", "reason": reason})
    began = time.monotonic()
    parts: dict[str, dict[str, object]] = {}
    code = 0
    for part in request.parts():
        if code:
            parts[part] = {"skipped": "an earlier part failed"}
            continue
        command = run_command(request, part, last_passed(records, part))
        if request.dry_run:
            names = ", ".join(s.name for s in chosen(request.step, part=part))
            print(f"b3d sweep: dry run of {commit[:10]} in a detached tree: {names}")
            print(f"b3d sweep: {shlex.join(command)}")
        part_began = time.monotonic()
        try:
            with system.file_lock(lock(part), timeout=0):
                output = folder() / f"last-{part}-output.txt"
                part_code = run(commit, command, output, part)
        except system.LockHeld:
            if part == QUICK:
                reason = "another sweep is in progress"
                return finish(
                    request, {**record, "outcome": "skipped", "reason": reason}
                )
            parts[part] = {"skipped": f"another sweep's {part} part is in progress"}
            continue
        seconds = round(time.monotonic() - part_began, 1)
        parts[part] = {"seconds": seconds, "code": part_code}
        code = part_code
    record["seconds"] = round(time.monotonic() - began, 1)
    record["code"] = code
    record["parts"] = parts
    return finish(request, {**record, "outcome": "failed" if code else "ok"})


def finish(request: Request, record: dict[str, object]) -> dict[str, object]:
    """`record`, logged when it is a full sweep's: a run of one step or a dry run is neither
    the last sweep nor a skip of one."""
    return logged(record) if request.full() else record


def start_background(
    head: Callable[[], str] = lambda: git("rev-parse", "HEAD"),
    pattern: Callable[[], re.Pattern[str]] = inputs,
    start: Callable[[list[str], Path], bool] = coverage.detached,
) -> str:
    """Starts `just b3d sweep` in the background from the main checkout's tools when one is
    due at slow CI's commit and none is running, and returns a line saying which. Slow CI calls
    this after each run on each machine; the sweep runs in a session of its own, so its outcome
    never reaches slow CI's."""
    reason = not_due(head(), last_swept(read_log()), pattern())
    if reason is not None:
        return f"b3d sweep: not due: {reason}"
    try:
        with system.file_lock(lock(QUICK), timeout=0):
            pass
    except system.LockHeld:
        return "b3d sweep: one is running"
    log = folder() / "background.log"
    if not start(["b3d", "sweep"], log):
        return "b3d sweep: not started: needs the main checkout and uv"
    return f"b3d sweep: started in the background ({log})"


# `just b3d sweep-run`, in the tree of the commit swept.


def commits_since(since: str) -> list[str]:
    """The commits since `since` up to HEAD, oldest first, on the first-parent line: none when
    `since` is empty or HEAD does not descend from it."""
    if not since or run_git("merge-base", "--is-ancestor", since, "HEAD") is None:
        return []
    return git("rev-list", "--first-parent", "--reverse", f"{since}..HEAD").split()


def reproduce(step: str, commit: str) -> str:
    """The command that replays one step of the sweep at `commit`."""
    return f"just b3d sweep --step {shlex.quote(step)} --commit {shlex.quote(commit)}"


def body(
    commit: str,
    step: str,
    failed: list[str],
    since: str,
    found: list[str],
    tail: str,
) -> str:
    listed = "\n".join(f"- `{c}`" for c in failed)
    origin = (
        f"The last sweep that passed on this machine was at {since}."
        if since
        else "No sweep has passed on record, so there was nothing to bisect against."
    )
    searched = "\n".join(found) or "(no bisection: see above)"
    return (
        f"The Beryl 3D sweep (`just b3d sweep`, tools/src/beryl_tools/beryl3d/sweep.py) "
        f"failed its step `{step}` on {sys.platform} at {commit}. {origin}\n\n{listed}\n\n"
        f"Bisection over the commits since that sweep:\n```\n{searched}\n```\n\n"
        f"Replay it with `{reproduce(step, commit[:12])}`, or run the step's own command in a "
        "worktree of the staging tip (`just b3d` lists them).\n\n"
        "Beryl 3D's checks run only for a change to Beryl 3D, so a change to the main "
        "workspace (the session, the fixture recorder, the image cache, the data files) can "
        "break Beryl 3D without its author being told. Fix the break in Beryl 3D's code, not "
        "by reverting the commit that exposed it (docs/beryl-3d.md, 'Checks and isolation'); "
        "whoever is working on Beryl 3D takes this bead first.\n\n"
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
    """Files the sweep's failed checks, by step: a comment on each open sweep bead that names
    one of them already, and a new bead per step for the rest. Returns a line per bead
    touched."""
    store = store or verdicts.STORE
    known = nightly_failures.open_beads(store, LABEL) if failed else {}
    tail = nightly_failures.tail(output)
    lines: list[str] = []
    for lane_name, names in sorted(failed.items()):
        step = step_of(lane_name)
        mine = [line for line in found if any(n in line for n in names)] or found
        for bead, theirs in sorted(known.items()):
            again = [c for c in names if c in theirs]
            if again:
                text = body(commit, step, again, since, mine, tail)
                code, _ = store.bd("comments", "add", bead, text)
                lines.append(
                    f"b3d sweep: {bead}: still failing: {', '.join(again)}"
                    if code == 0
                    else f"b3d sweep: {bead}: comment not added"
                )
        new = [c for c in names if not any(c in t for t in known.values())]
        if not new:
            continue
        more = f" and {len(new) - 1} more" if len(new) > 1 else ""
        code, out = store.bd(
            "create",
            "--title",
            f"Fix the Beryl 3D sweep's {step} step: {new[0]}{more}",
            "--description",
            body(commit, step, new, since, mine, tail),
            "--acceptance",
            f"`{reproduce(step, 'origin/staging')}` passes at the staging tip.",
            "--priority",
            PRIORITY,
            "--parent",
            paths.EPIC,
            "--labels",
            f"role:executor,{LABEL}",
            "--metadata",
            json.dumps({nightly_failures.CHECKS_KEY: "\n".join(new)}),
            "--silent",
        )
        lines.append(
            f"b3d sweep: {out.strip()}: filed {', '.join(new)}"
            if code == 0
            else f"b3d sweep: {step}: bead not filed for {', '.join(new)}"
        )
    return lines


class DryRun(verdicts.Beads):
    """bd as the sweep's filing uses it, reading the real beads but printing each bead or
    comment it would write instead of writing it."""

    def __init__(self, real: verdicts.Beads) -> None:
        self.real = real

    def bd(self, *args: str) -> tuple[int, str]:
        if args[0] == "list":
            return self.real.bd(*args)
        if args[0] == "create":
            flag = lambda name: args[args.index(name) + 1]  # noqa: E731
            print(
                f"b3d sweep: would file under {flag('--parent')}, labelled "
                f"{flag('--labels')}, P{flag('--priority')}: {flag('--title')}\n"
                f"{flag('--description')}\nAcceptance: {flag('--acceptance')}"
            )
            return 0, "(dry run)"
        print(f"b3d sweep: would add to {args[2]} the comment:\n{args[3]}")
        return 0, ""


def save_target(target: Path, cached: Path) -> bool:
    """Replaces `cached` with a clone of the build directory `target`, less its incremental
    caches: rustc keys each to the absolute path of the tree that built it and discards it
    anywhere else (caches.save_kept). Under the lock `just b3d` clones the cache with.

    The clone is held to the main cache's budget (warm.py, `prune`) before it replaces the
    cache. Without that, every saved cache kept every artifact of every earlier build, and every
    new worktree cloned them all: on 2026-11-16 the cache was 44 GB, with 77,077 files in
    debug/deps and nine copies of `libberyl_scene-*.rlib`, and free disk fell 174 GB in two days.
    """
    if not target.is_dir():
        return False
    copy = cached.with_name(f".{cached.name}-new-{os.getpid()}")
    old = cached.with_name(f".{cached.name}-old-{os.getpid()}")
    with system.file_lock(paths.target_lock()):
        shutil.rmtree(copy, ignore_errors=True)
        if not warm.clone_into(target, copy):
            return False
        for incremental in caches.incremental_folders(copy):
            shutil.rmtree(incremental, ignore_errors=True)
        warm.prune(copy)
        # The sources' times as this build saw them, which `cli.warm_target` gives the files
        # of a new worktree after it clones the cache.
        warm.record_sources(repo_paths.REPO, copy)
        if cached.exists():
            cached.rename(old)
        copy.rename(cached)
    shutil.rmtree(old, ignore_errors=True)
    return True


def failed_by_step(
    commit: str, began: datetime, failed: list[checks.Outcome]
) -> dict[str, list[str]]:
    """The checks each failed step logged as failed, or the step's own name when it logged
    none."""
    names = {o.lane for o in failed}
    found = {
        lane_name: listed
        for lane_name, listed in nightly_failures.failed_checks(
            commit, began, SCRIPT
        ).items()
        if lane_name in names
    }
    for o in failed:
        found.setdefault(o.lane, [f"{o.lane}:{step_of(o.lane)}"])
    return found


def sweep_run(args: argparse.Namespace) -> int:
    """Runs the steps of one part of the sweep at HEAD, or all of them when no `--part` is
    given; on a failure, bisects it over the commits since `--since` and files it, or with
    `--dry-run` prints what it would file."""
    os.environ.setdefault("CHECK_HANG_LIMIT_S", str(HANG_LIMIT_S))
    found_steps = chosen(args.step, part=args.part)
    if not found_steps:
        names = ", ".join(s.name for s in chosen(None, part=args.part))
        print(f"b3d sweep-run: no step {args.step!r} here; the steps are {names}")
        return 2
    commit = git("rev-parse", "HEAD")
    began = datetime.now(timezone.utc).replace(microsecond=0)
    report = checks.run_all(lanes(found_steps), SCRIPT, BUDGET)
    failed = [o for o in report.outcomes if not o.ok]
    built = {step_of(o.lane) for o in report.outcomes if o.ok}
    if not args.dry_run and set(BUILDS) <= built:
        saved = save_target(paths.WORKSPACE / "target", paths.cache() / "target")
        print(f"b3d sweep: build directory {'saved' if saved else 'not saved'}")
    if not failed:
        return 0
    commits = [] if args.dry_run else commits_since(args.since)
    found: list[str] = []
    if commits:
        found = bisect.bisect_since(
            failed,
            commits,
            SEARCH,
            repo_paths.REPO,
            "the last passing sweep",
            SCRIPT,
        )
        for line in found:
            print(line)
    output = "\n".join(o.log for o in failed)
    store = DryRun(verdicts.STORE) if args.dry_run else None
    by_step = failed_by_step(commit, began, failed)
    for line in file(commit, by_step, args.since, found, output, store):
        print(line, file=sys.stderr)
    return report.code


# The commands, which cli.py lists.


def sweep_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--commit",
        default="",
        help="the commit to sweep (default: origin/staging's tip)",
    )
    parser.add_argument("--step", help="run only this step, whether or not one is due")
    parser.add_argument(
        "--part",
        choices=PARTS,
        help="run only this part of the sweep, whether or not one is due",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="print the steps and the beads it would file, filing and bisecting nothing",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="sweep even when none is due or the machine is overloaded",
    )


def sweep_command(args: argparse.Namespace) -> int:
    request = Request(args.commit, args.step, args.part, args.dry_run, args.force)
    record = run_sweep(request)
    print(json.dumps(record))
    return 1 if record["outcome"] == "failed" else 0


def sweep_run_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--since", default="", help="the last commit a sweep passed at, to bisect from"
    )
    parser.add_argument("--step", help="run only this step")
    parser.add_argument("--part", choices=PARTS, help="run only this part's steps")
    parser.add_argument("--dry-run", action="store_true")
