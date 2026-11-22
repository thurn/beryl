"""The pytest reads plugin: a test whose module reads a data file at run time fails until the
record lists the file, and once it does, the file is among the test's inputs, so editing it
changes the test's input hash and editing an unrelated file does not."""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
from pathlib import Path

from beryl_tools.ignite import pytest_reads
from beryl_tools.ignite import python as adapter

READER = """\
import json
from functools import cache
from pathlib import Path

TABLE = Path(__file__).resolve().parents[3] / "data" / "table.json"


@cache
def table() -> dict[str, int]:
    return json.loads(TABLE.read_text())
"""

TESTS = """\
from pkg import reader


def test_reads_the_table() -> None:
    assert reader.table()["a"] == 1


def test_also_reads_the_table() -> None:
    assert reader.table()["b"] == 2
"""

PLUGIN_TESTS = """\
import os

import mdformat
import pytest

# A spelling pytest's own plugin discovery has not already cached.
SEARCH_PATH = "./."


def test_discovers_formatter_plugin(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.syspath_prepend(SEARCH_PATH)
    assert mdformat.text("~~strike~~", extensions={"gfm"}).strip() == "~~strike~~"


def test_lists_project_directory() -> None:
    assert "pyproject.toml" in os.listdir(SEARCH_PATH)
"""

NODE = "tests/test_reader.py::test_reads_the_table"
PROJECT = adapter.Project("tools", "pkg")


def write(root: Path, files: dict[str, str]) -> None:
    for name, text in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)


def checkout(root: Path) -> None:
    """A git checkout with a `tools` project whose `pkg.reader` reads `data/table.json`, an
    unrelated `data/other.json` and an empty record."""
    write(
        root,
        {
            "tools/pyproject.toml": '[tool.pytest.ini_options]\npythonpath = ["src"]\n',
            "tools/src/pkg/__init__.py": "",
            "tools/src/pkg/reader.py": READER,
            "tools/tests/__init__.py": "",
            "tools/tests/test_reader.py": TESTS,
            "data/table.json": '{"a": 1, "b": 2}\n',
            "data/other.json": "{}\n",
            adapter.RECORD: '{"reads": {}}\n',
        },
    )
    subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    subprocess.run(["git", "add", "-A"], cwd=root, check=True)


def run(root: Path, nodes: list[str]) -> dict[str, dict[str, str]]:
    results = root.parent / f"{root.name}-results.jsonl"
    results.unlink(missing_ok=True)
    adapter.run_pytest(PROJECT, nodes, root, results)
    return {r["check"]: r for r in map(json.loads, results.read_text().splitlines())}


def input_hash(root: Path) -> str:
    project = root / "tools"
    files = adapter.project_files(project)
    graph = adapter.import_graph(files)
    record = adapter.load_record(root)
    paths = adapter.test_inputs(
        "tests/test_reader.py", project, files, graph, root, record
    )
    return hashlib.sha256(
        b"".join(p.encode() + (root / p).read_bytes() for p in paths)
    ).hexdigest()


def test_a_read_the_record_lacks_fails_until_recorded_and_then_is_an_input(
    tmp_path: Path,
) -> None:
    root = tmp_path / "repo"
    checkout(root)
    check = f"pytest:tools/{NODE}"

    failed = run(root, [NODE])[check]
    assert failed["outcome"] == "fail"
    assert "tools/src/pkg/reader.py read data/table.json" in failed["message"]
    assert "data/table.json" not in adapter.test_inputs(
        "tests/test_reader.py",
        root / "tools",
        adapter.project_files(root / "tools"),
        adapter.import_graph(adapter.project_files(root / "tools")),
        root,
        adapter.load_record(root),
    )

    # Recording runs every test once; the read belongs to the module, so the cache that
    # keeps the second test from opening the file again does not matter.
    adapter.record_reads(root)
    assert adapter.load_record(root) == {"tools/src/pkg/reader.py": ["data/table.json"]}
    assert run(root, [NODE])[check]["outcome"] == "pass"

    recorded = input_hash(root)
    (root / "data/other.json").write_text('{"x": 1}\n')
    assert input_hash(root) == recorded
    (root / "data/table.json").write_text('{"a": 1, "b": 3}\n')
    assert input_hash(root) != recorded


def test_plugin_discovery_does_not_hide_a_real_directory_read(tmp_path: Path) -> None:
    root = tmp_path / "repo"
    checkout(root)
    write(root, {"tools/tests/test_reader.py": PLUGIN_TESTS})
    discovery = "tests/test_reader.py::test_discovers_formatter_plugin"
    listing = "tests/test_reader.py::test_lists_project_directory"
    results = run(root, [discovery, listing])

    discovered = results[f"pytest:tools/{discovery}"]
    assert discovered["outcome"] == "pass", discovered
    failed = results[f"pytest:tools/{listing}"]
    assert failed["outcome"] == "fail"
    assert "tools/tests/test_reader.py read tools" in failed["message"]


def test_a_recorded_file_that_was_deleted_is_not_an_input(tmp_path: Path) -> None:
    root = tmp_path / "repo"
    checkout(root)
    adapter.write_record(
        root, {"tools/src/pkg/reader.py": ["data/table.json", "data/gone.json"]}
    )
    found = adapter.module_reads(
        adapter.load_record(root),
        ["tools/src/pkg/reader.py"],
        lambda path: (root / path).exists(),
    )
    assert found == {"data/table.json"}


def test_only_opening_a_file_for_reading_counts_as_a_read() -> None:
    assert pytest_reads.reads(("f", "r", 0))
    assert pytest_reads.reads(("f", "rb", 0))
    assert pytest_reads.reads(("f", "w+", 0))
    assert not pytest_reads.reads(("f", "w", 0))
    assert not pytest_reads.reads(("f", "ab", 0))
    assert pytest_reads.reads(("f", None, os.O_RDONLY))
    assert pytest_reads.reads(("f", None, os.O_RDWR | os.O_CREAT))
    assert not pytest_reads.reads(("f", None, os.O_WRONLY | os.O_CREAT))
