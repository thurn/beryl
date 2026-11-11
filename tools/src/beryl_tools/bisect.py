"""Bisecting a slow-CI failure (plan K21; docs/project-50k.md, Target architecture, Checks, "After
a merge").

When a step of `by ci` still fails after its rerun and no quarantine or pending entry excuses it
(checks.py, `run_lane`), `after` reruns what it named as failed, narrowed as the rerun was, in a
detached worktree under the cache at the commits since master (`git rev-list --first-parent
origin/master..HEAD`), and binary-searches for the first commit at which each fails. A probe runs
every check whose search has reached the same commits, so checks with one culprit share one
search. It probes the tip first: a check that passes there is a flake and blames no commit.

Enact kills `by ci` at its `slow_timeout` (.enact.toml), so each probe dies in time to report the
range the search narrowed to. The log ends with the answer, which Enact copies into the Steward
bead with the log's tail: per check a `bisect:` line with its culprit and a `record:` line with
its record over 30 days (runlog.py's Tally, with the verdicts in its ci-check bead). Each probe's
checks go to the run log with script `bisect` and the probe's commit.

A culprit's fast check says whether the failure was a selection miss (K18; verdicts.py): `enact
push` runs it at the tip it pushes, so it is the first fast check in this host's run log at the
culprit or a later commit its bead delivered. A check that fast check did not run (not chosen, or
`cut` by its time box) is a miss, recorded on the check's bead once per culprit and printed in a
`miss:` line; with no fast check of the culprit here (it ran on another host, say) the line says
the miss is unknown and records nothing.

Only a step a rerun can narrow is bisected (`checks.Failure`): not a static check such as clippy,
nor one that timed out or could not build or start. A bisection that fails reports why and leaves
the run's verdict alone.
"""

from __future__ import annotations

import hashlib
import os
import platform
import shutil
import subprocess
import time
import tomllib
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path

from . import paths, runlog, system, verdicts, warm
from .checks import Failure, Outcome, Steps, WallClock
from .reruns import unproven

# Enact's `slow_timeout` where .enact.toml does not say.
ENACT_TIMEOUT = 900.0
# Wall seconds kept back from Enact's timeout for the report after the last probe.
MARGIN = 60.0
# Less than this left, a probe would only be killed: the search stops.
LEAST_PROBE = 20.0
# Enact copies the last 60 lines of the log into the Steward bead; the answer keeps to these.
LINES = 25
# A check's record lines past these are summed up in one.
RECORDS_EACH = 3
DAYS = 30
BASE = "origin/master"
# The fast check's script in the run log (checks.fast).
FAST = "check"


def enact_timeout(repo: Path) -> float:
    try:
        config = tomllib.loads((repo / ".enact.toml").read_text())
        return float(config["checks"]["slow_timeout"])
    except (OSError, KeyError, TypeError, ValueError, tomllib.TOMLDecodeError):
        return ENACT_TIMEOUT


def time_left(started: float, repo: Path = paths.REPO) -> float:
    """Wall seconds before Enact kills a run begun at `started` (time.monotonic), less the
    margin for the report."""
    return enact_timeout(repo) - MARGIN - (time.monotonic() - started)


def git(cwd: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args], cwd=cwd, capture_output=True, text=True, check=False
    )


def out(cwd: Path, *args: str) -> str:
    run = git(cwd, *args)
    return run.stdout.strip() if run.returncode == 0 else ""


@dataclass
class Suspect:
    """What one failed step's rerun narrowed to: one of its arguments (none: the whole step),
    with the records of the checks that argument names, as they failed at the tip."""

    lane: str
    failure: Failure
    arg: str | None
    records: list[dict[str, object]]

    def checks(self) -> list[str]:
        return list(dict.fromkeys(str(r["check"]) for r in self.records))

    def name(self) -> str:
        checks = self.checks()
        if len(checks) == 1:
            return checks[0]
        return f"{self.lane}:{self.arg or self.failure.step.name}"


def names(arg: str, check: str) -> bool:
    """Whether the narrowing argument `arg` names the run-log check `check`: a title or a shot
    is the check (`ui-test:<title>`), a verb check is named whole
    (`verb:<verb>/<seat>/<layout>`), a nextest `<binary> <test>` is `rust:<binary>::<test>`
    and a card is each of its scenarios (`oracle:<card>/<scenario>/<engine>`)."""
    tail = check.split(":", 1)[-1]
    return (
        check == arg
        or tail in (arg, arg.replace(" ", "::", 1))
        or tail.startswith(arg + "/")
    )


def suspects(outcomes: list[Outcome]) -> list[Suspect]:
    found: list[Suspect] = []
    for o in outcomes:
        f = o.failure
        if f is None:
            continue
        failing = [r for r in f.records if r["outcome"] == "fail"]
        if not f.args:
            found.append(Suspect(o.lane, f, None, failing))
        for arg in f.args:
            mine = [r for r in failing if names(arg, str(r["check"]))]
            found.append(Suspect(o.lane, f, arg, mine))
    return found


class Tree:
    """A detached worktree under the cache, kept between runs so that its builds stay warm. One
    per repository, since git registers a worktree with its repository."""

    def __init__(self, repo: Path) -> None:
        common = out(repo, "rev-parse", "--path-format=absolute", "--git-common-dir")
        key = hashlib.sha256(common.encode()).hexdigest()[:12]
        self.repo = repo
        self.path: Path = paths.cache_root() / "ci" / f"bisect-{key}"

    def at(self, commit: str) -> bool:
        """Checks out `commit`, making the worktree first where git does not know it; whether
        it could. Ignored files, such as the builds, stay. No hook runs: one that syncs state,
        such as the beads', must not see an old commit."""
        quiet = ("-c", f"core.hooksPath={os.devnull}")
        if (self.path / ".git").exists() and git(
            self.path, "rev-parse"
        ).returncode == 0:
            return all(
                git(self.path, *quiet, *args).returncode == 0
                for args in (
                    ("checkout", "--detach", "--force", "--quiet", commit),
                    ("clean", "-fdq"),
                )
            )
        shutil.rmtree(self.path, ignore_errors=True)
        git(self.repo, "worktree", "prune")
        self.path.parent.mkdir(parents=True, exist_ok=True)
        add = ("worktree", "add", "--detach", "--force", str(self.path), commit)
        return git(self.repo, *quiet, *add).returncode == 0

    def moved(self, arg: str) -> str:
        """`arg` with a path in the repository moved into the worktree, where the worktree has
        it: not the run's interpreter in the ignored `tools/.venv`, say."""
        repo = str(self.repo)
        if arg == repo or arg.startswith(repo + os.sep):
            moved = str(self.path) + arg[len(repo) :]
            if Path(moved).exists():
                return moved
        return arg


# Whether each of the suspects fails at the commit, with whether to call a pass a pass only when
# the run passed each check that failed (`reruns.unproven`); None when the probe did not finish.
Probe = Callable[[str, list[Suspect], bool], list[bool] | None]


class Prober:
    """Runs suspects' steps at a commit in the tree, each step once with all its arguments and
    the steps at once, under a deadline on the wall clock."""

    def __init__(self, tree: Tree, steps: Steps) -> None:
        self.tree = tree
        self.steps = steps

    def __call__(
        self, commit: str, group: list[Suspect], prove: bool
    ) -> list[bool] | None:
        if self.steps.deadline - self.steps.clock.now() < LEAST_PROBE:
            return None
        if not self.tree.at(commit):
            print(f"bisect: could not check out {commit[:10]} in {self.tree.path}")
            return None
        if any(s.failure.warm for s in group):
            warm.ensure(self.tree.path)
        run = runlog.Run(runlog.BISECT, commit=commit, place=runlog.BISECT)
        by_step: dict[int, list[Suspect]] = {}
        for s in group:
            by_step.setdefault(id(s.failure), []).append(s)
        with ThreadPoolExecutor(max_workers=len(by_step)) as pool:
            done = list(pool.map(lambda m: self.step(run, m, prove), by_step.values()))
        fails = {id(s): f for chunk in done for s, f in chunk}
        if any(f is None for f in fails.values()):
            print(f"bisect: {commit[:10]}: out of time")
            return None
        found = [bool(fails[id(s)]) for s in group]
        print(f"bisect: {commit[:10]}: {sum(found)} of {len(group)} checks fail")
        return found

    def step(
        self, run: runlog.Run, members: list[Suspect], prove: bool
    ) -> list[tuple[Suspect, bool | None]]:
        f = members[0].failure
        lane = members[0].lane
        whole = any(m.arg is None for m in members)
        args = [] if whole else [str(m.arg) for m in members]
        cmd = [self.tree.moved(c) for c in f.step.cmd] + args
        # A module the step runs with `-m` comes from the commit's tools, not this run's.
        src = str(self.tree.path / "tools" / "src")
        python = os.pathsep.join(filter(None, [src, os.environ.get("PYTHONPATH")]))
        env = {**os.environ, "PYTHONPATH": python}
        with run.step(env) as (step_env, results):
            started = time.perf_counter()
            cwd = Path(self.tree.moved(str(f.step.cwd)))
            code, output = self.steps.run(cmd, cwd, step_env)
            took = time.perf_counter() - started
            records = run.log(lane, f.step.name, code == 0, took, results)
        if code is None:
            return [(m, None) for m in members]
        named = f.step.failed(output) if code != 0 and f.step.failed else []
        found: list[tuple[Suspect, bool | None]] = []
        for m in members:
            fails = code != 0 and (m.arg is None or not named or m.arg in named)
            if prove and not fails:
                fails = bool(unproven(m.records, records, f"{lane}:{f.step.name}"))
            found.append((m, fails))
        return found


@dataclass
class Found:
    """Where the search left some suspects: `culprit` (the first commit they fail at, `low`),
    `flaky` (they passed at the tip), `master` (they fail before the first commit too) or
    `range` (out of time with the culprit among commits `low` to `high`)."""

    verdict: str
    suspects: list[Suspect]
    low: int = 0
    high: int = 0


def search(
    commits: list[str], base: str, group: list[Suspect], probe: Probe
) -> list[Found]:
    """Bisects `group` over `commits` (oldest first, the tip last) down from `base`, the commit
    before them. Each probe takes the suspects whose search has reached the same commits: they
    split by whether they failed there, so the ones with one culprit stay together."""
    last = len(commits) - 1
    tip = probe(commits[-1] if commits else base, group, True)
    if tip is None:
        return [Found("range", group, 0, last)]
    found: list[Found] = []
    flaky = [s for s, fails in zip(group, tip) if not fails]
    if flaky:
        found.append(Found("flaky", flaky))
    failing = [s for s, fails in zip(group, tip) if fails]
    if failing and not commits:
        found.append(Found("master", failing))
        failing = []
    queue: list[tuple[int, int, list[Suspect]]] = (
        [(0, last, failing)] if failing else []
    )
    timed_out = False
    while queue:
        low, high, members = queue.pop(0)
        if timed_out:
            found.append(Found("range", members, low, high))
            continue
        at = (low + high) // 2 if low < high else -1
        # Down to one commit: the first is the culprit only if they pass before it.
        result = probe(commits[at] if at >= 0 else base, members, False)
        if result is None:
            timed_out = True
            queue.insert(0, (low, high, members))
            continue
        fail = [s for s, fails in zip(members, result) if fails]
        pass_ = [s for s, fails in zip(members, result) if not fails]
        if at < 0:
            if fail:
                found.append(Found("master", fail))
            if pass_:
                found.append(Found("culprit", pass_, 0, 0))
            continue
        if fail:
            if at == low and low > 0:
                found.append(Found("culprit", fail, low, low))
            else:
                queue.append((low, at, fail))
        if pass_:
            if at + 1 == high:
                found.append(Found("culprit", pass_, high, high))
            else:
                queue.append((at + 1, high, pass_))
    return found


def delivering(
    found: list[Found], commits: list[str]
) -> dict[str, tuple[str, list[str]]]:
    """Each culprit's bead, with every commit it delivered (verdicts.Beads.delivering)."""
    beads: dict[str, tuple[str, list[str]]] = {}
    for f in found:
        sha = commits[f.low] if f.verdict == "culprit" else ""
        if sha and sha not in beads:
            beads[sha] = verdicts.STORE.delivering({sha})
    return beads


def fast_run(
    culprit: str,
    commits: list[str],
    delivered: list[str],
    log: list[dict[str, object]],
    repo: Path,
) -> tuple[str, list[dict[str, object]]]:
    """The fast check that saw `culprit`, with its records: the first in `log` at the culprit or
    a later commit among `commits` that its bead `delivered`, matched by commit or by tree; none
    when no such commit has one."""
    at = commits.index(culprit)
    mine = set(delivered)
    last = max((i for i in range(at, len(commits)) if commits[i] in mine), default=at)
    fast = [r for r in log if r.get("script") == FAST]
    for commit in commits[at : last + 1]:
        tree = out(repo, "rev-parse", f"{commit}^{{tree}}")
        found = [
            r
            for r in fast
            if r.get("commit") == commit or (tree and r.get("tree") == tree)
        ]
        if found:
            return commit, found
    return "", []


def unchosen(
    check: str, lane: str, records: list[dict[str, object]], whole: bool = False
) -> str:
    """Why the fast check whose records these are did not run `check`, a check of `lane` or
    with `whole` the lane's step itself (`<lane>:<step>`, logged only when it failed or named
    no checks); empty when it did."""
    ran = [r for r in records if r.get("lane") == lane and r.get("outcome") != "skip"]
    if whole and ran:
        return ""
    mine = [r for r in records if r.get("check") == check]
    if any(r.get("outcome") != "skip" for r in mine):
        return ""
    if any(r.get("chosen") == "cut" for r in mine):
        return "cut"
    if mine:
        return "skipped"
    if any(r.get("lane") == lane for r in records):
        return "not chosen"
    return f"its {lane} lane did not run"


def misses(
    found: list[Found],
    commits: list[str],
    beads: dict[str, tuple[str, list[str]]],
    log: list[dict[str, object]],
    history: dict[str, list[verdicts.Entry]],
    repo: Path,
) -> dict[str, str]:
    """A `miss:` line for each culprit's check that its fast check did not run, recording the
    miss on the check's bead (and in `history`) unless one names the culprit already, and an
    unknown one for each check whose culprit has no fast check in `log`."""
    lines: dict[str, str] = {}
    for f in found:
        if f.verdict != "culprit":
            continue
        sha = commits[f.low]
        bead, delivered = beads.get(sha, ("", []))
        fast, records = fast_run(sha, commits, delivered, log, repo)
        for s in f.suspects:
            for check in s.checks():
                if not fast:
                    lines[check] = (
                        f"miss: {check} unknown (no fast check of {sha[:10]} in this"
                        " host's run log)"
                    )
                    continue
                whole = check == f"{s.lane}:{s.failure.step.name}"
                why = unchosen(check, s.lane, records, whole)
                if not why:
                    continue
                lines[check] = (
                    f"miss: {check} (fast check at {fast[:10]} did not choose it: {why})"
                )
                known = history.setdefault(check, [])
                if any(
                    e.get("verdict") == "miss" and e.get("commit") == sha for e in known
                ):
                    continue
                entry: verdicts.Entry = {
                    "time": verdicts.stamp(verdicts.now()),
                    "host": platform.node(),
                    "commit": sha,
                    "script": "ci",
                    "lane": s.lane,
                    "verdict": "miss",
                    "fast": fast,
                    "chosen": why,
                }
                if bead:
                    entry["bead"] = bead
                if verdicts.STORE.record(check, s.lane, entry):
                    known.append(entry)
    return lines


def record_line(check: str, t: runlog.Tally) -> str:
    v = t.verdicts
    return (
        f"record: {check}  runs {t.runs}, failures {t.failures}, flaky {v.get('flaky', 0)},"
        f" stale {v.get('stale', 0)}, catch {v.get('catch', 0)}, miss {v.get('miss', 0)}"
        f" ({DAYS} days)"
    )


def answer(
    found: list[Found],
    commits: list[str],
    base: str,
    repo: Path,
    tallies: dict[str, runlog.Tally],
    beads: dict[str, tuple[str, list[str]]],
    missed: dict[str, str],
) -> list[list[str]]:
    """A `bisect:` line per suspect, then per check it names any `miss:` line and a `record:`
    line."""
    n = len(commits)
    since = f"({n} commit{'' if n == 1 else 's'} since master)"
    blocks: list[list[str]] = []
    for f in found:
        if f.verdict == "culprit":
            sha = commits[f.low]
            subject = out(repo, "log", "-1", "--format=%s", sha)
            name = beads.get(sha, ("", []))[0]
            bead = f"  bead {name}" if name else ""
            said = f"culprit {sha[:10]} {subject}  {since}{bead}"
        elif f.verdict == "flaky":
            said = "flaky: passed at the tip on its bisection rerun"
        elif f.verdict == "master":
            said = f"fails at {base[:10]} too: no commit since master to blame"
        elif commits:
            span = f"{commits[f.low][:10]}..{commits[f.high][:10]}"
            said = f"out of time: culprit among {f.high - f.low + 1} commits {span}  {since}"
        else:
            said = "out of time"
        for s in f.suspects:
            checks = s.checks() or [s.name()]
            lines = [f"bisect: {s.name()}  {said}"]
            for c in checks[:RECORDS_EACH]:
                lines += [missed[c]] if c in missed else []
                lines.append(record_line(c, tallies.get(c, runlog.Tally())))
            if len(checks) > RECORDS_EACH:
                lines.append(f"record: and {len(checks) - RECORDS_EACH} more checks")
            blocks.append(lines)
    return blocks


def after(outcomes: list[Outcome], started: float, repo: Path = paths.REPO) -> None:
    """Bisects the failed lanes in `outcomes` of a run begun at `started` (time.monotonic) and
    prints the answer last. Never raises: a run that failed has failed, bisected or not.
    """
    try:
        bisect(outcomes, started, repo)
    except Exception as e:
        print(f"bisect: could not bisect: {e}")


def bisect(outcomes: list[Outcome], started: float, repo: Path) -> None:
    since = out(repo, "rev-list", "--first-parent", "--reverse", f"{BASE}..HEAD")
    commits = since.split()
    base = out(repo, "rev-parse", f"{commits[0]}^" if commits else "HEAD")
    group = suspects(outcomes)
    unnamed = [o.lane for o in outcomes if o.failure is None]
    left = time_left(started, repo)
    print(
        f"bisect: {len(group)} failed checks over {len(commits)} commits since"
        f" {BASE} ({base[:10]}), {max(0.0, left):.0f} s to search"
    )
    found: list[Found] = []
    if group:
        tree = Tree(repo)
        tree.path.parent.mkdir(parents=True, exist_ok=True)
        try:
            with system.file_lock(tree.path.with_suffix(".lock"), timeout=LEAST_PROBE):
                steps = Steps(left, WallClock())
                found = search(commits, base, group, Prober(tree, steps))
        except system.LockHeld:
            print(f"bisect: another bisection holds {tree.path}")
            found = [Found("range", group, 0, len(commits) - 1)]
    since = datetime.now(timezone.utc) - timedelta(days=DAYS)
    history = verdicts.STORE.history(
        [c for s in group for c in s.checks() or [s.name()]]
    )
    log = runlog.read_log(since)
    beads = delivering(found, commits)
    missed = misses(found, commits, beads, log, history, repo)
    tallies, _ = runlog.tally(log, history, since)
    blocks = answer(found, commits, base, repo, tallies, beads, missed)
    lines = [
        f"bisect: {lane}  not bisected: a static check, a timeout or a build that failed"
        for lane in unnamed
    ]
    for i, block in enumerate(blocks):
        if len(lines) + len(block) > LINES - 1:
            lines.append(
                f"bisect: and {len(blocks) - i} more checks; `just ci-report` has their records"
            )
            break
        lines += block
    for line in lines:
        print(line)
