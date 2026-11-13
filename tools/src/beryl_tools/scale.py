"""The scale rehearsal (docs/project-50k.md, Acceptance, "Scale rehearsal"): whether the compiler
copes with 50,000 implemented cards.

`scale` writes a scratch workspace under `$BERYL_CACHE/scale` (never in the tree) that builds
what the tree builds: set crates written and built as `rust/cards/*` are, by the same build
helper, a registry built as `beryl_cards` is, and a scratch card store, with N clones of the real
card files in place of the real cards (scale_workspace.py says how). It builds the workspace and
prints what that cost against the plan's budgets ([`BUDGETS`]):

- cold check: `cargo check` of the registry and every set crate with their scenarios, as tests
  and tools build them, after removing the scratch crates' artifacts; the checkout's own crates
  are built beforehand, so only the cards' crates, their build scripts and the registry count.
- release build: `cargo build --release` of the registry's binary, which reads every card, so
  the registry and every set crate are compiled and linked as the app's release build would
  (without scenarios, which the app never compiles).
- one-edit rebuild: the same `cargo check` after a change to one card file, which reruns that
  crate's build script and rechecks it and the registry; the median of
  `--edits` edits ([`REBUILDS`] by default), with each edit's time printed as the spread.
- largest compiler process: the highest peak resident set of any single process of the three
  builds (`/usr/bin/time -l`), which is a `rustc`.

Seconds are wall-clock seconds on this machine with all its cores, load-adjusted as the checks'
clocks are (`clock.Clock`: wall time over the load beyond the cores and the speed factor). The
CPU-seconds beside each are what scales with machines. With `--assert`, a measure over its
budget makes the run exit non-zero. A budget missed after three recorded attempts holds that
measure to its recorded value plus 10% instead, naming the follow-up bead that owns it.
`--assert-memory` asserts only the largest compiler process, which the machine's load does not
move.

Every run is made under the machine's normal load, a load average of 20 to 40 on 18 cores
(docs/benchmarking.md, "Timing under load"); there is no quiet run to wait for. The clock
removes the load beyond the cores, but not the slowdown from sharing caches and memory below
that, so the times still move with load: four cold checks took 41 to 56 s against the 60 s
budget, and one-edit medians ranged from 2.2 to 3.2 s against 2 s (docs/project-50k.md, "Scale
rehearsal"). So:

- `--assert` is how a budget is judged. Run it under normal load with `--edits 8` or more, and
  read each time with its load and CPU-seconds. When a change is meant to move a time, compare
  it with the change's base in alternating runs (A B B A) on two workspaces rather than against
  the budget alone.
- The nightly (nightly.py) passes `--assert-memory`, not `--assert`. A nightly failure files a
  Steward bead, so it asserts only the measure whose night-to-night spread is far below its
  margin. The times are printed in its log, where their trend shows.

The rehearsal holds the machine for minutes, so it runs in the nightly, never in a check. It
then prints what scripts cost (plan K24): the seconds
`beryl_cli script-check` takes to play every spec moment of each pooled card from both seats,
per card and extrapolated to the N clones, reported with no bar.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import subprocess
import sys
import tempfile
import time
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path

from . import card_store, clock, nearest, paths, script_checks
from .scale_workspace import (
    BINARY,
    REGISTRY,
    WARM,
    Source,
    edit_card,
    generate,
    plan,
    sources,
    tree_groups,
)

DEFAULT_CARDS = 50_000
# The one-edit rebuild is a few seconds of mostly serial work, which the machine's other load
# moves by more than its budget's margin: its measure is the median of this many edits unless
# `--edits` asks for more.
REBUILDS = 3
GIB: int = 2**30
# A card's script check in the run log: `script:<card>/<moment>/<seat>`.
SCRIPT_CHECK: re.Pattern[str] = re.compile(r"^script:([^/]+)/[^/]+/[^/]+$")
# The cards the script cost names by name, the costliest first.
COSTLIEST = 5
# `script-check`'s last line: its checks' own seconds, past the process's start.
SCRIPT_SUMMARY: re.Pattern[str] = re.compile(
    r"^script-check: .* in ([\d.]+) s$", re.MULTILINE
)


@dataclass(frozen=True)
class Budget:
    """A measure's bar: the plan's value, in seconds or bytes. `held` names the follow-up bead
    that owns a budget missed after three attempts, and `limit` is then the recorded value
    plus 10%, which `--assert` holds the measure to meanwhile."""

    plan: float
    limit: float
    unit: str
    held: str | None = None


# The plan's four budgets (docs/project-50k.md, Acceptance, "Scale rehearsal").
BUDGETS: dict[str, Budget] = {
    "cold check": Budget(60, 60, "s"),
    "release build": Budget(180, 180, "s"),
    "one-edit rebuild": Budget(2, 2, "s"),
    "largest compiler process": Budget(4 * GIB, 4 * GIB, "B"),
}


@dataclass
class Run:
    """One timed build: its load-adjusted and wall seconds, its CPU-seconds (user and system,
    every process it waited for), its largest process's peak resident set in bytes and the
    highest load average while it ran."""

    seconds: float
    wall: float
    cpu: float
    rss: int
    load: float


def parse_time(output: str) -> tuple[float, float, int]:
    """Wall seconds, CPU-seconds and the maximum resident set size in bytes from
    `/usr/bin/time -l`'s report."""
    found = re.search(
        r"^\s*([\d.]+)\s+real\s+([\d.]+)\s+user\s+([\d.]+)\s+sys", output, re.M
    )
    rss = re.search(r"^\s*(\d+)\s+maximum resident set size", output, re.MULTILINE)
    if found is None or rss is None:
        raise RuntimeError("no /usr/bin/time -l report in:\n" + output[-2000:])
    wall, user, system = (float(found.group(i)) for i in (1, 2, 3))
    return wall, user + system, int(rss.group(1))


def cargo_env(root: Path) -> dict[str, str]:
    env = {**os.environ, "CARGO_TARGET_DIR": str(root / "target")}
    env.pop("CARGO_BUILD_TARGET_DIR", None)
    return env


def timed(cmd: list[str], root: Path) -> Run:
    """Runs `cmd` in the workspace's `rust` directory under `/usr/bin/time -l`, on the
    load-adjusted clock; a failure ends the run with the compiler's output."""
    timer = clock.Clock()
    run = subprocess.run(
        ["/usr/bin/time", "-l", *cmd],
        cwd=root / "rust",
        env=cargo_env(root),
        capture_output=True,
        text=True,
        check=False,
    )
    adjusted = timer.now()
    if run.returncode != 0:
        sys.exit(f"scale: {' '.join(cmd)} failed:\n{run.stderr[-6000:]}")
    wall, cpu, rss = parse_time(run.stderr)
    return Run(adjusted, wall, cpu, rss, timer.peak)


def median(runs: list[Run]) -> Run:
    """The run of median load-adjusted seconds."""
    return sorted(runs, key=lambda run: run.seconds)[len(runs) // 2]


def measures(runs: dict[str, Run]) -> dict[str, float]:
    """Each budget's measure from the three builds."""
    return {
        "cold check": runs["cold check"].seconds,
        "release build": runs["release build"].seconds,
        "one-edit rebuild": runs["one-edit rebuild"].seconds,
        "largest compiler process": float(max(r.rss for r in runs.values())),
    }


def shown(value: float, unit: str) -> str:
    return f"{value / GIB:.2f} GiB" if unit == "B" else f"{value:.1f} s"


def missed(values: dict[str, float], times: bool = True) -> list[str]:
    """The measures over their limits; with `times` false, only those in bytes."""
    return [
        name
        for name, value in values.items()
        if value > BUDGETS[name].limit and (times or BUDGETS[name].unit == "B")
    ]


def report(
    cards: int, crates: int, chosen: bool, pool: int, runs: dict[str, Run]
) -> str:
    """Each measure with its budget and margin, and each build's CPU-seconds and load."""
    how = "as --crates gave" if chosen else "as the tree's grouping gives"
    lines = [
        f"{cards:,} cards: clones of {pool} real card files, in {crates} set crates "
        f"({how})",
        "",
    ]
    for (name, value), run in zip(measures(runs).items(), [*runs.values(), None]):
        budget = BUDGETS[name]
        margin = budget.limit - value
        verdict = "ok" if margin >= 0 else "MISSED"
        held = f", held to {shown(budget.limit, budget.unit)} ({budget.held})" * bool(
            budget.held
        )
        line = (
            f"{name:<26}{shown(value, budget.unit):>12}   budget "
            f"{shown(budget.plan, budget.unit)}{held}, margin "
            f"{shown(margin, budget.unit)}  {verdict}"
        )
        if run is not None:
            line += (
                f"\n{'':<26}{run.cpu:>10.0f} s CPU, {run.wall:.1f} s on the clock, "
                f"peak load {run.load:.0f}, peak process {run.rss / GIB:.2f} GiB"
            )
        lines.append(line)
    lines.append(clock.load_note())
    return "\n".join(lines)


def store_note(path: Path) -> str:
    """The size and card count of the card store at `path`."""
    with card_store.Store(path) as store:
        count = len(store.names("card"))
    return f"{path.stat().st_size / 2**20:.1f} MiB, {count:,} card keys"


@dataclass
class Scripts:
    """What a script-check run of the pool's cards cost: each card's checks' seconds summed, by
    slug, how many checks there were, the run's seconds and the seconds its checks took, past
    the process's start, by its own clock."""

    cards: dict[str, float]
    checks: int
    seconds: float  # load-adjusted
    wall: float
    checking: float


def card_seconds(lines: Iterable[str]) -> tuple[dict[str, float], int]:
    """Each card's script checks' seconds summed, by its slug, from `script-check`'s run-log
    lines, and how many checks there were; fixtures' recordings are no card's."""
    cards: dict[str, float] = {}
    count = 0
    for line in lines:
        try:
            record = json.loads(line)
            check, seconds = str(record["check"]), float(record["seconds"])
        except (json.JSONDecodeError, KeyError, TypeError, ValueError):
            continue
        found = SCRIPT_CHECK.match(check)
        if found is None or found.group(1) == "fixtures":
            continue
        cards[found.group(1)] = cards.get(found.group(1), 0.0) + seconds
        count += 1
    return cards, count


def clone_seconds(cards: dict[str, float], pool: list[Source], count: int) -> float:
    """The script checks' seconds of `count` clones: clone `i` is `pool[i % len(pool)]`, as
    [`generate`] picks them, and costs what its original does."""
    each = [cards.get(nearest.slug(s.name), 0.0) for s in pool]
    rounds, rest = divmod(count, len(each))
    return rounds * math.fsum(each) + math.fsum(each[:rest])


def measure_scripts(pool: list[Source]) -> Scripts:
    """Runs the script checks of the pool's cards, every spec moment from both seats, and reads
    what each cost from the results they log."""
    if not script_checks.build():
        sys.exit("scale: beryl_cli did not build for the script checks")
    slugs = sorted({nearest.slug(s.name) for s in pool})
    with tempfile.TemporaryDirectory() as tmp:
        results, printed = Path(tmp) / "results.jsonl", Path(tmp) / "out.txt"
        timer = clock.Clock()
        began = time.monotonic()
        with open(printed, "w") as out:
            code = script_checks.run(slugs, results, out=out)
        wall, adjusted = time.monotonic() - began, timer.now()
        output = printed.read_text()
        print(output, end="", file=sys.stderr)
        if code != 0:
            sys.exit(
                "scale: a script check failed: `just test-rust`, or the script lane"
            )
        lines = results.read_text().splitlines() if results.exists() else []
    summary = SCRIPT_SUMMARY.search(output)
    cards, count = card_seconds(lines)
    checking = float(summary.group(1)) if summary else wall
    return Scripts(cards, count, adjusted, wall, checking)


def script_report(scripts: Scripts, pool: list[Source], cards: int) -> str:
    """The script cost, per card and for the `cards` clones, at the run's own parallelism."""
    measured = [nearest.slug(s.name) for s in pool]
    spent = sum(scripts.cards.values())
    names = sorted(set(measured))
    mean = sum(scripts.cards.get(n, 0.0) for n in names) / len(names)
    costliest = sorted(scripts.cards.items(), key=lambda kv: -kv[1])[:COSTLIEST]
    clones = clone_seconds(scripts.cards, pool, cards)
    # The run's checks overlap on its threads: the clones' would as much.
    parallel = max(spent / scripts.checking, 1.0) if scripts.checking > 0 else 1.0
    run = clock.took(scripts.wall, scripts.seconds)
    lines = [
        "",
        f"{'script checks':<26}{run:>34}   {scripts.checks:,} checks: the spec moments "
        f"of {len(names)} cards from both seats, every script built and replayed",
        f"{'  per card':<26}{mean:>32.3f} s   of checks on average; the costliest: "
        + ", ".join(f"{name} {seconds:.2f} s" for name, seconds in costliest),
        f"{f'  {cards:,} clones':<26}{clones:>32,.0f} s   of checks, about "
        f"{clones / parallel:,.0f} s at this run's parallelism ({parallel:.1f}x)",
    ]
    return "\n".join(lines)


def scale(args: argparse.Namespace) -> int:
    pool, left = sources()
    for path, why in left.items():
        print(f"scale: left out {path.relative_to(paths.REPO)}: {why}", file=sys.stderr)
    root = paths.cache_root() / "scale"
    crates = plan(args.cards, args.crates, tree_groups())
    print(
        f"scale: writing {args.cards:,} clones in {len(crates)} crates to {root}",
        file=sys.stderr,
    )
    names = generate(root, crates, pool)
    print(f"scale: scratch store {store_note(root / 'data' / 'card-store.tsv')}")
    print(f"scale: the tree's store {store_note(card_store.STORE)}")
    # The scratch crates' artifacts and incremental state from an earlier run would make a
    # cold build warm.
    subprocess.run(
        ["cargo", "clean", *[f"--package={n}" for n in [REGISTRY, *names]]],
        cwd=root / "rust",
        env=cargo_env(root),
        capture_output=True,
        check=False,
    )
    print("scale: building the checkout's crates the cards use", file=sys.stderr)
    timed(["cargo", "check", f"--package={WARM}"], root)
    timed(["cargo", "build", "--release", f"--package={WARM}"], root)
    check = ["cargo", "check", f"--package={REGISTRY}", "--features=scenarios"]
    runs: dict[str, Run] = {}
    print("scale: cold check", file=sys.stderr)
    runs["cold check"] = timed(check, root)
    print("scale: release build", file=sys.stderr)
    release = ["cargo", "build", "--release", f"--package={REGISTRY}"]
    runs["release build"] = timed([*release, f"--bin={BINARY}"], root)
    print("scale: one-edit rebuild", file=sys.stderr)
    rebuilds = []
    for _ in range(args.edits):
        edit_card(root, names)
        rebuilds.append(timed(check, root))
    runs["one-edit rebuild"] = median(rebuilds)
    print(report(args.cards, len(crates), args.crates is not None, len(pool), runs))
    each = ", ".join(f"{run.seconds:.1f} s" for run in rebuilds)
    spread = sorted(run.seconds for run in rebuilds)
    print(
        f"{'':<26}the {args.edits} edits' rebuilds, in order: {each}; "
        f"range {spread[0]:.1f} to {spread[-1]:.1f} s"
    )
    if not args.no_scripts:
        print("scale: script checks", file=sys.stderr)
        print(script_report(measure_scripts(pool), pool, args.cards))
    over = missed(measures(runs), times=args.assert_)
    if (args.assert_ or args.assert_memory) and over:
        print(f"scale: over budget: {', '.join(over)}", file=sys.stderr)
        return 1
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "scale",
        help="build cloned cards at scale and time the compiler (docs/project-50k.md)",
    )
    p.add_argument(
        "--cards", type=int, default=DEFAULT_CARDS, help="definitions to build"
    )
    p.add_argument(
        "--crates",
        type=int,
        default=None,
        help="set crates holding them (default: as the tree's grouping gives)",
    )
    p.add_argument(
        "--edits",
        type=lambda text: max(1, int(text)),
        default=REBUILDS,
        help=f"one-card edits whose median rebuild is measured (default {REBUILDS})",
    )
    p.add_argument(
        "--assert",
        dest="assert_",
        action="store_true",
        help="exit non-zero when a measure misses its budget",
    )
    p.add_argument(
        "--assert-memory",
        action="store_true",
        help="exit non-zero only when the largest compiler process misses its budget",
    )
    p.add_argument(
        "--no-scripts",
        action="store_true",
        help="skip measuring what the pooled cards' script checks cost",
    )
    p.set_defaults(func=scale)


if __name__ == "__main__":
    sys.exit(f"run it as `by scale`, not {os.path.basename(sys.argv[0])}")
