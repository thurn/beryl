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
two samples), and processes alive when the sampler starts are only a baseline. On macOS each
process's own and reaped-children CPU come from `proc_pid_rusage` ([`read_rusage`]). When a
process exits, the CPU already counted for it is owed to its parent: the parent's
reaped-children total later grows by the child's whole lifetime, and only the growth beyond what
is owed is counted ([`Sampler`]). So the CPU a process used between its last sample and its exit,
and every process too short-lived to be sampled at all, is still counted, charged to the exited
child's context or else the parent's. Root-owned processes cannot be read that way, so their
own CPU comes from `ps` and a child that outlives its parent (re-parented to `launchd`) loses
its last few seconds. A pid is the same process for as long as its start time (`ps`'s `lstart`)
is; a figure that falls for the same process comes from a less precise reading (a root-owned
zombie shows `ps`'s cputime of zero) and adds nothing. A process `ps` lists but that is reaped
before `proc_pid_rusage` reads it is left out of that sample, as if it had already exited. Each
row also records `top`'s own busy figure for the minute (`top_cores`); the sampled sum should be
close to it. `peak_cores` is the highest rate seen between two consecutive samples, for bursts
shorter than a minute.

`by cpu install` keeps the sampler running as a launchd agent, and `by cpu uninstall` removes it.
"""

from __future__ import annotations

import argparse
import ctypes
import errno
import json
import os
import plistlib
import re
import shutil
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from re import Pattern
from typing import TypedDict

from . import nightly, paths

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
    "tk",
    "tk",
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
    # CPU seconds of the children this process has reaped, or None where it cannot be read.
    children: float | None = None
    # When the process started, as `ps` prints it: a pid whose start changes is a new process.
    start: str = ""


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
    """Every process: pid, parent, start time, cumulative CPU, executable and arguments. The two
    `ps` calls are joined by pid; a process gone between them is dropped. Where
    `proc_pid_rusage` can read a process, its CPU and its reaped children's come from there.
    """
    table: dict[int, Process] = {}
    # `lstart` is five words, such as `Sun Nov 15 06:54:46 2026`.
    for row in _run(["ps", "-axo", "pid=,ppid=,lstart=,cputime=,comm="]).splitlines():
        parts = row.split(None, 8)
        if len(parts) == 9:
            table[int(parts[0])] = Process(
                int(parts[0]),
                int(parts[1]),
                parse_cputime(parts[7]),
                parts[8],
                "",
                start=" ".join(parts[2:7]),
            )
    for row in _run(["ps", "-axo", "pid=,args="]).splitlines():
        parts = row.split(None, 1)
        if len(parts) == 2 and int(parts[0]) in table:
            table[int(parts[0])].args = parts[1]
    # Parents before children: a child read alive and then reaped before its parent is read
    # would be counted twice, once itself and once in its parent's reaped total. A child that
    # `ps` listed but that was reaped before it is read is dropped from the table for the same
    # reason: its parent may already have been read with the child's whole lifetime in its
    # reaped total, and [`Sampler`] must record what the child owes before that total grows.
    for pid in Sampler._parents_first(table):
        proc = table[pid]
        try:
            found = read_rusage(proc.pid)
        except ProcessLookupError:
            del table[pid]
            continue
        if found is not None:
            proc.cpu, proc.children = found
    return table


class _Timebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


_LIBSYSTEM: ctypes.CDLL | None = None
_TICK_SECONDS: float = 0.0
# `RUSAGE_INFO_V2` in <sys/resource.h>, and the 64-bit word offsets of the fields read from
# `struct rusage_info_v2`: after the 16-byte UUID come ri_user_time and ri_system_time, then
# eight other fields, then ri_child_user_time and ri_child_system_time.
_RUSAGE_INFO_V2 = 2
_USER, _SYSTEM, _CHILD_USER, _CHILD_SYSTEM = 2, 3, 12, 13


def read_rusage(pid: int) -> tuple[float, float] | None:
    """(own CPU seconds, reaped children's CPU seconds) of `pid` from macOS's
    `proc_pid_rusage`, or None off macOS or for a process this user cannot read (root's).
    Raises `ProcessLookupError` for a process that has been reaped (an unreaped zombie still
    reads). Its times are mach ticks, which `mach_timebase_info` converts to nanoseconds.
    """
    global _LIBSYSTEM, _TICK_SECONDS
    if sys.platform != "darwin":
        return None
    library = _LIBSYSTEM
    if library is None:
        library = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
        base = _Timebase()
        library.mach_timebase_info(ctypes.byref(base))
        _TICK_SECONDS = base.numer / base.denom / 1e9
        _LIBSYSTEM = library
    info = (ctypes.c_uint64 * 40)()
    if library.proc_pid_rusage(pid, _RUSAGE_INFO_V2, info) != 0:
        if ctypes.get_errno() == errno.ESRCH:
            raise ProcessLookupError(pid)
        return None
    own = (info[_USER] + info[_SYSTEM]) * _TICK_SECONDS
    children = (info[_CHILD_USER] + info[_CHILD_SYSTEM]) * _TICK_SECONDS
    return own, children


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
    """Turns successive process tables into CPU seconds per [`Key`].

    Besides each live process's own CPU, it counts the growth of each process's reaped-children
    total. When a process exits, the CPU already counted for it (its own, its reaped children's
    and what its own exited children were still owed) is recorded in `owed` against its parent,
    and the parent's next growth in reaped-children CPU pays that first. Only the rest is new:
    the exited child's last seconds and processes that never lived through a sample. That rest
    is charged to the most recently exited child's key (`tail`), or to the parent's own.

    A pid whose start time changed is an exit followed by a new process. For the same process, a
    figure is never allowed to fall: a zombie's `ps` cputime is zero, and taking that as a fresh
    start would leave its exit owing its parent nothing, so the parent's reaped total would count
    the child's whole lifetime a second time.
    """

    def __init__(self) -> None:
        self.last: dict[int, float] = {}
        self.last_children: dict[int, float] = {}
        self.parents: dict[int, int] = {}
        self.keys: dict[int, Key] = {}
        self.owed: dict[int, float] = {}
        self.tail: dict[int, Key] = {}
        self.starts: dict[int, str] = {}
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

        def charge(key: Key, exe: str, seconds: float) -> None:
            used[key] = used.get(key, 0.0) + seconds
            if key[0] == "other" and key[1] == "other":
                name = os.path.basename(exe)
                unnamed[name] = unnamed.get(name, 0.0) + seconds

        # Exits first: a child that exited and was reaped since the previous step shows up in its
        # parent's reaped total in this step, so what it is owed must be recorded before then.
        reused = {
            pid
            for pid, proc in table.items()
            if pid in self.starts and self.starts[pid] != proc.start
        }
        self._forget((set(self.last) - set(table)) | reused)
        for pid in self._parents_first(table):
            proc = table[pid]
            if pid not in self.keys:
                self.keys[pid] = classify(proc, self.keys.get(proc.ppid))
            key = self.keys[pid]
            self.parents[pid] = proc.ppid
            self.starts[pid] = proc.start
            before = self.last.get(pid)
            delta = self._growth(before, proc.cpu)
            self.last[pid] = max(before or 0.0, proc.cpu)
            if delta > 0:
                charge(key, proc.exe, delta)
            children = proc.children
            if children is None:
                continue
            before = self.last_children.get(pid)
            grown = self._growth(before, children)
            self.last_children[pid] = max(before or 0.0, children)
            owed = self.owed.get(pid, 0.0)
            paid = min(owed, grown)
            if owed:
                self.owed[pid] = owed - paid
            if grown - paid > 0:
                charge(self.tail.get(pid, key), proc.exe, grown - paid)
            if pid in self.tail and not self.owed.get(pid):
                del self.tail[pid]
        self.primed = True
        return used, unnamed

    def _growth(self, before: float | None, now: float) -> float:
        """How much a cumulative figure grew. A process first seen after the baseline was born
        since the previous sample, so all of it is new; a figure that fell is a less precise
        reading of the same process and adds nothing."""
        if before is None:
            return now if self.primed else 0.0
        return max(now - before, 0.0)

    def _forget(self, gone: set[int]) -> None:
        """Drops exited processes, children before parents, and records what was counted for
        each against its parent. Only a parent whose reaped-children CPU can be read will ever
        pay it back."""

        def depth(pid: int) -> int:
            levels = 0
            while pid in self.parents and levels < 256:
                pid = self.parents[pid]
                levels += 1
            return levels

        for pid in sorted(gone, key=depth, reverse=True):
            counted = (
                self.last.pop(pid)
                + self.last_children.pop(pid, 0.0)
                + self.owed.pop(pid, 0.0)
            )
            parent = self.parents.pop(pid, 0)
            key = self.keys.pop(pid, None)
            self.tail.pop(pid, None)
            self.starts.pop(pid, None)
            if parent in self.last_children:
                self.owed[parent] = self.owed.get(parent, 0.0) + counted
                if key is not None:
                    self.tail[parent] = key

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


# Rows older than this are dropped when the sampler starts, so the file stays a few hundred MB.
KEEP_DAYS = 30
LABEL = "ca.thurn.beryl.cpu"


def drop_old_rows(path: Path, now: float) -> None:
    """Rewrites `path` without the rows from before the last [`KEEP_DAYS`] days."""
    if not path.exists():
        return
    since = now - KEEP_DAYS * 86400
    lines = path.read_text().splitlines(keepends=True)
    kept = [
        line
        for line in lines
        if line.strip() and float(json.loads(line).get("time", 0)) >= since
    ]
    if len(kept) < len(lines):
        tmp = path.with_suffix(".tmp")
        tmp.write_text("".join(kept))
        tmp.replace(path)


def sample(args: argparse.Namespace) -> int:
    """Appends a row each minute until killed (or after --duration seconds)."""
    out = samples_file()
    drop_old_rows(out, time.time())
    sampler = Sampler()
    cores = os.cpu_count() or 1
    deadline = time.monotonic() + args.duration if args.duration else None
    # A row covers the time between two samples' process reads, so its CPU and its seconds
    # match; the `top` call after a row is written falls inside the next row.
    minute_start: float | None = None
    total: dict[str, float] = {}
    unnamed_total: dict[str, float] = {}
    peak = 0.0
    previous = time.monotonic()
    while deadline is None or time.monotonic() < deadline:
        table = read_processes()
        stamp = time.time()
        if minute_start is None:
            minute_start = stamp
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
        if stamp - minute_start >= 60:
            row = {
                "time": minute_start,
                "seconds": stamp - minute_start,
                "cores": cores,
                "cpu": {k: round(v, 3) for k, v in total.items()},
                "unnamed": {k: round(v, 3) for k, v in unnamed_total.items()},
                "peak_cores": round(peak, 2),
                "top_cores": round(top_busy_cores(), 2),
                "load": os.getloadavg()[0],
            }
            with out.open("a") as handle:
                handle.write(json.dumps(row) + "\n")
            minute_start, total, unnamed_total, peak = stamp, {}, {}, 0.0
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
        return "no samples: run `just cpu install`, or `just cpu sample`"
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


def plist_path() -> Path:
    return Path.home() / "Library" / "LaunchAgents" / f"{LABEL}.plist"


def agent_plist(main: Path, uv: str) -> dict[str, object]:
    """The launchd agent: `by cpu sample` from the main checkout's tools, started at login and
    restarted whenever it exits, at low priority. Its paths are POSIX's, as launchd's are.
    """
    log = samples_file().parent / "launchd.log"
    return {
        "Label": LABEL,
        "ProgramArguments": [
            uv,
            "run",
            "--quiet",
            "--project",
            (main / "tools").as_posix(),
            "by",
            "cpu",
            "sample",
        ],
        "WorkingDirectory": main.as_posix(),
        "RunAtLoad": True,
        "KeepAlive": True,
        "ThrottleInterval": 60,
        "ProcessType": "Background",
        "Nice": 10,
        "EnvironmentVariables": nightly.agent_environment(),
        "StandardOutPath": log.as_posix(),
        "StandardErrorPath": log.as_posix(),
    }


def install(args: argparse.Namespace) -> int:
    if sys.platform != "darwin":
        print(
            "cpu install: the launchd agent is supported on macOS only; nothing to do"
        )
        return 0
    main = paths.main_checkout()
    uv = shutil.which("uv")
    if main is None or uv is None:
        print("cpu install: needs the main checkout and uv", file=sys.stderr)
        return 1
    path = plist_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(plistlib.dumps(agent_plist(main, uv)))
    nightly.launchctl("bootout", f"{nightly.domain()}/{LABEL}")
    loaded = nightly.launchctl("bootstrap", nightly.domain(), str(path))
    if loaded.returncode:
        print(f"cpu install: launchctl: {loaded.stderr.strip()}", file=sys.stderr)
        return 1
    print(
        f"cpu install: {path} loaded: `by cpu sample` from {main} runs from now on and at "
        f"each login, writing {samples_file()}; rerun this after the sampler changes, and "
        "`just cpu uninstall` removes it"
    )
    return 0


def uninstall(args: argparse.Namespace) -> int:
    if sys.platform != "darwin":
        return 0
    nightly.launchctl("bootout", f"{nightly.domain()}/{LABEL}")
    plist_path().unlink(missing_ok=True)
    print(f"cpu uninstall: {LABEL} unloaded, {plist_path()} removed")
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "cpu", help="record where the CPU goes, by session and kind of work"
    )
    commands = p.add_subparsers(dest="cpu_command", required=True)
    s = commands.add_parser("sample", help="append one row per minute until killed")
    s.add_argument(
        "--interval", type=float, default=5.0, help="seconds between samples"
    )
    s.add_argument(
        "--duration", type=float, default=0, help="stop after this many seconds"
    )
    s.set_defaults(func=sample)
    r = commands.add_parser("report", help="sum the recorded rows")
    r.add_argument("--hours", type=float, default=1.0, help="how far back to sum")
    r.set_defaults(func=report)
    i = commands.add_parser(
        "install", help="keep the sampler running as a launchd agent (macOS)"
    )
    i.set_defaults(func=install)
    u = commands.add_parser("uninstall", help="unload and remove the launchd agent")
    u.set_defaults(func=uninstall)
