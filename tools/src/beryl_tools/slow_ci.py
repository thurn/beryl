"""`by ci`: slow CI on the staging tip, running only the lanes and checks that what changed since
each lane last passed can affect, and every lane once an hour.

Each lane has a base: the commit at which it last passed on this machine, recorded in
`$BERYL_CACHE/ci/lane-passes.json` ([`Passes`]), when the commit checked out descends from it
and it is newer than where the checkout left `origin/master`; else that merge base. A lane that
fails keeps its old base, so it runs on every later run until it passes. A lane passes at a
commit only when every check it holds passes there, and a change that cannot reach it cannot
change that. So master, which moves only when slow CI passes, never moves past a lane that
would fail, as long as these rules say truly which changes reach which lane.

A lane runs when a path changed since its base reaches it ([`lane_reason`]): the Rust lane for
a change under rust/ or to a data file a crate reads, the browser smoke lane for one under rust/
or ui/, and so on. Within a lane, the Rust lane tests only the packages the change can break
(`rust_tests.affected_packages`), and the oracle lane plays the changed cards and a sample
(oracle_selection.py). A lane left out gets a `skip` record in the run log that says why.

The full browser lanes (`shots`, `ui-test`) and the phone builds (`mobile`), [`SWEPT`], are not
slow CI's: the fast check plays the browser checks a change can break, and the browser sweep
(sweep.py) runs all three on origin/staging every few hours, filing what fails without blocking
master. A run starts the sweep in the background when one is due (`sweep.start_background`). Slow CI keeps one small browser check, `ui-smoke` (`checks.SMOKE_TITLES`).

The tools are not judged by path. Each lane's pass records its fingerprint ([`fingerprint`]):
its steps' commands, the source of every module of tools/src they run or import, of the modules
that judge a step's results ([`JUDGES`]) and of the tools' environment. A lane whose fingerprint
changed since its last pass runs in full, as does one with no pass on record, one whose base git
cannot diff against, and one a changed path reaches that no rule here places
([`whole_reason`]). The runner itself (checks.py) is not in the fingerprints: every run runs it.

Every lane runs in full when the last full sweep on this machine began over [`FULL_EVERY`] ago,
or `$BERYL_CI_FULL` is set. Slow CI runs only while staging moves, so a check that the selection
wrongly left out fails within the hour, and bisect.py finds the commit that broke it.

`by ci <lanes>` runs the lanes named, in full, the [`SWEPT`] ones too.
"""

from __future__ import annotations

import argparse
import ast
import hashlib
import json
import os
import platform
import re
import sys
import time
from collections.abc import Callable, Iterable, Mapping
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path

from . import (
    bench,
    checks,
    mobile,
    oracle_selection,
    paths,
    runlog,
    script_checks,
    speed,
    sweep,
    system,
    verdicts,
)
from .changes import changed_paths, master_base, run_git
from .rust_tests import affected_packages

# How long after a full sweep began slow CI runs every lane again.
FULL_EVERY = timedelta(hours=1)
FULL_ENV = "BERYL_CI_FULL"
# The paths no lane but `repo` (Markdown formatting) and `names` reads: documentation, agent
# instructions and skills, editor settings and the beads.
QUIET: re.Pattern[str] = re.compile(
    r"^(docs|\.beads|\.agents|\.claude|\.vscode)/|^(.*/)?[^/]*\.md$|^\.gitignore$"
)
# The paths the rules of `lane_reason` place; a change to any other runs every lane it reaches
# in full.
PLACED: re.Pattern[str] = re.compile(
    r"^(rust|ui|tools|data)/|^justfile$|^\.enact\.toml$|^rust-toolchain\.toml$"
)
# The data files that Rust crates embed or their tests and the card-text step read, and the
# toolchain's pin: these run the rust lane on the whole workspace.
RUST_DATA: re.Pattern[str] = re.compile(
    r"^rust-toolchain\.toml$"
    r"|^data/(card-store\.tsv|catalogs\.json|set-groups\.json|blunders\.json"
    r"|save-breaks\.json|ui-conventions\.json|card-text-allowlist\.json"
    r"|card-escalations\.json)$"
)
# The data files ui/src imports (and selection.py's harness, so every browser check runs).
UI_DATA: re.Pattern[str] = re.compile(
    r"^data/(arena-motion\.json|ui-conventions\.json|card-store\.tsv)$"
)
# The modules that decide whether a step's results fail its lane: its reruns, and the
# pending and slow-only checks.
JUDGES: tuple[str, ...] = ("reruns", "verdicts", "slow_only")
# The modules whose imports [`closure`] does not follow: `checks` and `cli` import every step's
# module, and this one the helpers of every lane.
LEAVES: frozenset[str] = frozenset({"checks", "cli", "slow_ci"})
# The files of the tools' environment, in every fingerprint.
ENVIRONMENT: tuple[str, ...] = ("pyproject.toml", "uv.lock")
# The packages whose change the app and counts lanes build.
APP = "beryl_app"
BENCH = "beryl_bench"
# Every lane slow CI has, in the order it starts them. `scale` runs the Python tests that read
# card files when the python lane, which includes them, does not run.
LANES: tuple[str, ...] = (
    "repo",
    "python",
    "scale",
    "rust",
    "script",
    "ui",
    "names",
    "app",
    "ui-smoke",
    "oracle",
    "counts",
)
# The lanes the browser sweep runs in full every few hours (sweep.py) instead of slow CI, which
# `by ci` still runs when named.
SWEPT: tuple[str, ...] = ("shots", "ui-test", "mobile")
ALWAYS = "always: needs no build"


# What each lane runs of the tools.


def imports(source: str) -> set[str]:
    """The sibling modules a module of beryl_tools imports, anywhere in it."""
    found: set[str] = set()
    for node in ast.walk(ast.parse(source)):
        if isinstance(node, ast.ImportFrom) and node.level == 1:
            if node.module:
                found.add(node.module.split(".")[0])
            else:
                found |= {a.name for a in node.names}
    return found


def closure(
    roots: Iterable[str], sources: Mapping[str, str], leaves: frozenset[str] = LEAVES
) -> set[str]:
    """`roots` and every module they import, directly or not, not following the imports of
    `leaves`."""
    seen = {r for r in roots if r in sources}
    todo = [r for r in seen if r not in leaves]
    while todo:
        for found in imports(sources[todo.pop()]) - seen:
            if found in sources:
                seen.add(found)
                if found not in leaves:
                    todo.append(found)
    return seen


def entry_modules(lane: checks.Lane, commands: Mapping[str, str]) -> set[str]:
    """The modules of beryl_tools that `lane`'s steps run: a `checks` step's function's, a `by`
    subcommand's (`commands`, cli.commands) or one run with `python -m`."""
    found: set[str] = set()
    named = checks.steps()
    for step in lane.steps:
        cmd = step.cmd
        if len(cmd) < 3 or cmd[1] != "-m" or not cmd[2].startswith("beryl_tools."):
            continue
        module = cmd[2].split(".")[1]
        if module == "checks" and len(cmd) > 3 and cmd[3] in named:
            found.add(named[cmd[3]].__module__.rsplit(".", 1)[-1])
        elif module == "cli" and len(cmd) > 3 and cmd[3] in commands:
            found.add(commands[cmd[3]])
        else:
            found.add(module)
    return found


def fingerprint(
    lane: checks.Lane,
    sources: Mapping[str, str],
    commands: Mapping[str, str],
    environment: str,
) -> str:
    """A hash of what the tools do in `lane`: its steps' commands, as the same in any
    checkout, the `sources` of the modules they run or import and of the [`JUDGES`], and the
    tools' `environment`."""
    repo = str(paths.REPO)
    digest = hashlib.sha256(environment.encode())
    for step in lane.steps:
        cmd = [
            "python" if a == sys.executable else a.replace(repo, ".") for a in step.cmd
        ]
        where = os.path.relpath(step.cwd, paths.REPO)
        digest.update(json.dumps([step.name, cmd, where]).encode())
    modules = closure(entry_modules(lane, commands), sources) | closure(JUDGES, sources)
    for module in sorted(modules):
        digest.update(f"\0{module}\0{sources[module]}".encode())
    return digest.hexdigest()[:16]


def fingerprints(lanes: Mapping[str, checks.Lane], tools: Path) -> dict[str, str]:
    """Each lane's [`fingerprint`] in the tools at `tools`."""
    from . import cli  # It imports every module.

    folder = tools / "src" / "beryl_tools"
    sources = {p.stem: p.read_text() for p in folder.glob("*.py")}
    others = sorted(p for p in folder.iterdir() if p.is_file() and p.suffix != ".py")
    environment = "".join(
        f"\0{p.name}\0{p.read_text(errors='replace')}"
        for p in [*(tools / name for name in ENVIRONMENT), *others]
        if p.exists()
    )
    commands = cli.commands()
    return {
        name: fingerprint(lane, sources, commands, environment)
        for name, lane in lanes.items()
    }


# Choosing the lanes.


def whole_reason(changed: list[str] | None) -> str:
    """Why a lane whose base the `changed` paths changed since (None when git cannot say)
    runs, and in full: a path the rules here do not place. Nothing when there is none.
    """
    if changed is None:
        return "git cannot say what changed since its base"
    unplaced = next(
        (p for p in changed if not QUIET.match(p) and not PLACED.match(p)), None
    )
    return f"{unplaced} changed, which no rule places" if unplaced else ""


def lane_reason(
    lane: str, changed: list[str], packages: list[str] | None, script_inputs: str
) -> str:
    """Why `lane` runs for the `changed` paths since its base, or nothing when none reaches
    it: from them, the Rust `packages` they can break (None for the whole workspace,
    `affected_packages`) and the pattern of the script lane's inputs. A change to the tools
    reaches only the python lane here; the fingerprints place it in the others."""
    if lane in ("repo", "names"):
        return ALWAYS
    loud: list[str] = [p for p in changed if not QUIET.match(p)]

    def first(*patterns: str) -> str:
        """The first changed path matching one of `patterns`, as a reason."""
        found = next((p for p in loud if any(re.match(r, p) for r in patterns)), None)
        return f"{found} changed" if found else ""

    rust: str = first(r"^rust/", RUST_DATA.pattern)

    def reaches(package: str) -> str:
        """Why the lanes that build `package` run: a Rust change it depends on."""
        return rust if packages is None or package in packages else ""

    ui = first(r"^ui/", UI_DATA.pattern)
    browser = first(r"^(rust|ui)/", UI_DATA.pattern)
    why = {
        # The tools' tests read data files and the justfile.
        "python": first(r"^tools/", r"^data/", r"^justfile$"),
        "scale": first(checks.CARD_FILES),
        "rust": rust,
        "script": first(script_inputs, checks.CARD_STORE),
        "ui": ui,
        "app": ui or reaches(APP),
        "ui-smoke": browser,
        "oracle": first(r"^rust/", oracle_selection.HARNESS.pattern),
        "counts": reaches(BENCH),
    }
    return why.get(lane, "")


# What this machine's slow CI has passed.


def ancestor(commit: str, of: str = "HEAD") -> bool:
    return run_git("merge-base", "--is-ancestor", commit, of) is not None


@dataclass
class Passes:
    """Each lane's last pass on this machine, `{"commit", "fingerprint"}`, and when the last
    full sweep began, kept in `path`."""

    path: Path
    lanes: dict[str, dict[str, str]] = field(default_factory=dict)
    full: str = ""

    @staticmethod
    def load(path: Path) -> Passes:
        try:
            found = json.loads(path.read_text())
            lanes = {
                str(k): {
                    "commit": str(v["commit"]),
                    "fingerprint": str(v["fingerprint"]),
                }
                for k, v in found.get("lanes", {}).items()
            }
            return Passes(path, lanes, str(found.get("full", "")))
        except (OSError, ValueError, AttributeError, TypeError, KeyError):
            return Passes(path)

    def save(self) -> None:
        try:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            self.path.write_text(json.dumps({"lanes": self.lanes, "full": self.full}))
        except OSError as e:
            runlog.warn(e)

    def base(self, lane: str, master: str) -> str:
        """Where `lane`'s changes count from: its last pass when HEAD descends from it and it
        is newer than `master` (master_base), else `master`."""
        last = self.lanes.get(lane, {}).get("commit", "")
        return last if last and ancestor(last) and ancestor(master, last) else master

    def stale(self, lane: str, now: str) -> str:
        """Why `lane`'s last pass cannot vouch for its tools, `now` its fingerprint; nothing
        when it can."""
        last = self.lanes.get(lane)
        if last is None:
            return "no pass on record"
        if last["fingerprint"] != now:
            return "its steps or the tools code they run changed since its last pass"
        return ""

    def full_due(self, now: datetime) -> str:
        """Why this run sweeps every lane by the clock: `$BERYL_CI_FULL`, no full sweep on
        record, or one that began over FULL_EVERY ago; nothing when none is due."""
        if os.environ.get(FULL_ENV):
            return f"${FULL_ENV} is set"
        try:
            began = datetime.fromisoformat(self.full)
        except ValueError:
            return "no full sweep on record"
        if now - began >= FULL_EVERY:
            minutes = (now - began) // timedelta(minutes=1)
            return f"the last full sweep began {minutes} min ago"
        return ""

    def record(
        self, lanes: Iterable[str], commit: str, prints: Mapping[str, str]
    ) -> None:
        """Records that `lanes` passed at `commit`, with their fingerprints `prints`, keeping
        what another run recorded meanwhile."""
        fresh = Passes.load(self.path)
        for lane in lanes:
            fresh.lanes[lane] = {"commit": commit, "fingerprint": prints[lane]}
        fresh.full = max(fresh.full, self.full)
        fresh.save()


def passes_path() -> Path:
    return paths.cache_root() / "ci" / "lane-passes.json"


def recordable() -> str:
    """The commit a pass may be recorded at: HEAD, when the working tree has no change of its
    own (a developer's `just ci` in a worktree tests their edits, not HEAD); else nothing.
    """
    clean = run_git("status", "--porcelain", "--untracked-files=no") == ""
    return checks.git("rev-parse", "HEAD") if clean else ""


# The run.


def lanes_for(packages: list[str] | None) -> dict[str, Callable[[], checks.Lane]]:
    """Each lane's constructor, the [`SWEPT`] ones too, the Rust lane held to `packages` (None:
    the workspace), each without the steps verdicts.FAST_ONLY leaves to the fast check.
    """
    made: dict[str, Callable[[], checks.Lane]] = {
        "repo": checks.repo_lane,
        "python": python_lane,
        "scale": checks.scale_lane,
        "rust": lambda: checks.rust_lane(packages=packages),
        "script": lambda: script_checks.lane(slow=True),
        "ui": checks.ui_lane,
        "names": lambda: checks.names_lane(fetch=True),
        "app": checks.app_lane,
        "ui-smoke": checks.ui_smoke_lane,
        "shots": checks.shots_lane,
        "ui-test": checks.ui_test_lane,
        "oracle": checks.oracle_lane,
        "counts": bench.lane,
        "mobile": mobile.lane,
    }
    return {name: without_fast_only(make) for name, make in made.items()}


def python_lane() -> checks.Lane:
    """The python lane with `--slow` on its pytest step, which leaves out the tests
    verdicts.FAST_ONLY leaves to the fast check (python_tests.py)."""
    lane = checks.python_lane()
    next(s for s in lane.steps if s.name == "pytest").cmd.append("--slow")
    return lane


def without_fast_only(make: Callable[[], checks.Lane]) -> Callable[[], checks.Lane]:
    """`make`'s lane without the steps verdicts.FAST_ONLY leaves to the fast check, each named
    `<lane>:<step>` as the run log names a step that logs no checks of its own (`ui:test`,
    `rust:fmt`). The fast check still runs them before every push."""

    def lane() -> checks.Lane:
        made = make()
        made.steps = [
            s for s in made.steps if f"{made.name}:{s.name}" not in verdicts.FAST_ONLY
        ]
        return made

    return lane


@dataclass
class Plan:
    """What a slow CI run runs: each lane with why and the base its changes count from, the
    lanes that run in full (`whole`), why every lane runs in full (`full`, empty when the
    selection stands) and the lanes left out, each with its base."""

    why: dict[str, str] = field(default_factory=dict)
    bases: dict[str, str] = field(default_factory=dict)
    whole: set[str] = field(default_factory=set)
    full: str = ""
    left: dict[str, str] = field(default_factory=dict)

    def packages(self) -> list[str] | None:
        """The packages the rust lane tests: those a change since its base can break, or the
        workspace."""
        if "rust" in self.whole or "rust" not in self.bases:
            return None
        changed = changed_paths(self.bases["rust"])
        return None if changed is None else rust_packages(changed)


def rust_packages(changed: list[str]) -> list[str] | None:
    """The packages a list of changes can break: None for the workspace, as when a data file
    the crates read changed."""
    if any(RUST_DATA.match(p) for p in changed):
        return None
    return affected_packages(changed)


def choose(
    lanes: Iterable[str],
    bases: Mapping[str, str],
    diffs: Mapping[str, list[str] | None],
    stale: Callable[[str], str],
    script_inputs: str,
    packages: Callable[[list[str]], list[str] | None],
) -> Plan:
    """The plan for `lanes` from each one's base (`bases`), the paths changed since each base
    (`diffs`), why a lane's last pass cannot vouch for its tools (`stale`), the pattern of the
    script lane's inputs, and the Rust packages a list of changes can break (`packages`).
    """
    chosen = Plan(bases=dict(bases))
    for lane in lanes:
        if lane == "scale" and "python" in chosen.why:
            continue  # The python lane runs the scale lane's tests.
        changed = diffs[bases[lane]]
        reason = whole_reason(changed) or stale(lane)
        if reason:
            chosen.whole.add(lane)
        else:
            found = changed or []
            reason = lane_reason(lane, found, packages(found), script_inputs)
        if reason:
            chosen.why[lane] = reason
        elif lane != "scale":
            chosen.left[lane] = bases[lane]
    return chosen


def plan(
    passes: Passes, now: datetime, lanes: list[str], prints: Mapping[str, str]
) -> Plan:
    """The lanes of `lanes` to run, against what changed since each one's base, with their
    fingerprints `prints`."""
    master = master_base()
    bases = {lane: passes.base(lane, master) for lane in lanes}
    full = passes.full_due(now) or (
        "" if master else "no merge base with origin/master"
    )
    if full:
        return Plan({lane: full for lane in lanes}, bases, set(lanes), full)
    diffs = {base: changed_paths(base) for base in set(bases.values())}
    return choose(
        lanes,
        bases,
        diffs,
        lambda lane: passes.stale(lane, prints[lane]),
        script_checks.inputs(),
        rust_packages,
    )


def skip_records(left: Mapping[str, str], commit: str) -> list[dict[str, object]]:
    """A run-log `skip` record for each lane left out, saying since which commit no change
    reaches it."""
    now = datetime.now(timezone.utc).isoformat(timespec="seconds")
    records: list[dict[str, object]] = []
    for lane, base in left.items():
        record: dict[str, object] = {
            "time": now,
            "host": platform.node(),
            "commit": commit,
            "script": "ci",
            "lane": lane,
            "check": f"{lane}:lane",
            "outcome": "skip",
            "seconds": 0.0,
            "chosen": f"cut: no change since {base[:10]} reaches it",
        }
        records.append(record)
    return records


def report(chosen: Plan, passes: Passes, now: datetime, commit: str) -> None:
    """Prints why each lane runs or is left out, logs a `skip` for each one left out, and
    notes a full sweep's start in `passes`."""
    if chosen.full:
        print(f"ci: every lane: {chosen.full}")
        if commit:
            passes.full = now.isoformat()
        return
    for lane, reason in chosen.why.items():
        scope = "in full" if lane in chosen.whole else "what it can break"
        print(f"ci: {lane}, {scope} since {chosen.bases[lane][:10]}: {reason}")
    if chosen.left:
        print(
            "ci: left out, as nothing since their bases reaches them:"
            f" {' '.join(chosen.left)}"
        )
        try:
            runlog.append(skip_records(chosen.left, checks.git("rev-parse", "HEAD")))
        except Exception as e:  # Logging never fails a check.
            runlog.warn(e)


def slow(args: argparse.Namespace) -> int:
    measuring = speed.start_background()
    if measuring:
        print(f"ci: {measuring}")
    started = time.monotonic()
    now = datetime.now(timezone.utc)
    if args.lanes:
        unknown = set(args.lanes) - set(LANES) - set(SWEPT)
        if unknown:
            raise SystemExit(
                f"unknown lanes {sorted(unknown)}; lanes: {' '.join(LANES + SWEPT)}"
            )
        names = list(args.lanes)
    else:
        names = [n for n in LANES if not (system.WINDOWS and n in checks.WINDOWS_SKIPS)]
        if system.WINDOWS:
            print(
                f"ci: skipping {' '.join(checks.WINDOWS_SKIPS)} on Windows; the Mac's"
                " slow CI runs them"
            )
    canonical = {name: make() for name, make in lanes_for(None).items()}
    prints: dict[str, str] = fingerprints(canonical, paths.TOOLS)
    passes: Passes = Passes.load(passes_path())
    commit: str = recordable()
    if args.lanes:
        chosen: Plan = Plan({n: "named" for n in names}, {}, set(names), "named")
    else:
        chosen = plan(passes, now, names, prints)
        report(chosen, passes, now, commit)
    constructors = lanes_for(chosen.packages())
    # A lane whose every step verdicts.FAST_ONLY leaves to the fast check (`scale`, `script`)
    # has nothing to run.
    lanes = [lane for name in chosen.why if (lane := constructors[name]()).steps]

    def passed(lanes: list[str]) -> None:
        if commit:
            passes.record(lanes, commit, prints)

    budget = int(os.environ.get("CI_BUDGET_S", checks.SLOW_BUDGET))
    every = len(chosen.whole) == len(chosen.why)
    code = checks.slow_run(
        lanes, budget, started, "all" if every else "affected", passed
    )
    if not args.lanes and commit:
        try:
            print(f"ci: {sweep.start_background()}")
        except Exception as e:  # Starting the sweep never fails slow CI.
            runlog.warn(e)
    return code
