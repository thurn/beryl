"""The Python check families, `black`, `pyre` and `pytest`, over the two Python projects in the
repository: `tools` and `efficiency`. The families are declared in `ignite/checks/python.toml`.

Run from the repository's root as `python -m beryl_tools.ignite.python <verb> <family> ...`:

- `list <family>` prints one JSON line per check: `{"check": "pytest:tools/tests/test_x.py::t",
  "inputs": ["tools/src/beryl_tools/x.py", ...]}`. The inputs are the paths that check reads, in
  addition to the family's own `inputs` in the manifest.
- `run <family> <check>...` runs the checks named and writes one JSON line per check that
  reached a verdict to the file named by `IGNITE_RESULTS`: `{"check", "outcome"}` and, for a
  failure, `at` and `message`. A check the tool ended without judging (a crash of black, a
  pytest collection error) gets no line, so Ignite records it as `incomplete`.

A check is a whole project for `black` and `pyre` (`black:tools`, `pyre:efficiency`), which
cannot check a part of one, and one test for `pytest` (`pytest:<project>/<node id>`).

A pytest test's inputs are its file, the modules its file imports directly or indirectly (found
with `ast`, from `tools/src` or `efficiency/src` and the test files), and its fixtures: the
`conftest.py` files that apply to it, with what they import. Editing one module therefore
changes the input hash of the tests that reach it and of no others. The method is the import
graph that `beryl_tools.python_fit` builds; it is copied here because an adapter must not
import orchestration code.

`pytest` has no skipped outcome in Ignite's vocabulary, and a skipped test is neither a failure
nor evidence of a regression, so a skipped test (and an expected failure) is reported as a pass
whose message says why. This module is also the pytest plugin that writes those lines
(`-p beryl_tools.ignite.python`).
"""

from __future__ import annotations

import ast
import json
import os
import re
import subprocess
import sys
from collections.abc import Iterable, Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    import pytest

RESULTS_ENV = "IGNITE_RESULTS"
# The prefix of a project's node ids in check names, set for the pytest plugin by `run`.
PREFIX_ENV = "IGNITE_PYTEST_PREFIX"
TESTS_DIR = "tests"
FAMILIES = ("black", "pyre", "pytest")
# A run of more tests than this uses pytest-xdist.
PARALLEL_FROM = 8
WORKERS = 4
# The longest message a result carries.
MESSAGE_LIMIT = 300


@dataclass(frozen=True)
class Project:
    """A Python project: its folder under the repository's root and its package under `src`."""

    folder: str
    package: str


PROJECTS: tuple[Project, ...] = (
    Project("tools", "beryl_tools"),
    Project("efficiency", "beryl_efficiency"),
)


# --- The import graph (the method of python_fit.py) ---


def module_name(path: Path, project: Path) -> str | None:
    """The module a .py file under `project` imports as: `pkg.x` for src/pkg/x.py, `tests.test_x`
    for tests/test_x.py; None for any other file."""
    if path.suffix != ".py":
        return None
    parts = list(path.relative_to(project).with_suffix("").parts)
    if parts[0] == "src":
        parts = parts[1:]
    elif parts[0] != TESTS_DIR:
        return None
    if parts and parts[-1] == "__init__":
        parts = parts[:-1]
    return ".".join(parts) or None


def imported(source: str, module: str, package: bool, known: set[str]) -> set[str]:
    """The modules of `known` that the source of `module` imports, with their parent packages.
    `package` says whether `module` is a package's `__init__`, which relative imports start from.
    """
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


def project_files(project: Path) -> dict[str, Path]:
    """Each module of the project's `src` and `tests`, mapped to its file."""
    files: dict[str, Path] = {}
    for base in (project / "src", project / TESTS_DIR):
        for file in sorted(base.rglob("*.py")):
            if "__pycache__" in file.parts or ".venv" in file.parts:
                continue
            if name := module_name(file, project):
                files[name] = file
    return files


def import_graph(files: Mapping[str, Path]) -> dict[str, set[str]]:
    """Each module, mapped to the modules it imports directly."""
    known = set(files)
    return {
        name: imported(file.read_text(), name, file.name == "__init__.py", known)
        for name, file in files.items()
    }


def reached(graph: Mapping[str, set[str]], starts: Iterable[str]) -> set[str]:
    """The `starts` and every module they import, directly or through others."""
    seen: set[str] = set()
    todo = list(starts)
    while todo:
        name = todo.pop()
        if name not in seen:
            seen.add(name)
            todo.extend(graph.get(name, ()))
    return seen


def conftests(test_module: str, known: set[str]) -> list[str]:
    """The `conftest` modules that apply to a test module: one in each package from `tests` down
    to the test's own."""
    parts = test_module.split(".")[:-1]
    return [
        name
        for n in range(1, len(parts) + 1)
        if (name := ".".join([*parts[:n], "conftest"])) in known
    ]


def test_inputs(
    test_file: str,
    project: Path,
    files: Mapping[str, Path],
    graph: Mapping[str, set[str]],
    root: Path,
) -> list[str]:
    """The paths, from `root`, that the tests in `test_file` (relative to `project`) read: the
    file, its fixtures, and every module either imports."""
    module = module_name(project / test_file, project)
    if module is None:
        return [(project / test_file).relative_to(root).as_posix()]
    starts = [module, *conftests(module, set(files))]
    inputs = {files[name] for name in reached(graph, starts) if name in files}
    return sorted(p.relative_to(root).as_posix() for p in inputs)


# --- list ---


def collect(project: Project, root: Path) -> list[str]:
    """The node ids of every test of the project, as `tests/test_x.py::test`. A collection error
    raises, which leaves the family `incomplete`."""
    done = subprocess.run(
        [
            sys.executable,
            "-m",
            "pytest",
            "--collect-only",
            "-o",
            "addopts=",
            "-q",
            "-p",
            "no:cacheprovider",
        ],
        cwd=root / project.folder,
        capture_output=True,
        text=True,
    )
    if done.returncode not in (0, 5):  # 5: no tests collected
        raise RuntimeError(
            f"collecting {project.folder}'s tests failed:\n{done.stdout}{done.stderr}"
        )
    return [
        line
        for line in done.stdout.splitlines()
        if "::" in line and not line.startswith(" ")
    ]


def list_checks(family: str, root: Path) -> list[dict[str, Any]]:
    """The family's checks with the inputs of their own."""
    out: list[dict[str, Any]] = []
    for project in PROJECTS:
        folder = project.folder
        if family == "black":
            out.append(
                {
                    "check": f"black:{folder}",
                    "inputs": [f"{folder}/src", f"{folder}/{TESTS_DIR}"],
                }
            )
        elif family == "pyre":
            out.append(
                {
                    "check": f"pyre:{folder}",
                    "inputs": [
                        f"{folder}/src",
                        f"{folder}/{TESTS_DIR}",
                        f"{folder}/.pyre_configuration",
                    ],
                }
            )
        elif family == "pytest":
            path = root / folder
            files = project_files(path)
            graph = import_graph(files)
            cache: dict[str, list[str]] = {}
            for node in collect(project, root):
                test_file = node.split("::", 1)[0]
                if test_file not in cache:
                    cache[test_file] = test_inputs(test_file, path, files, graph, root)
                out.append(
                    {"check": f"pytest:{folder}/{node}", "inputs": cache[test_file]}
                )
        else:
            raise ValueError(f"unknown family {family!r}")
    return out


# --- run ---


def clip(text: str) -> str:
    text = " ".join(text.split())
    return text if len(text) <= MESSAGE_LIMIT else text[: MESSAGE_LIMIT - 1] + "…"


def tool(command: list[str], cwd: Path) -> tuple[int, str]:
    done = subprocess.run(command, cwd=cwd, capture_output=True, text=True)
    return done.returncode, done.stdout + done.stderr


def run_black(project: Project, root: Path) -> dict[str, Any] | None:
    code, output = tool(
        [sys.executable, "-m", "black", "--check", "--quiet", "."],
        root / project.folder,
    )
    # Black exits 1 when it would reformat a file and 123 when it failed on one.
    if code == 0:
        return {"outcome": "pass"}
    if code != 1:
        return None
    return {"outcome": "fail", "message": clip(output) or "would reformat files"}


PYRE_ERROR: re.Pattern[str] = re.compile(r"^(?P<path>\S+?):(?P<line>\d+):\d+ ")


def run_pyre(project: Project, root: Path) -> dict[str, Any] | None:
    code, output = tool(["pyre", "--noninteractive", "check"], root / project.folder)
    if code == 0:
        return {"outcome": "pass"}
    # Pyre exits 1 when it found type errors; any other code is a failure to run.
    if code != 1:
        return None
    first = next(
        (m for line in output.splitlines() if (m := PYRE_ERROR.match(line))), None
    )
    result: dict[str, Any] = {"outcome": "fail", "message": clip(output)}
    if first:
        result["at"] = f"{project.folder}/{first['path']}:{first['line']}"
    return result


def run_pytest(project: Project, nodes: list[str], root: Path, results: Path) -> None:
    """Runs the tests in `nodes`; the plugin below appends their lines to `results`."""
    command = [sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider"]
    command += ["-p", "beryl_tools.ignite.python"]
    if len(nodes) >= PARALLEL_FROM:
        command += ["-n", str(WORKERS)]
    subprocess.run(
        [*command, *nodes],
        cwd=root / project.folder,
        env={
            **os.environ,
            RESULTS_ENV: str(results),
            PREFIX_ENV: f"{project.folder}/",
        },
    )


def append(results: Path, lines: Iterable[dict[str, Any]]) -> None:
    with results.open("a") as out:
        for line in lines:
            out.write(json.dumps(line) + "\n")


def run_checks(family: str, names: Sequence[str], root: Path, results: Path) -> None:
    by_folder = {p.folder: p for p in PROJECTS}
    if family == "pytest":
        grouped: dict[str, list[str]] = {}
        for name in names:
            folder, _, node = name.removeprefix("pytest:").partition("/")
            grouped.setdefault(folder, []).append(node)
        for folder, nodes in grouped.items():
            run_pytest(by_folder[folder], nodes, root, results)
        return
    runner = {"black": run_black, "pyre": run_pyre}[family]
    for name in names:
        folder = name.removeprefix(f"{family}:")
        result = runner(by_folder[folder], root)
        if result is not None:
            append(results, [{"check": name, **result}])


# --- the pytest plugin ---

# What each test (by node id) has done so far in this session: its outcome and message.
_seen: dict[str, dict[str, Any]] = {}


def pytest_runtest_logreport(report: pytest.TestReport) -> None:
    """Folds the setup, call and teardown reports of a test into one result: a failure in any
    phase fails it, and a skip in setup or call is a pass that says it was skipped."""
    entry = _seen.setdefault(report.nodeid, {"outcome": "pass"})
    if entry["outcome"] == "fail":
        return
    if report.failed:
        crash = getattr(report.longrepr, "reprcrash", None)
        entry["outcome"] = "fail"
        entry["message"] = clip(getattr(crash, "message", None) or str(report.longrepr))
        if crash is not None:
            entry["at"] = (
                f"{os.environ.get(PREFIX_ENV, '')}{_relative(crash.path)}:{crash.lineno}"
            )
    elif report.skipped and report.when in ("setup", "call"):
        reason = getattr(report, "wasxfail", None)
        if reason is None:
            longrepr = report.longrepr
            reason = longrepr[2] if isinstance(longrepr, tuple) else str(longrepr)
            entry["message"] = clip(f"skipped: {reason}")
        else:
            entry["message"] = clip(f"expected failure: {reason}")


def _relative(path: str) -> str:
    try:
        return Path(path).relative_to(Path.cwd()).as_posix()
    except ValueError:
        return path


def pytest_sessionfinish(session: pytest.Session) -> None:
    """Writes the results once, on the process that runs the session (not an xdist worker)."""
    results = os.environ.get(RESULTS_ENV)
    if not results or hasattr(session.config, "workerinput"):
        return
    prefix = os.environ.get(PREFIX_ENV, "")
    append(
        Path(results),
        (
            {"check": f"pytest:{prefix}{node}", **entry}
            for node, entry in sorted(_seen.items())
        ),
    )


def main(argv: Sequence[str]) -> int:
    root = Path.cwd()
    if len(argv) < 2 or argv[0] not in ("list", "run") or argv[1] not in FAMILIES:
        print(__doc__, file=sys.stderr)
        return 1
    verb, family, rest = argv[0], argv[1], list(argv[2:])
    if verb == "list":
        for line in list_checks(family, root):
            print(json.dumps(line))
        return 0
    results = os.environ.get(RESULTS_ENV)
    if not results:
        print(f"{RESULTS_ENV} is not set", file=sys.stderr)
        return 1
    run_checks(family, rest, root, Path(results))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
