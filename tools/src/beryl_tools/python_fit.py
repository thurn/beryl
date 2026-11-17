"""Which Python test files the fast check's pytest step runs, so that the python lane finishes
inside the fast check's target (deadline.py) and leaves the rest to the browser sweep, which runs
the whole suite on staging every few hours (sweep.py).

Before this, the step ran all of the roughly 855 tests on any change to tools/. Between 08:00
UTC 2026-11-16 and 08:00 UTC 2026-11-17 the step took a median of 93 s, and in 25 of 100 fast
checks the 180 s hard limit cut it off, so those pushes waited the full 180 s. Slow CI does not
run the Python tests (slow_only.FAST_ONLY), so the tests a cut-off step never reached ran
nowhere. Seven files (test_bisect, test_checks, test_caches, test_verdicts, test_beryl3d_camera,
test_runlog and test_warm) held about 230 of the suite's 280 test-seconds.

Selecting by imports alone would not have been enough: `checks.py` imports nearly every module,
so 70 of the 107 test files reach it, and replaying three days of staging commits, the files
whose imports reach a change held a median of 67% of the suite's time. So the step plans by
file, in this order, and runs the files that fit the step's window (`deadline.step_limit`):

1. Test files the change edits or adds. These always run, however little time is left.
2. Test files whose imports, followed through tools/src, reach a module the change edits.
3. Test files the run log has never timed.
4. The rest by their record (selection.Record): files with tests that caught a break first,
   then those that fail most often, then the cheapest.

A change to anything under tools/ other than a module of tools/src or a test file (conftest.py,
pyproject.toml, uv.lock, a data file a test reads) runs every file, as does a change to the
card store, which the tests read.
"""

from __future__ import annotations

import ast
from collections.abc import Iterable, Mapping
from pathlib import Path
from typing import TYPE_CHECKING

from . import paths

if TYPE_CHECKING:
    from .selection import Record

# The seconds the plan keeps back for pytest-xdist to start its workers and collect.
MARGIN = 6.0
# The workers pytest_step runs (`-n 4`). The plan takes them as four tests at a time, which is
# optimistic under load (on 2026-11-17 the whole suite, about 200 test-seconds by the run log,
# took 104 s at load 25), because the step stops pytest at the window's end anyway: planning at
# two workers deferred 42 of 107 files in a push whose tests then ended in 20 s of a 46 s window.
WORKERS = 4
TESTS_DIR = "tests"


def module_name(path: Path, tools: Path) -> str | None:
    """The module a .py file under `tools` imports as: `beryl_tools.x` for src/beryl_tools/x.py,
    `tests.test_x` for tests/test_x.py; None for any other file."""
    if path.suffix != ".py":
        return None
    rel = path.relative_to(tools)
    parts = list(rel.with_suffix("").parts)
    if parts[0] == "src":
        parts = parts[1:]
    elif parts[0] != TESTS_DIR:
        return None
    if parts and parts[-1] == "__init__":
        parts = parts[:-1]
    return ".".join(parts) or None


def imported(source: str, module: str, package: bool, known: set[str]) -> set[str]:
    """The modules of `known` that the source of `module` imports, with their parent packages.
    `package` says whether `module` is a package's `__init__`, which relative imports start
    from."""
    here = module.split(".") if package else module.split(".")[:-1]
    names: set[str] = set()
    for node in ast.walk(ast.parse(source)):
        if isinstance(node, ast.Import):
            names.update(a.name for a in node.names)
        elif isinstance(node, ast.ImportFrom):
            if node.level:
                base = here[: len(here) - (node.level - 1)]
                start = ".".join([*base, *([node.module] if node.module else [])])
            else:
                start = node.module or ""
            names.add(start)
            # `from . import paths` imports a module; `from .paths import REPO` a name in one.
            names.update(f"{start}.{a.name}" for a in node.names)
    found: set[str] = set()
    for name in names:
        while name and name not in known:
            name = name.rpartition(".")[0]
        if name:
            parts = name.split(".")
            found.update(
                ".".join(parts[:n])
                for n in range(1, len(parts) + 1)
                if ".".join(parts[:n]) in known
            )
    return found


def import_graph(tools: Path = paths.TOOLS) -> dict[str, set[str]]:
    """Each module of tools/src and tools/tests, mapped to the modules it imports directly."""
    files = {
        name: f
        for f in [*(tools / "src").rglob("*.py"), *(tools / TESTS_DIR).rglob("*.py")]
        if (name := module_name(f, tools))
    }
    known = set(files)
    return {
        name: imported(f.read_text(), name, f.name == "__init__.py", known)
        for name, f in files.items()
    }


def reached(graph: Mapping[str, set[str]], start: str) -> set[str]:
    """`start` and every module it imports, directly or through others."""
    seen: set[str] = set()
    todo = [start]
    while todo:
        name = todo.pop()
        if name not in seen:
            seen.add(name)
            todo.extend(graph.get(name, ()))
    return seen


def test_files(tools: Path = paths.TOOLS) -> list[str]:
    """Every test file, as pytest names it from tools/: `tests/test_x.py`."""
    return sorted(
        f"{TESTS_DIR}/{f.relative_to(tools / TESTS_DIR).as_posix()}"
        for f in (tools / TESTS_DIR).rglob("test_*.py")
    )


def file_records(known: Mapping[str, Record]) -> dict[str, Record]:
    """Each test file's record from its tests' records (`python:<file>::<test>`): the sum of
    their catches, failures and mean seconds, over the most runs any of them had."""
    from .selection import Record  # It imports checks, which imports this module.

    out: dict[str, Record] = {}
    for check, r in known.items():
        if not check.startswith("python:") or "::" not in check:
            continue
        name = check.removeprefix("python:").split("::", 1)[0]
        was = out.get(name, Record(0, 0, 0, 0.0))
        out[name] = Record(
            was.catches + r.catches,
            was.failures + r.failures,
            max(was.runs, r.runs),
            was.seconds + r.seconds,
        )
    return out


def touched(changed: Iterable[str]) -> tuple[set[str], set[str]] | None:
    """The test files and the other modules a change edits, as `tests/test_x.py` and
    `beryl_tools.x` (or `tests.helper` for a helper beside the tests); None when it edits anything else the tests can read, so every file runs.
    Paths outside tools/ are left out: the fast check runs the python lane for those only on a
    card store change, which `CARD_STORE` reports as such."""
    tests: set[str] = set()
    modules: set[str] = set()
    for path in changed:
        if path == "data/card-store.tsv":
            return None
        if not path.startswith("tools/"):
            continue
        rel = path.removeprefix("tools/")
        name = rel.rsplit("/", 1)[-1]
        if rel.startswith(f"{TESTS_DIR}/") and name.startswith("test_"):
            tests.add(rel)
        elif (
            rel.startswith(("src/", f"{TESTS_DIR}/"))
            and rel.endswith(".py")
            and name != "conftest.py"
        ):
            module = module_name(paths.TOOLS / rel, paths.TOOLS)
            if module:
                modules.add(module)
        else:
            return None
    return tests, modules


def plan(
    files: list[str],
    edited: set[str],
    reaching: set[str],
    known: Mapping[str, Record],
    left: float,
    workers: int = WORKERS,
) -> tuple[list[str], list[str]]:
    """The test `files` to run in `left` seconds on `workers` workers, and those cut. The files
    in `edited` always run; the rest go in the order the module comment gives, `reaching` (the
    files whose imports reach an edited module) first."""
    from . import selection  # It imports checks, which imports this module.

    held = [f for f in files if f in edited]
    rest = [f for f in files if f not in edited]
    near = [f for f in rest if f in reaching]
    far = [f for f in rest if f not in reaching]

    def ordered(group: list[str]) -> list[str]:
        new = [f for f in group if f not in known]
        return new + selection.order([f for f in group if f in known], known)

    spent = sum(
        known[f].seconds if f in known else selection.DEFAULT_SECONDS for f in held
    )
    chosen, cut = selection.fit(
        ordered(near) + ordered(far), known, left - MARGIN - spent / workers, workers
    )
    run = {*held, *chosen}
    return [f for f in files if f in run], [f for f in files if f not in run]


def choose(
    changed: list[str] | None,
    known: Mapping[str, Record],
    left: float,
    tools: Path = paths.TOOLS,
) -> tuple[list[str], list[str]] | None:
    """The test files the fast check runs and those it defers to the sweep, or None to run them
    all: no base to compare with, or a change outside tools/src and the test files."""
    if changed is None or (edits := touched(changed)) is None:
        return None
    edited, modules = edits
    files = test_files(tools)
    graph = import_graph(tools)
    reaching = {
        f
        for f in files
        if reached(graph, f.removesuffix(".py").replace("/", ".")) & modules
    }
    return plan(files, edited, reaching, file_records(known), left)


def deferred_line(cut: list[str], left: float) -> str:
    return (
        f"pytest: {len(cut)} test files deferred to the sweep: they do not fit the"
        f" {max(left, 0):.0f} s left of this step's window ({', '.join(cut)})"
    )
