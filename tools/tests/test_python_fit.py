"""The fast check's choice of Python test files (python_fit.py) and its pytest step's stop at
the end of its window (python_tests.py)."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

from beryl_tools import deadline, python_fit, python_tests, runlog
from beryl_tools.selection import Record


def tools_tree(root: Path) -> Path:
    """A tools/ folder with a package of three modules and four test files."""
    package = root / "src" / "beryl_tools"
    package.mkdir(parents=True)
    (package / "__init__.py").write_text("")
    (package / "paths.py").write_text("REPO = 1\n")
    (package / "deep.py").write_text("from .paths import REPO\n")
    (package / "top.py").write_text("from . import deep\n")
    tests = root / "tests"
    tests.mkdir()
    (tests / "test_top.py").write_text("from beryl_tools import top\n")
    (tests / "test_paths.py").write_text("from beryl_tools.paths import REPO\n")
    (tests / "test_alone.py").write_text("import json\n")
    (tests / "test_new.py").write_text("from beryl_tools import deep\n")
    return root


def test_imports_are_followed_through_modules_and_relative_imports(
    tmp_path: Path,
) -> None:
    graph = python_fit.import_graph(tools_tree(tmp_path))
    assert graph["beryl_tools.top"] == {"beryl_tools", "beryl_tools.deep"}
    assert "beryl_tools.paths" in python_fit.reached(graph, "tests.test_top")
    assert "beryl_tools.paths" not in python_fit.reached(graph, "tests.test_alone")


def test_a_change_names_its_test_files_and_modules_or_runs_everything() -> None:
    assert python_fit.touched(
        ["tools/tests/test_warm.py", "tools/src/beryl_tools/warm.py", "README.md"]
    ) == ({"tests/test_warm.py"}, {"beryl_tools.warm"})
    assert python_fit.touched(["tools/tests/conftest.py"]) is None
    assert python_fit.touched(["tools/pyproject.toml"]) is None
    assert python_fit.touched(["tools/tests/fixtures/a.json"]) is None
    assert python_fit.touched(["data/card-store.tsv"]) is None


def test_files_reaching_the_change_go_first_and_edited_files_always_run(
    tmp_path: Path,
) -> None:
    tools = tools_tree(tmp_path)
    known = {
        "python:tests/test_top.py::test_a": Record(0, 0, 10, 40.0),
        "python:tests/test_top.py::test_b": Record(0, 0, 10, 40.0),
        "python:tests/test_paths.py::test_a": Record(0, 0, 10, 2.0),
        "python:tests/test_alone.py::test_a": Record(0, 0, 10, 1.0),
    }
    # deep.py changed: test_top (80 s, 40 s on 2 workers) and test_new reach it; test_new has
    # never been timed. Of the rest, which do not reach it, the cheaper test_alone fits.
    chosen = python_fit.choose(
        ["tools/src/beryl_tools/deep.py"], known, 42.0 + python_fit.MARGIN, tools
    )
    assert chosen == (
        ["tests/test_alone.py", "tests/test_new.py", "tests/test_top.py"],
        ["tests/test_paths.py"],
    )
    # An edited test file runs even when nothing fits.
    chosen = python_fit.choose(["tools/tests/test_paths.py"], known, 0.0, tools)
    assert chosen is not None and chosen[0] == ["tests/test_paths.py"]
    assert python_fit.choose(["tools/pyproject.toml"], known, 0.0, tools) is None
    assert python_fit.choose(None, known, 0.0, tools) is None


def test_a_files_record_sums_its_tests() -> None:
    known = {
        "python:tests/test_a.py::test_x": Record(1, 2, 10, 3.0),
        "python:tests/test_a.py::test_y[1]": Record(0, 1, 4, 5.0),
        "python:black": Record(0, 0, 10, 1.0),
    }
    assert python_fit.file_records(known) == {"tests/test_a.py": Record(1, 3, 10, 8.0)}


def test_a_step_stopped_at_its_window_fails_only_on_a_finished_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    results = tmp_path / "results.jsonl"
    monkeypatch.setenv(runlog.RESULTS_ENV, str(results))
    results.write_text(
        json.dumps({"check": "python:tests/test_a.py::test_x", "outcome": "pass"})
        + "\n"
    )
    assert python_tests.stopped("python") == 0
    with open(results, "a") as f:
        f.write(
            json.dumps({"check": "python:tests/test_a.py::test_y", "outcome": "fail"})
            + "\n"
        )
    assert python_tests.stopped("python") == 1
    # The step's one rerun reads the failed test from the output, as from pytest's summary.
    assert "FAILED tests/test_a.py::test_y" in capsys.readouterr().out


def test_the_step_stops_pytest_at_the_end_of_its_window(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    started: list[list[str]] = []

    class Sleeper(subprocess.Popen[bytes]):
        def __init__(self, cmd: list[str], **kwargs: object) -> None:
            if cmd[1:3] != ["-m", "pytest"]:  # ps, by way of system.kill_tree
                super().__init__(cmd, **kwargs)
                return
            started.append(cmd)
            sleep = [sys.executable, "-c", "import time; time.sleep(60)"]
            super().__init__(sleep, start_new_session=True)

    monkeypatch.setattr(python_tests.subprocess, "Popen", Sleeper)
    monkeypatch.setattr(deadline, "step_limit", lambda: python_tests.STOP_EARLY + 1.0)
    monkeypatch.setattr(python_tests, "fit", lambda window: ["tests/test_a.py"])
    monkeypatch.setenv(runlog.RESULTS_ENV, str(tmp_path / "none.jsonl"))
    assert python_tests.pytest_step() == 0
    assert started[0][-1] == "tests/test_a.py"
