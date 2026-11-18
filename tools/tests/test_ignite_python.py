"""The Python check families' adapter: which paths a test reads, and how results are reported."""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from beryl_tools.ignite import python as adapter


def write(root: Path, files: dict[str, str]) -> None:
    for name, text in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)


def synthetic(root: Path) -> Path:
    """A project with `pkg.a` (which imports `pkg.b`), `pkg.c`, and a test for each, one beside
    them with a conftest that imports `pkg.d`."""
    project = root / "proj"
    write(
        project,
        {
            "src/pkg/__init__.py": "",
            "src/pkg/a.py": "from . import b\nVALUE = b.VALUE\n",
            "src/pkg/b.py": "VALUE = 1\n",
            "src/pkg/c.py": "VALUE = 2\n",
            "src/pkg/d.py": "VALUE = 3\n",
            "tests/__init__.py": "",
            "tests/conftest.py": "from pkg import d\n",
            "tests/test_a.py": "from pkg import a\n\ndef test_a() -> None:\n    assert a.VALUE\n",
            "tests/test_c.py": "from pkg.c import VALUE\n\ndef test_c() -> None:\n    assert VALUE\n",
        },
    )
    return project


def inputs_of(project: Path, test_file: str) -> list[str]:
    files = adapter.project_files(project)
    graph = adapter.import_graph(files)
    return adapter.test_inputs(test_file, project, files, graph, project.parent)


def digest(root: Path, paths: list[str]) -> str:
    return hashlib.sha256(
        b"".join(p.encode() + (root / p).read_bytes() for p in paths)
    ).hexdigest()


def test_a_test_reads_its_file_its_imports_and_its_fixtures(tmp_path: Path) -> None:
    project = synthetic(tmp_path)
    assert inputs_of(project, "tests/test_a.py") == [
        "proj/src/pkg/__init__.py",
        "proj/src/pkg/a.py",
        "proj/src/pkg/b.py",
        "proj/src/pkg/d.py",
        "proj/tests/conftest.py",
        "proj/tests/test_a.py",
    ]


def test_editing_a_module_changes_only_the_tests_that_reach_it(tmp_path: Path) -> None:
    project = synthetic(tmp_path)
    before = {
        f: digest(tmp_path, inputs_of(project, f))
        for f in ("tests/test_a.py", "tests/test_c.py")
    }
    # `b` is imported by `a` and so reached by test_a through it, but not by test_c.
    (project / "src/pkg/b.py").write_text("VALUE = 5\n")
    after = {
        f: digest(tmp_path, inputs_of(project, f))
        for f in ("tests/test_a.py", "tests/test_c.py")
    }
    assert after["tests/test_a.py"] != before["tests/test_a.py"]
    assert after["tests/test_c.py"] == before["tests/test_c.py"]
    # A fixture's import is read by every test.
    (project / "src/pkg/d.py").write_text("VALUE = 6\n")
    assert (
        digest(tmp_path, inputs_of(project, "tests/test_c.py"))
        != after["tests/test_c.py"]
    )


PLUGIN_TESTS = """\
import pytest

def test_passes() -> None:
    pass

def test_fails() -> None:
    assert 1 == 2

@pytest.mark.skip(reason="not today")
def test_skipped() -> None:
    raise AssertionError
"""


@pytest.mark.parametrize("workers", [[], ["-n", "2"]])
def test_the_plugin_reports_a_pass_a_failure_and_a_skip_as_three_results(
    tmp_path: Path, workers: list[str]
) -> None:
    write(tmp_path, {"test_x.py": PLUGIN_TESTS})
    results = tmp_path / "results.jsonl"
    subprocess.run(
        [
            sys.executable,
            "-m",
            "pytest",
            "-q",
            "-p",
            "no:cacheprovider",
            "-p",
            "beryl_tools.ignite.python",
            *workers,
            "test_x.py",
        ],
        cwd=tmp_path,
        env={
            **os.environ,
            "IGNITE_RESULTS": str(results),
            "IGNITE_PYTEST_PREFIX": "proj/",
        },
        capture_output=True,
    )
    lines = {r["check"]: r for r in map(json.loads, results.read_text().splitlines())}
    assert set(lines) == {
        "pytest:proj/test_x.py::test_passes",
        "pytest:proj/test_x.py::test_fails",
        "pytest:proj/test_x.py::test_skipped",
    }
    assert lines["pytest:proj/test_x.py::test_passes"]["outcome"] == "pass"
    failed = lines["pytest:proj/test_x.py::test_fails"]
    assert failed["outcome"] == "fail"
    assert failed["at"] == "proj/test_x.py:7"
    skipped = lines["pytest:proj/test_x.py::test_skipped"]
    assert skipped["outcome"] == "pass"
    assert "not today" in skipped["message"]


def test_a_collection_error_leaves_the_test_unreported(tmp_path: Path) -> None:
    write(
        tmp_path,
        {"test_broken.py": "import nothing_here\n\ndef test_x() -> None: ...\n"},
    )
    results = tmp_path / "results.jsonl"
    subprocess.run(
        [
            sys.executable,
            "-m",
            "pytest",
            "-q",
            "-p",
            "no:cacheprovider",
            "-p",
            "beryl_tools.ignite.python",
            "test_broken.py",
        ],
        cwd=tmp_path,
        env={**os.environ, "IGNITE_RESULTS": str(results)},
        capture_output=True,
    )
    assert not results.exists() or results.read_text().strip() == ""


SPACED_TESTS = """\
import pytest

@pytest.mark.parametrize("card", ["Grizzly Bears", "Counterspell"])
def test_card(card: str) -> None:
    pass
"""


def test_a_parametrized_id_with_spaces_is_a_valid_check_name_that_runs(
    tmp_path: Path,
) -> None:
    write(tmp_path, {"test_x.py": SPACED_TESTS})
    results = tmp_path / "results.jsonl"
    spaced = "pytest:proj/test_x.py::test_card[Grizzly%20Bears]"
    plain = "pytest:proj/test_x.py::test_card[Counterspell]"
    nodes = [
        adapter.decode_part(name.removeprefix("pytest:proj/"))
        for name in (spaced, plain)
    ]
    assert nodes == [
        "test_x.py::test_card[Grizzly Bears]",
        "test_x.py::test_card[Counterspell]",
    ]
    subprocess.run(
        [
            sys.executable,
            "-m",
            "pytest",
            "-q",
            "-p",
            "no:cacheprovider",
            "-p",
            "beryl_tools.ignite.python",
            *nodes,
        ],
        cwd=tmp_path,
        env={
            **os.environ,
            "IGNITE_RESULTS": str(results),
            "IGNITE_PYTEST_PREFIX": "proj/",
        },
        capture_output=True,
    )
    lines = {r["check"]: r for r in map(json.loads, results.read_text().splitlines())}
    assert set(lines) == {spaced, plain}
    assert all(not any(c.isspace() for c in name) for name in lines)
    assert lines[spaced]["outcome"] == "pass"


def test_a_name_without_spaces_or_percent_signs_is_unchanged() -> None:
    for name in ["test_x.py::test_a_b", "test_x.py::test_c[1-2]"]:
        assert adapter.check_part(name) == name
    odd = "test_x.py::t[a b\t100%]"
    assert adapter.decode_part(adapter.check_part(odd)) == odd
