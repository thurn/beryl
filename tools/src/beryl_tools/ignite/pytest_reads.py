"""The repository files that Python code reads at run time, recorded so that a pytest check's
input hash covers them.

A pytest check's inputs (`list_checks` in `beryl_tools.ignite.python`) are found statically: the
test's file, the modules it imports and its conftest files. A module can also open a data file
at run time, as `beryl_tools.beryl3d.match.support` reads `data/beryl3d/match/supported.json`.
No import names that file, so before this record existed the check's input hash did not change
when the file did, and Ignite's gate reused a pass from before the change.

`data/pytest-reads.json` maps each module file (repository-relative) to the tracked files and
directories it reads at run time. `list_checks` adds a module's entries to the inputs of every
test that reaches the module through its imports. The entries belong to the module that opens
the file, not to the test that happened to run it, so a function that caches what it read
(`functools.cache`) is covered no matter which test called it first.

This module is a pytest plugin, which the adapter loads for every pytest check
(`-p beryl_tools.ignite.pytest_reads`). It installs an audit hook (`sys.addaudithook`) that
watches every file opened for reading (the `open` event) and every directory listed
(`os.listdir`, `os.scandir`). Each one that git tracks is attributed to the innermost frame on
the stack that runs a module of `tools` or `efficiency` (under `src` or `tests`). A read the
record does not list fails the test that made it, with a message naming the record's missing
entry; a read during collection fails the tests of the file being collected, and one outside
both fails every test of the run. The failure is deterministic: the same code reads the same
file on every run, so a test cannot pass in one run and fail in another for this reason.

With `IGNITE_RECORD_READS=1` the plugin adds what it saw to the record instead of failing
(`python -m beryl_tools.ignite.python reads` runs both projects' tests that way). Python source
files of the two projects are left out: the import graph already covers them. Files read by a
subprocess a test starts are not seen, since the hook lives in the pytest process only.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from collections.abc import Iterable, Mapping
from pathlib import Path
from types import FrameType
from typing import Any

import pytest

from beryl_tools.ignite.python import (
    RECORD,
    RECORD_ENV,
    ROOT_ENV,
    load_record,
    write_record,
)

# The two projects' folders, whose `src` and `tests` modules reads are attributed to.
PROJECT_FOLDERS = ("tools", "efficiency")
SOURCE_DIRS = ("src", "tests")
# Modules whose reads are Ignite's own bookkeeping, not a test's input.
OWN_MODULES = (
    "tools/src/beryl_tools/ignite/pytest_reads.py",
    "tools/src/beryl_tools/ignite/python.py",
)


def tracked(root: Path) -> tuple[frozenset[str], frozenset[str]]:
    """The files git tracks under `root`, and every directory that holds one."""
    done = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    )
    files = frozenset(p for p in done.stdout.split("\0") if p)
    dirs: set[str] = set()
    for file in files:
        parts = file.split("/")[:-1]
        dirs.update("/".join(parts[:n]) for n in range(1, len(parts) + 1))
    return files, frozenset(dirs)


def repository_root() -> Path | None:
    """The checkout's root, or None when pytest runs outside a git checkout (as the adapter's
    own tests do), where there is nothing to watch."""
    if root := os.environ.get(ROOT_ENV):
        return Path(root).resolve()
    done = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        capture_output=True,
        text=True,
    )
    if done.returncode != 0:
        return None
    return Path(done.stdout.strip()).resolve()


class Watcher:
    """What the audit hook has seen: every (module, path) read, and the ones the record lacks,
    by the test (node id), collected file or session that was running."""

    def __init__(self, root: Path, record: Mapping[str, Iterable[str]]) -> None:
        self.root: Path = root
        self.prefix: str = str(root) + os.sep
        self.record: dict[str, set[str]] = {
            module: set(paths) for module, paths in record.items()
        }
        files, dirs = tracked(root)
        self.files: frozenset[str] = files
        self.dirs: frozenset[str] = dirs
        self.sources: dict[str, str | None] = {}
        self.seen: dict[str, set[str]] = {}
        # The node id of the test running, or of the file being collected, or None.
        self.owner: str | None = None
        self.unlisted: dict[str | None, set[tuple[str, str]]] = {}
        self.busy = False

    def source(self, filename: str) -> str | None:
        """The repository path of a project module file, or None for any other file."""
        if filename in self.sources:
            return self.sources[filename]
        found: str | None = None
        if filename.startswith(self.prefix) and filename.endswith(".py"):
            rel = filename[len(self.prefix) :].replace(os.sep, "/")
            parts = rel.split("/")
            if (
                len(parts) > 2
                and parts[0] in PROJECT_FOLDERS
                and parts[1] in SOURCE_DIRS
                and rel not in OWN_MODULES
            ):
                found = rel
        self.sources[filename] = found
        return found

    def relative(self, path: object, directory: bool) -> str | None:
        """The repository path of a tracked file (or, for a listing, directory) that `path`
        names, or None."""
        if isinstance(path, os.PathLike):
            path = os.fspath(path)
        if isinstance(path, bytes):
            path = os.fsdecode(path)
        if not isinstance(path, str):
            return None
        full = os.path.realpath(os.path.abspath(path))
        if not full.startswith(self.prefix):
            return None
        rel = full[len(self.prefix) :].replace(os.sep, "/")
        if directory:
            return rel if rel in self.dirs else None
        if rel not in self.files:
            return None
        if rel.endswith(".py") and rel.split("/")[0] in PROJECT_FOLDERS:
            return None
        return rel

    def reader(self, frame: FrameType | None) -> str | None:
        while frame is not None:
            if module := self.source(frame.f_code.co_filename):
                return module
            frame = frame.f_back
        return None

    def hook(self, event: str, args: tuple[Any, ...]) -> None:
        if self.busy or event not in ("open", "os.listdir", "os.scandir"):
            return
        self.busy = True
        try:
            if event == "open":
                if not reads(args):
                    return
                rel = self.relative(args[0], directory=False)
            else:
                rel = self.relative(args[0] if args[0] is not None else ".", True)
            if rel is None:
                return
            caller = sys._getframe(1)
            # The import system lists package directories and reads source files to import
            # them; the import graph covers what it finds.
            if caller.f_code.co_filename.startswith("<frozen importlib"):
                return
            module = self.reader(caller)
            if module is None:
                return
            self.seen.setdefault(module, set()).add(rel)
            if not self.listed(module, rel):
                self.unlisted.setdefault(self.owner, set()).add((module, rel))
        finally:
            self.busy = False

    def listed(self, module: str, rel: str) -> bool:
        """Whether the record lists `rel` for `module`, itself or a directory holding it, whose
        tree ID covers it as an input."""
        paths = self.record.get(module, set())
        parts = rel.split("/")
        return any("/".join(parts[:n]) in paths for n in range(1, len(parts) + 1))

    def missing_for(self, nodeid: str) -> set[tuple[str, str]]:
        """The unlisted reads that fail the test `nodeid`: its own, its file's collection's and
        the session's."""
        file = nodeid.split("::", 1)[0]
        return (
            self.unlisted.get(nodeid, set())
            | self.unlisted.get(file, set())
            | self.unlisted.get(None, set())
        )


def reads(args: tuple[Any, ...]) -> bool:
    """Whether an `open` audit event opens its file for reading. `open()` passes its mode
    string; `os.open` passes no mode and its flags."""
    mode = args[1] if len(args) > 1 else None
    if isinstance(mode, str):
        return "r" in mode or "+" in mode
    flags = args[2] if len(args) > 2 and isinstance(args[2], int) else 0
    return flags & (os.O_WRONLY | os.O_RDWR) != os.O_WRONLY


def message(missing: Iterable[tuple[str, str]]) -> str:
    pairs = sorted(missing)
    listed = "; ".join(f"{module} read {path}" for module, path in pairs)
    return (
        f"read files that {RECORD} does not list ({listed}), so this check's input hash "
        f"would not change when they do. Add each path to its module's entry there, or run "
        f"`uv run --project tools python -m beryl_tools.ignite.python reads`."
    )


_watcher: Watcher | None = None


def pytest_configure(config: pytest.Config) -> None:
    global _watcher
    root = repository_root()
    if root is None:
        return
    _watcher = Watcher(root, load_record(root))
    sys.addaudithook(_watcher.hook)


@pytest.hookimpl(wrapper=True)
def pytest_make_collect_report(collector: pytest.Collector) -> Any:
    watcher = _watcher
    if watcher is None or not isinstance(collector, pytest.Module):
        return (yield)
    before = watcher.owner
    watcher.owner = collector.nodeid
    try:
        return (yield)
    finally:
        watcher.owner = before


@pytest.hookimpl(wrapper=True)
def pytest_runtest_protocol(item: pytest.Item) -> Any:
    watcher = _watcher
    if watcher is None:
        return (yield)
    watcher.owner = item.nodeid
    try:
        return (yield)
    finally:
        watcher.owner = None


@pytest.hookimpl(wrapper=True)
def pytest_runtest_makereport(item: pytest.Item, call: pytest.CallInfo[None]) -> Any:
    """Fails a test's teardown report when it, its file's collection or the session read a file
    the record does not list (unless the run records reads instead)."""
    report = yield
    watcher = _watcher
    if (
        watcher is None
        or call.when != "teardown"
        or os.environ.get(RECORD_ENV)
        or report.failed
    ):
        return report
    missing = watcher.missing_for(item.nodeid)
    if missing:
        report.outcome = "failed"
        report.longrepr = message(missing)
    return report


def pytest_sessionfinish(session: pytest.Session) -> None:
    """With `IGNITE_RECORD_READS` set, adds every read seen to the record."""
    watcher = _watcher
    if watcher is None or not os.environ.get(RECORD_ENV) or not watcher.seen:
        return
    watcher.busy = True
    record = load_record(watcher.root)
    for module, paths in watcher.seen.items():
        record[module] = sorted({*record.get(module, ()), *paths})
    write_record(watcher.root, record)
