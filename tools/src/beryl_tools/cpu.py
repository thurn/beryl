"""Where this machine's CPU goes, by session and by kind of work.

`by cpu sample` samples every process's cumulative CPU time every few seconds and appends one
JSON row per minute to `$BERYL_CACHE/cpu/samples.jsonl` ([`samples_file`]). `by cpu report`
sums those rows. The run log (`runlog.py`) only times the checks; this also sees builds, the
commands agents run by hand, editors, the desktop apps and leaked processes.

Each process is given a context (who asked for the work), a kind (what the work is) and a
worktree under `.worktrees/` ([`classify`]). The context comes from the process's own command
line, its working directory (one `lsof -d cwd` call covers many processes) and, failing both,
its parent's: a process too short-lived for `lsof` still counts toward whoever started it.
Headless WebKit pages are started by `launchd`, so they have no check or session as an
ancestor; they are reported as the `browser` context. A row's `unnamed` field records the
executable names of everything classified `other/other`, so the report can list the largest
consumers that no rule names.

A process's CPU is counted from the sample after it first appears, in full (it was born between
two samples), and processes alive when the sampler starts are only a baseline. The CPU a process
used between its last sample and its exit is lost, which is about a fifth of all work, so each
row also records `top`'s own busy figure for the minute (`top_cores`). Report both: the sampled
sum should be within about a fifth of `top`'s. `peak_cores` is the highest rate seen between two
consecutive samples, for bursts shorter than a minute.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import statistics
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from re import Pattern
from typing import TypedDict

from . import paths

# The classification of one process: (context, kind, worktree).
Key = tuple[str, str, str]

# Executable names that count as a kind of work, checked in order against the executable's
# base name, then against the whole command line for the interpreters.
_BY_EXECUTABLE: list[tuple[str, tuple[str, ...]]] = [
    (
        "compile",
        (
            "rustc",
            "cargo",
            "cc",
            "clang",
            "clang++",
            "ld",
            "rust-lld",
            "swiftc",
            "swift-frontend",
            "build-script-build",
        ),
    ),
    ("rust_test", ("cargo-nextest",)),
    (
        "browser",
        (
            "WebKitWebContent",
            "WebKitNetworking",
            "WebKitGPU",
            "Playwright",
            "chrome-headless-shell",
            "MiniBrowser",
            "com.apple.WebKit",
        ),
    ),
    ("oracle", ("java",)),
    (
        "simulator",
        ("Simulator", "simctl", "launchd_sim", "qemu-system-aarch64", "emulator"),
    ),
    (
        "editor",
        (
            "Electron",
            "Code Helper",
            "Code Helper (Renderer)",
            "Code Helper (Plugin)",
            "rust-analyzer",
            "rust-analyzer-proc-macro-srv",
            "Cursor Helper",
            "tsserver",
        ),
    ),
    (
        "desktop",
        (
            "Claude",
            "Claude Helper",
            "Claude Helper (Renderer)",
            "ChatGPT",
            "Codex",
            "WindowServer",
            "Finder",
            "Dock",
            "Safari",
            "kernel_task",
            "Terminal",
        ),
    ),
]
_TOOLS = (
    "python",
    "python3",
    "python3.15",
    "Python",
    "node",
    "git",
    "bd",
    "enact",
    "bash",
    "zsh",
    "sh",
    "just",
    "lsof",
    "ps",
    "top",
)

SLOW_CI: Pattern[str] = re.compile(
    r"beryl_tools\.cli\s+ci\b|\bby\s+ci\b|enact\.py\s+ci\b"
)
FAST_CHECK: Pattern[str] = re.compile(
    r"beryl_tools\.cli\s+check\b|\bby\s+check\b|enact\.py\s+push\b"
)
WORKTREE: Pattern[str] = re.compile(r"/\.worktrees/([^/\s]+)")


@dataclass
class Process:
    pid: int
    ppid: int
    cpu: float  # cumulative CPU seconds
    exe: str
    args: str
    cwd: str = ""


def parse_cputime(text: str) -> float:
    """Seconds in `ps`'s cputime column: `1:02.50`, `166:27.11` or `1:02:03.00`."""
    seconds = 0.0
    for part in text.strip().split(":"):
        seconds = seconds * 60 + float(part)
    return seconds


def kind_of(proc: Process) -> str:
    name = os.path.basename(proc.exe)
    if "/target/" in proc.exe and "/deps/" in proc.exe:
        return "rust_test"
    if "forge" in proc.args.lower() or "xmage" in proc.args.lower():
        return "oracle"
    for kind, names in _BY_EXECUTABLE:
        if any(
            name == n or (n.startswith("com.apple") and n in proc.exe) for n in names
        ):
            return kind
    if proc.exe.startswith(
        ("/System/", "/usr/libexec/", "/usr/sbin/", "/sbin/", "/Library/Apple/")
    ):
        return "system"
    if ".app/Contents/" in proc.exe:
        app = proc.exe.split(".app/Contents/")[0].rsplit("/", 1)[-1]
        if app in ("Claude", "ChatGPT", "Codex", "Google Chrome", "Slack", "Arq"):
            return "desktop"
        if app in ("Visual Studio Code", "Cursor"):
            return "editor"
    if "playwright" in proc.args or "webkit" in proc.exe.lower():
        return "browser"
    if name in _TOOLS:
        return "tool"
    return "other"


def classify(proc: Process, parent: Key | None) -> Key:
    """(context, kind, worktree) of `proc`, taking its parent's context when its own command
    line and working directory name none."""
    kind = kind_of(proc)
    found = WORKTREE.search(proc.cwd) or WORKTREE.search(proc.args)
    worktree = found.group(1) if found else "-"
    if SLOW_CI.search(proc.args) or worktree.startswith("ci-"):
        context = "slow_ci"
    elif FAST_CHECK.search(proc.args):
        context = "fast_check"
    elif kind == "browser" and proc.ppid == 1 and worktree == "-":
        context = "browser"
    elif kind == "desktop":
        context = "desktop"
    elif kind == "system":
        context = "system"
    elif kind == "editor":
        context = "editor"
    elif worktree != "-":
        context = "session"
    elif parent is not None and parent[0] not in ("other", "desktop", "editor"):
        return (parent[0], kind if kind != "other" else parent[1], parent[2])
    elif proc.cwd.startswith(str(paths.REPO)) or "/beryl" in proc.cwd:
        context = "session"
    else:
        context = "other"
    if (
        kind == "tool"
        and parent is not None
        and parent[0] == context
        and parent[1] not in ("tool", "other")
    ):
        kind = parent[1]
    if worktree == "-" and parent is not None and parent[0] == context:
        worktree = parent[2]
    return (context, kind, worktree)


def samples_file() -> Path:
    folder = paths.cache_root() / "cpu"
    folder.mkdir(parents=True, exist_ok=True)
    return folder / "samples.jsonl"


def _run(command: list[str]) -> str:
    return subprocess.run(command, capture_output=True, text=True, check=False).stdout


def read_processes() -> dict[int, Process]:
    """Every process: pid, parent, cumulative CPU, executable and arguments. The two `ps` calls
    are joined by pid; a process gone between them is dropped."""
    table: dict[int, Process] = {}
    for row in _run(["ps", "-axo", "pid=,ppid=,cputime=,comm="]).splitlines():
        parts = row.split(None, 3)
        if len(parts) == 4:
            table[int(parts[0])] = Process(
                int(parts[0]), int(parts[1]), parse_cputime(parts[2]), parts[3], ""
            )
    for row in _run(["ps", "-axo", "pid=,args="]).splitlines():
        parts = row.split(None, 1)
        if len(parts) == 2 and int(parts[0]) in table:
            table[int(parts[0])].args = parts[1]
    return table


def read_cwds(pids: list[int]) -> dict[int, str]:
    """The working directory of each of `pids` still running, from one `lsof` call."""
    if not pids:
        return {}
    out = _run(["lsof", "-a", "-d", "cwd", "-Fpn", "-p", ",".join(map(str, pids))])
    cwds: dict[int, str] = {}
    pid = 0
    for line in out.splitlines():
        if line.startswith("p"):
            pid = int(line[1:])
        elif line.startswith("n"):
            cwds[pid] = line[1:]
    return cwds


def top_busy_cores() -> float:
    """Cores `top` says are busy over one second: its second sample, as the first is since boot."""
    out = _run(["top", "-l", "2", "-n", "0", "-s", "1"])
    idle = re.findall(r"([\d.]+)% idle", out)
    return (1 - float(idle[-1]) / 100) * (os.cpu_count() or 1) if idle else 0.0


class Sampler:
    """Turns successive process tables into CPU seconds per [`Key`]."""

    def __init__(self) -> None:
        self.last: dict[int, float] = {}
        self.keys: dict[int, Key] = {}
        self.primed = False

    def step(
        self, table: dict[int, Process], cwds: dict[int, str]
    ) -> tuple[dict[Key, float], dict[str, float]]:
        """CPU seconds used since the previous step by each key, and by each executable name
        classified `other/other`. Parents are classified before children."""
        for proc in sorted(table.values(), key=lambda p: p.pid):
            proc.cwd = cwds.get(proc.pid, "")
        used: dict[Key, float] = {}
        unnamed: dict[str, float] = {}
        for pid in self._parents_first(table):
            proc = table[pid]
            if pid not in self.keys:
                self.keys[pid] = classify(proc, self.keys.get(proc.ppid))
            before = self.last.get(pid)
            if before is None:
                delta = proc.cpu if self.primed else 0.0
            else:
                delta = proc.cpu - before if proc.cpu >= before else proc.cpu
            self.last[pid] = proc.cpu
            if delta > 0:
                key = self.keys[pid]
                used[key] = used.get(key, 0.0) + delta
                if key[0] == "other" and key[1] == "other":
                    name = os.path.basename(proc.exe)
                    unnamed[name] = unnamed.get(name, 0.0) + delta
        for gone in set(self.last) - set(table):
            del self.last[gone]
            self.keys.pop(gone, None)
        self.primed = True
        return used, unnamed

    @staticmethod
    def _parents_first(table: dict[int, Process]) -> list[int]:
        order: list[int] = []
        seen: set[int] = set()

        def visit(pid: int) -> None:
            chain: list[int] = []
            while pid in table and pid not in seen:
                seen.add(pid)
                chain.append(pid)
                pid = table[pid].ppid
            order.extend(reversed(chain))

        for pid in table:
            visit(pid)
        return order


def sample(args: argparse.Namespace) -> int:
    """Appends a row each minute until killed (or after --duration seconds)."""
    out = samples_file()
    sampler = Sampler()
    cores = os.cpu_count() or 1
    deadline = time.monotonic() + args.duration if args.duration else None
    minute_start = time.time()
    total: dict[str, float] = {}
    unnamed_total: dict[str, float] = {}
    peak = 0.0
    previous = time.monotonic()
    while deadline is None or time.monotonic() < deadline:
        table = read_processes()
        fresh = [p for p in table if p not in sampler.keys]
        used, unnamed = sampler.step(table, read_cwds(fresh))
        now = time.monotonic()
        busy = sum(used.values())
        peak = max(peak, busy / max(now - previous, 1e-3))
        previous = now
        for key, seconds in used.items():
            name = "/".join(key)
            total[name] = total.get(name, 0.0) + seconds
        for name, seconds in unnamed.items():
            unnamed_total[name] = unnamed_total.get(name, 0.0) + seconds
        if time.time() - minute_start >= 60:
            row = {
                "time": minute_start,
                "seconds": time.time() - minute_start,
                "cores": cores,
                "cpu": {k: round(v, 3) for k, v in total.items()},
                "unnamed": {k: round(v, 3) for k, v in unnamed_total.items()},
                "peak_cores": round(peak, 2),
                "top_cores": round(top_busy_cores(), 2),
                "load": os.getloadavg()[0],
            }
            with out.open("a") as handle:
                handle.write(json.dumps(row) + "\n")
            minute_start, total, unnamed_total, peak = time.time(), {}, {}, 0.0
        time.sleep(args.interval)
    return 0


class Row(TypedDict):
    """One minute of samples, as written by `sample`."""

    time: float
    seconds: float
    cores: int
    cpu: dict[str, float]
    unnamed: dict[str, float]
    peak_cores: float
    top_cores: float
    load: float


def read_rows(since: float) -> list[Row]:
    path = samples_file()
    if not path.exists():
        return []
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    return [r for r in rows if float(r["time"]) >= since]


def summarize(rows: list[Row]) -> str:
    """The report text for `rows`: cores by context, kind and worktree, the largest unnamed
    executables, per-minute and burst figures, and `top`'s figure beside the sampled one.
    """
    if not rows:
        return "no samples: run `just cpu sample`"
    elapsed: float = sum(float(r["seconds"]) for r in rows)
    by_context: dict[str, float] = {}
    by_kind: dict[str, float] = {}
    by_worktree: dict[str, float] = {}
    unnamed: dict[str, float] = {}
    total: float = 0.0
    named: float = 0.0
    for r in rows:
        for name, seconds in r["cpu"].items():
            context, kind, worktree = name.split("/", 2)
            total += seconds
            if (context, kind) != ("other", "other"):
                named += seconds
            by_context[context] = by_context.get(context, 0.0) + seconds
            by_kind[kind] = by_kind.get(kind, 0.0) + seconds
            by_worktree[worktree] = by_worktree.get(worktree, 0.0) + seconds
        for name, seconds in r["unnamed"].items():
            unnamed[name] = unnamed.get(name, 0.0) + seconds
    per_minute = [sum(r["cpu"].values()) / float(r["seconds"]) for r in rows]
    cores = int(rows[-1]["cores"])
    top = statistics.mean(float(r["top_cores"]) for r in rows)
    lines: list[str] = [
        f"{len(rows)} minutes, {cores} cores",
        f"sampled {total / elapsed:.2f} cores on average ({total / elapsed / cores:.0%}); "
        f"top said {top:.2f}",
        f"per minute: median {statistics.median(per_minute):.2f}, "
        f"90th percentile {sorted(per_minute)[int(0.9 * (len(per_minute) - 1))]:.2f}, "
        f"max {max(per_minute):.2f}; busiest burst between two samples "
        f"{max(float(r['peak_cores']) for r in rows):.2f} cores",
        f"named {named / total:.0%} of the CPU seen" if total else "no CPU seen",
    ]

    def block(title: str, values: dict[str, float], limit: int = 12) -> None:
        lines.append(f"\n{title} (cores, share)")
        for name, seconds in sorted(values.items(), key=lambda kv: -kv[1])[:limit]:
            lines.append(f"  {seconds / elapsed:6.2f}  {seconds / total:5.0%}  {name}")

    if total:
        block("context", by_context)
        block("kind", by_kind)
        block("worktree", by_worktree)
        block("unnamed executables", unnamed, 10)
    return "\n".join(lines)


def report(args: argparse.Namespace) -> int:
    print(summarize(read_rows(time.time() - args.hours * 3600)))
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "cpu", help="record where the CPU goes, by session and kind of work"
    )
    commands = p.add_subparsers(dest="cpu_command", required=True)
    s = commands.add_parser("sample", help="append one row per minute until killed")
    s.add_argument(
        "--interval", type=float, default=2.0, help="seconds between samples"
    )
    s.add_argument(
        "--duration", type=float, default=0, help="stop after this many seconds"
    )
    s.set_defaults(func=sample)
    r = commands.add_parser("report", help="sum the recorded rows")
    r.add_argument("--hours", type=float, default=1.0, help="how far back to sum")
    r.set_defaults(func=report)
