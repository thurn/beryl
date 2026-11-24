"""Every Ignite check family's commands run as Ignite runs them.

Ignite splits a family's `list` and `run` commands on whitespace and runs them without a shell
(`words` in ignite/crates/ignite_daemon/src/checks/adapters.rs), so quotes reach the program as
characters. For a `nextest` family it appends `-E <filterset>` naming the tests to run, and reads
each test's outcome from the libtest JSON in the command's output; `{checks}` is replaced only in
a `jsonl` family's `run`. The first landing gate after the switch to Ignite failed on both: the
`client-test` family's run passed nextest the literal filterset `{checks}`, and the `rust` family's
list passed `'not` and `binary(allocations)'` as two arguments. A `nextest` family's commands run
in the directory of the first product it needs (`nextest_dir` in
ignite/crates/ignite_daemon/src/checks/execute.rs), so the `rust` family, run in rust/, named
rust/engine/Cargo.toml as its manifest.
"""

from __future__ import annotations

import tomllib
from pathlib import Path
from typing import Any

REPO: Path = Path(__file__).resolve().parents[2]
COMMANDS: tuple[str, ...] = ("list", "run", "next")


def tables(kind: str) -> dict[str, dict[str, Any]]:
    found: dict[str, dict[str, Any]] = {}
    for manifest in sorted((REPO / "ignite" / "checks").glob("*.toml")):
        found.update(tomllib.loads(manifest.read_text()).get(kind, {}))
    return found


def families() -> dict[str, dict[str, Any]]:
    return tables("family")


def test_no_command_quotes_an_argument() -> None:
    quoted = {
        f"{name}.{key}": word
        for name, family in families().items()
        for key in COMMANDS
        for word in str(family.get(key) or "").split()
        if "'" in word or '"' in word
    }
    assert quoted == {}


def test_nextest_runs_name_no_tests_and_print_libtest_json() -> None:
    nextest = {n: f for n, f in families().items() if f.get("adapter") == "nextest"}
    assert nextest, "no nextest family found"
    for name, family in nextest.items():
        run = str(family.get("run") or "").split()
        if not run:
            continue  # Ignite's default run command
        assert "{checks}" not in run, name
        assert "-E" not in run, name
        assert "libtest-json-plus" in run, name
        assert run[run.index("libtest-json-plus") - 1] == "--message-format", name


def test_nextest_manifest_paths_resolve_where_the_commands_run() -> None:
    products = tables("product")
    for name, family in families().items():
        if family.get("adapter") != "nextest":
            continue
        needs = family.get("needs") or []
        dir_name = products[needs[0]].get("dir", "") if needs else ""
        for key in ("list", "run"):
            words = str(family.get(key) or "").split()
            if "--manifest-path" in words:
                manifest = words[words.index("--manifest-path") + 1]
                assert (REPO / dir_name / manifest).is_file(), f"{name}.{key}"
            else:
                assert (REPO / dir_name / "Cargo.toml").is_file(), f"{name}.{key}"
