"""The scale rehearsal: whether the compiler
copes with 50,000 implemented cards.

`scale` writes a scratch workspace under `$BERYL_CACHE/scale` (never in the tree) that builds
what the tree builds: set crates written and built as `engine/cards/*` are, by the same build
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
- largest compiler process: the highest peak memory of any single process of the three builds,
  which is a `rustc`. macOS uses resident set size from `/usr/bin/time -l`. Windows uses the
  job object's peak committed memory per process.
- build directory size: the logical bytes in Cargo's `target` directory after the release
  build and again after all the one-card edits. These measurements have no budget. The build
  report is also saved to `$BERYL_CACHE/scale/build-report.txt`.

Seconds are wall-clock seconds on this machine with all its cores. The CPU-seconds beside each
are what scales with machines. With `--assert`, a measure over its budget makes the run exit
non-zero. A budget missed after three recorded attempts holds that measure to its recorded value
plus 10% instead, naming the follow-up ticket that owns it. `--assert-memory` asserts only the
largest compiler process, which the machine's load does not move.

The times move with the machine's other load, because of the slowdown from sharing cores,
caches and memory (docs/benchmarking.md, "Timing under load"): four cold checks took 41 to 56 s
against the 60 s budget, and one-edit medians ranged from 2.2 to 3.2 s against 2 s. So:

- `--assert` is how a budget is judged. Run it with `--edits 8` or more, and read each time with
  its CPU-seconds. When a change is meant to move a time, compare it with the change's base in
  alternating runs (A B B A) on two workspaces rather than against the budget alone.
- `--assert-memory` is the assertion for an unattended run, because the largest compiler
  process's size is far less variable than the times are.

The rehearsal holds the machine for minutes. It then prints what scripts cost: the seconds
`beryl_cli script-check` takes to play every spec moment of each pooled card from both seats,
per card and extrapolated to the N clones, reported with no bar.
"""

from __future__ import annotations

import argparse
import ctypes
import json
import math
import os
import re
import subprocess
import sys
import tempfile
import time
from collections.abc import Iterable
from contextlib import ExitStack
from ctypes import wintypes
from dataclasses import dataclass
from pathlib import Path
from typing import IO

from . import card_store, nearest, paths, system
from .client.paths import FIXTURE_DEFINITIONS
from .scale_workspace import (
    BINARY,
    REGISTRY,
    WARM,
    Source,
    directory_bytes,
    edit_card,
    generate,
    original,
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
# A card's script check in `script-check`'s results: `script:<card>/<moment>/<seat>`.
SCRIPT_CHECK: re.Pattern[str] = re.compile(r"^script:([^/]+)/[^/]+/[^/]+$")
# The cards the script cost names by name, the costliest first.
COSTLIEST = 5
# `script-check`'s last line: its checks' own seconds, past the process's start.
SCRIPT_SUMMARY: re.Pattern[str] = re.compile(
    r"^script-check: .* in ([\d.]+) s$", re.MULTILINE
)


@dataclass(frozen=True)
class Budget:
    """A measure's bar: the plan's value, in seconds or bytes. `held` names the follow-up ticket
    that owns a budget missed after three attempts, and `limit` is then the recorded value
    plus 10%, which `--assert` holds the measure to meanwhile."""

    plan: float
    limit: float
    unit: str
    held: str | None = None


# The plan's four budgets. The cold check's limit is the wall time of the run that set it, 127.5 s
# with a load average of 13 to 17 on 10 cores (295 CPU-seconds), plus 10%: the plan's 60 s was
# met on an idle machine, which that run was not.
BUDGETS: dict[str, Budget] = {
    "cold check": Budget(60, 140, "s", "measured under load"),
    "release build": Budget(180, 180, "s"),
    "one-edit rebuild": Budget(2, 2, "s"),
    "largest compiler process": Budget(4 * GIB, 4 * GIB, "B"),
}


@dataclass
class Run:
    """One timed build: its wall seconds, its CPU-seconds (user and system, every process it
    waited for) and its largest process's peak memory in bytes."""

    seconds: float
    cpu: float
    peak_memory: int


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
    """Runs a timed build in the scratch workspace and shows compiler diagnostics on failure."""
    cwd, env = root / "engine", cargo_env(root)
    try:
        if system.WINDOWS:
            return timed_windows(cmd, cwd, env)
        run = subprocess.run(
            ["/usr/bin/time", "-l", *cmd],
            cwd=cwd,
            env=env,
            capture_output=True,
            text=True,
            check=True,
        )
    except subprocess.CalledProcessError as error:
        sys.exit(f"scale: {' '.join(cmd)} failed:\n{error.stderr[-6000:]}")
    return Run(*parse_time(run.stderr))


class JobAccounting(ctypes.Structure):
    """JOBOBJECT_BASIC_ACCOUNTING_INFORMATION."""

    _fields_ = [
        ("user_ticks", wintypes.LARGE_INTEGER),
        ("kernel_ticks", wintypes.LARGE_INTEGER),
        ("period_user_ticks", wintypes.LARGE_INTEGER),
        ("period_kernel_ticks", wintypes.LARGE_INTEGER),
        ("page_faults", wintypes.DWORD),
        ("processes", wintypes.DWORD),
        ("active_processes", wintypes.DWORD),
        ("terminated_processes", wintypes.DWORD),
    ]


class JobBasicLimits(ctypes.Structure):
    """JOBOBJECT_BASIC_LIMIT_INFORMATION."""

    _fields_ = [
        ("process_user_ticks", wintypes.LARGE_INTEGER),
        ("job_user_ticks", wintypes.LARGE_INTEGER),
        ("flags", wintypes.DWORD),
        ("minimum_working_set", ctypes.c_size_t),
        ("maximum_working_set", ctypes.c_size_t),
        ("active_process_limit", wintypes.DWORD),
        ("affinity", ctypes.c_size_t),
        ("priority_class", wintypes.DWORD),
        ("scheduling_class", wintypes.DWORD),
    ]


class JobExtendedLimits(ctypes.Structure):
    """JOBOBJECT_EXTENDED_LIMIT_INFORMATION, including the six IO_COUNTERS."""

    _fields_: list[
        tuple[
            str,
            type[JobBasicLimits | ctypes.Array[ctypes.c_ulonglong] | ctypes.c_size_t],
        ]
    ] = [
        ("basic", JobBasicLimits),
        ("io_counters", ctypes.c_ulonglong * 6),
        ("process_memory_limit", ctypes.c_size_t),
        ("job_memory_limit", ctypes.c_size_t),
        ("peak_process_memory", ctypes.c_size_t),
        ("peak_job_memory", ctypes.c_size_t),
    ]


def timed_windows(cmd: list[str], cwd: Path, env: dict[str, str]) -> Run:
    """Measures Cargo and its descendants with a Win32 Job Object."""
    import _winapi
    import msvcrt

    # Pyre checks from macOS or Linux, where these Windows APIs are absent.
    # pyre-ignore[16]: Windows only
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    close_handle = _winapi.CloseHandle  # pyre-ignore[16]: Windows only
    create_process = _winapi.CreateProcess  # pyre-ignore[16]: Windows only
    create = kernel.CreateJobObjectW
    create.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
    create.restype = wintypes.HANDLE
    assign = kernel.AssignProcessToJobObject
    assign.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
    resume = kernel.ResumeThread
    resume.argtypes = [wintypes.HANDLE]
    resume.restype = wintypes.DWORD
    query = kernel.QueryInformationJobObject
    job_info_args = [
        wintypes.HANDLE,
        ctypes.c_int,
        ctypes.c_void_p,
        wintypes.DWORD,
    ]
    query.argtypes = [*job_info_args, ctypes.c_void_p]
    set_info = kernel.SetInformationJobObject
    set_info.argtypes = job_info_args
    accounting_class, extended_limits_class = 1, 9
    kill_on_close, create_suspended = 0x2000, 0x0004

    with ExitStack() as handles:
        job = create(None, None)
        if not job:
            raise ctypes.WinError()  # pyre-ignore[16]: Windows only
        handles.callback(close_handle, job)
        limits = JobExtendedLimits()
        limits.basic.flags = kill_on_close
        if not set_info(
            job, extended_limits_class, ctypes.byref(limits), ctypes.sizeof(limits)
        ):
            raise ctypes.WinError()  # pyre-ignore[16]: Windows only

        # A file lets Cargo write diagnostics while the parent waits, even if they exceed a
        # pipe buffer. Inherit only the handles used for the child's standard streams.
        with (
            open(os.devnull, "r+b") as discard,
            tempfile.TemporaryFile(mode="w+t", errors="replace") as stderr,
        ):
            get_os_handle = msvcrt.get_osfhandle  # pyre-ignore[16]: Windows only
            null_handle = get_os_handle(discard.fileno())
            err_handle = get_os_handle(stderr.fileno())
            for handle in (null_handle, err_handle):
                os.set_handle_inheritable(handle, True)  # pyre-ignore[16]: Windows only
            # pyre-ignore[16]: Windows only
            use_std_handles = subprocess.STARTF_USESTDHANDLES
            startup = subprocess.STARTUPINFO(  # pyre-ignore[16]: Windows only
                dwFlags=use_std_handles,
                hStdInput=null_handle,
                hStdOutput=null_handle,
                hStdError=err_handle,
                lpAttributeList={"handle_list": [null_handle, err_handle]},
            )
            started = time.perf_counter()
            # Keep the primary thread handle so Cargo can enter the job before it runs rustc.
            process, thread, _, _ = create_process(
                None,
                subprocess.list2cmdline(cmd),
                None,
                None,
                True,
                create_suspended,
                env,
                str(cwd),
                startup,
            )
            handles.callback(close_handle, process)
            handles.callback(close_handle, thread)
            if not assign(job, process) or resume(thread) == 0xFFFFFFFF:
                error = ctypes.WinError()  # pyre-ignore[16]: Windows only
                _winapi.TerminateProcess(process, 1)  # pyre-ignore[16]: Windows only
                raise error

            # pyre-ignore[16]: Windows only
            _winapi.WaitForSingleObject(process, _winapi.INFINITE)
            wall = time.perf_counter() - started
            code = _winapi.GetExitCodeProcess(process)  # pyre-ignore[16]: Windows only
            if code:
                stderr.seek(0)
                raise subprocess.CalledProcessError(code, cmd, stderr=stderr.read())

        accounting = JobAccounting()
        for info_class, info in (
            (accounting_class, accounting),
            (extended_limits_class, limits),
        ):
            if not query(
                job, info_class, ctypes.byref(info), ctypes.sizeof(info), None
            ):
                raise ctypes.WinError()  # pyre-ignore[16]: Windows only
        cpu = (accounting.user_ticks + accounting.kernel_ticks) / 10_000_000
        return Run(wall, cpu, limits.peak_process_memory)


def median(runs: list[Run]) -> Run:
    """The run of median seconds."""
    return sorted(runs, key=lambda run: run.seconds)[len(runs) // 2]


def measures(runs: dict[str, Run]) -> dict[str, float]:
    """Each budget's measure from the three builds."""
    return {
        "cold check": runs["cold check"].seconds,
        "release build": runs["release build"].seconds,
        "one-edit rebuild": runs["one-edit rebuild"].seconds,
        "largest compiler process": float(max(r.peak_memory for r in runs.values())),
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
    cards: int,
    crates: int,
    chosen: bool,
    pool: int,
    runs: dict[str, Run],
    built_bytes: int,
    rebuilt_bytes: int,
) -> str:
    """Build timings, budget margins, compiler memory, and build directory sizes."""
    how = "as --crates gave" if chosen else "as the tree's grouping gives"
    memory_kind = " committed" if system.WINDOWS else ""
    lines = [
        f"{cards:,} cards: clones of {pool} real card files, three in four of them with no "
        f"scenarios, in {crates} set crates "
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
                f"\n{'':<26}{run.cpu:>10.0f} s CPU, "
                f"peak process {run.peak_memory / GIB:.2f} GiB{memory_kind}"
            )
        lines.append(line)
    for name, size in (
        ("target after build", built_bytes),
        ("target after edits", rebuilt_bytes),
    ):
        lines.append(f"{name:<26}{size:>10,} B")
    one, five, fifteen = system.loadavg()
    lines.append(
        f"load average {one:.0f} {five:.0f} {fifteen:.0f} on {os.cpu_count()} cores"
    )
    return "\n".join(lines)


def store_note(path: Path) -> str:
    """The size and card count of the card store at `path`."""
    with card_store.Store(path) as store:
        count = len(store.names("card"))
    return f"{path.stat().st_size / 2**20:.1f} MiB, {count:,} card keys"


@dataclass
class Scripts:
    """What a script-check run of the pool's cards cost: each card's checks' seconds summed, by
    slug, how many checks there were, the run's wall seconds and the seconds its checks took,
    past the process's start, by the checker's own clock."""

    cards: dict[str, float]
    checks: int
    wall: float
    checking: float


def card_seconds(lines: Iterable[str]) -> tuple[dict[str, float], int]:
    """Each card's script checks' seconds summed, by its slug, from `script-check`'s result
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
    [`generate`] picks them (`original`), and costs what its original does."""
    return math.fsum(
        cards.get(nearest.slug(original(pool, i).name), 0.0) for i in range(count)
    )


def build_checker() -> bool:
    """Builds `beryl_cli` with debug assertions, where the script checks are assertions, into
    `paths.RUN_TARGET`; whether it built."""
    built = subprocess.run(
        paths.CLI_BUILD,
        cwd=paths.ENGINE,
        env=paths.run_env(),
        stdin=subprocess.DEVNULL,
        check=False,
    )
    return built.returncode == 0


def run_checker(only: list[str], results: Path, out: IO[str]) -> int:
    """Runs the built checker's `script-check` on the checks `only` names (a card's slug), which
    writes one line per check, with its seconds, to `results`; returns its exit code."""
    binary = system.exe(paths.RUN_TARGET / "debug" / "beryl_cli")
    definitions = paths.REPO / FIXTURE_DEFINITIONS
    return subprocess.run(
        [str(binary), "script-check", str(definitions), *only],
        cwd=paths.ENGINE,
        env={**os.environ, "BERYL_CHECK_RESULTS": str(results)},
        stdin=subprocess.DEVNULL,
        stdout=out,
        check=False,
    ).returncode


def measure_scripts(pool: list[Source]) -> Scripts:
    """Runs the script checks of the pool's cards, every spec moment from both seats, and reads
    what each cost from the results they log."""
    if not build_checker():
        sys.exit("scale: beryl_cli did not build for the script checks")
    slugs = sorted({nearest.slug(s.name) for s in pool})
    with tempfile.TemporaryDirectory() as tmp:
        results, printed = Path(tmp) / "results.jsonl", Path(tmp) / "out.txt"
        began = time.monotonic()
        with open(printed, "w") as out:
            code = run_checker(slugs, results, out)
        wall = time.monotonic() - began
        output = printed.read_text()
        print(output, end="", file=sys.stderr)
        if code != 0:
            sys.exit("scale: a script check failed: see `just test-rust`")
        lines = results.read_text().splitlines() if results.exists() else []
    summary = SCRIPT_SUMMARY.search(output)
    cards, count = card_seconds(lines)
    checking = float(summary.group(1)) if summary else wall
    return Scripts(cards, count, wall, checking)


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
    run = f"{scripts.wall:.1f} s"
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
        cwd=root / "engine",
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
    target = root / "target"
    built_bytes = directory_bytes(target)
    print("scale: one-edit rebuild", file=sys.stderr)
    rebuilds = []
    for _ in range(args.edits):
        edit_card(root, names)
        rebuilds.append(timed(check, root))
    runs["one-edit rebuild"] = median(rebuilds)
    rebuilt_bytes = directory_bytes(target)
    summary = report(
        args.cards,
        len(crates),
        args.crates is not None,
        len(pool),
        runs,
        built_bytes,
        rebuilt_bytes,
    )
    each = ", ".join(f"{run.seconds:.1f} s" for run in rebuilds)
    spread = sorted(run.seconds for run in rebuilds)
    summary += (
        f"\n{'':<26}the {args.edits} edits' rebuilds, in order: {each}; "
        f"range {spread[0]:.1f} to {spread[-1]:.1f} s"
    )
    print(summary)
    (root / "build-report.txt").write_text(summary + "\n", encoding="utf-8")
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
        help="build cloned cards at scale and time the compiler",
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
