"""The Rust families' adapter for Ignite: how it reads each tool's output into one result per
check, and that it imports none of the orchestration the switch deletes."""

import json
import subprocess
import sys
from pathlib import Path

import pytest

from beryl_tools import ignite
from beryl_tools.ignite import rust
from beryl_tools.render_diff import Diff

FORBIDDEN: list[str] = str(
    "tokens lanes step_runner selection rust_fit python_fit runlog runstore verdicts budget "
    "hang flaky bisect slow_ci sweep nightly warm caches incidents deadline clock check_value "
    "checks"
).split()


def test_the_adapter_imports_no_orchestration() -> None:
    code = (
        "import sys, beryl_tools.ignite.rust; "
        "print(' '.join(m for m in sys.modules if m.startswith('beryl_tools.')))"
    )
    loaded = subprocess.run(
        [sys.executable, "-c", code], capture_output=True, text=True, check=True
    ).stdout.split()
    assert not [m for m in loaded if m.removeprefix("beryl_tools.") in FORBIDDEN]


def message(reason: str, package_id: str, **fields: object) -> str:
    return json.dumps({"reason": reason, "package_id": package_id, **fields})


KERNEL = "path+file:///w/engine/crates/beryl_kernel#0.1.0"
RULES = "path+file:///w/engine/crates/beryl_rules#beryl_rules@0.1.0"
BUILD_FINISHED: str = json.dumps({"reason": "build-finished", "success": True})


def test_clippy_fails_a_package_with_a_warning_and_passes_one_without() -> None:
    warning = {
        "level": "warning",
        "message": "needless borrow",
        "rendered": "warning: needless borrow\n",
        "spans": [
            {
                "is_primary": True,
                "file_name": "crates/beryl_rules/src/a.rs",
                "line_start": 7,
            }
        ],
    }
    output = "\n".join(
        [
            message("compiler-artifact", KERNEL),
            message("compiler-message", RULES, message=warning),
            "not json",
            BUILD_FINISHED,
        ]
    )
    results = rust.clippy_results(
        output, ["beryl_kernel", "beryl_rules", "beryl_cli"], 0
    )
    assert results == [
        ignite.Result("clippy:beryl_kernel", "pass"),
        ignite.Result(
            "clippy:beryl_rules",
            "fail",
            "warning: needless borrow",
            "engine/crates/beryl_rules/src/a.rs:7",
        ),
        ignite.Result(
            "clippy:beryl_cli",
            "incomplete",
            "cargo clippy exited with status 0 without reporting an artifact for this package",
        ),
    ]


def test_a_note_alone_is_no_failure() -> None:
    note = {"level": "note", "message": "for more information", "spans": []}
    output = "\n".join(
        [
            message("compiler-artifact", KERNEL),
            message("compiler-message", KERNEL, message=note),
            BUILD_FINISHED,
        ]
    )
    assert rust.clippy_results(output, ["beryl_kernel"], 0) == [
        ignite.Result("clippy:beryl_kernel", "pass")
    ]


@pytest.mark.parametrize(
    ("returncode", "finished", "evidence"),
    [
        (0, "", "without a successful build-finished message"),
        (1, BUILD_FINISHED, "status 1 before confirming the package result"),
    ],
)
def test_clippy_requires_a_successful_build_to_report_a_pass(
    returncode: int, finished: str, evidence: str
) -> None:
    output = "\n".join([message("compiler-artifact", KERNEL), finished])
    result = rust.clippy_results(output, ["beryl_kernel"], returncode)[0]
    assert result.outcome == "incomplete"
    assert evidence in result.message


def test_doctests_pass_only_if_every_batch_passes() -> None:
    both_ok = "running 2 tests\ntest result: ok. 2 passed\n\nrunning 1 test\ntest result: ok. 1 passed\n"
    second_failed = "running 2 tests\ntest result: ok. 2 passed\n\nrunning 1 test\ntest result: FAILED. 0 passed; 1 failed\n"
    assert rust.doctest_result(both_ok, "beryl_card") == ignite.Result(
        "doctests:beryl_card", "pass"
    )
    failed = rust.doctest_result(second_failed, "beryl_card")
    assert failed is not None and failed.outcome == "fail"
    assert rust.doctest_result("error: could not compile", "beryl_card") is None


def test_has_doctest_skips_text_blocks() -> None:
    assert rust.has_doctest("/// ```\n/// let a = 1;\n/// ```\n")
    assert not rust.has_doctest("/// ```text\n/// not rust\n/// ```\n")


def test_unformatted_names_each_file_once() -> None:
    output = "Diff in /w/rust/a.rs:12:\n-x\n+y\nDiff in /w/rust/a.rs:40:\nDiff in /w/rust/b.rs at line 3:\n"
    assert rust.unformatted(output) == {
        Path("/w/rust/a.rs"): "12",
        Path("/w/rust/b.rs"): "3",
    }


def test_allocation_binaries_by_package() -> None:
    listing = {
        "rust-binaries": {
            "beryl_search::allocations": {
                "binary-id": "beryl_search::allocations",
                "binary-name": "allocations",
                "binary-path": "/t/debug/deps/allocations-1",
            },
            "beryl_search": {
                "binary-id": "beryl_search",
                "binary-name": "beryl_search",
                "binary-path": "/t/debug/deps/beryl_search-2",
            },
        }
    }
    assert rust.allocation_binaries(json.dumps(listing)) == {
        "beryl_search": "/t/debug/deps/allocations-1"
    }


MATCH = Diff(lines=[], causes=[])
WORDING = Diff(lines=["Card: draw [-a-] {+one+} card ."], causes=[])


def test_card_text_reports_each_card_file() -> None:
    named = {"card-text:c/s/match": "Match", "card-text:c/s/off": "Off"}
    diffs = {"Match": MATCH, "Off": WORDING}
    stale = {"reason": "wording", "note": "n", "diff": []}
    results = rust.card_text_results(
        named,
        diffs,
        {"Gone": stale},
        {},
        [*named, "card-text:c/s/unknown", rust.ALLOWLIST_CHECK],
    )
    outcomes = {r.check: r.outcome for r in results}
    assert outcomes == {
        "card-text:c/s/match": "pass",
        "card-text:c/s/off": "fail",
        "card-text:c/s/unknown": "fail",
        rust.ALLOWLIST_CHECK: "fail",
    }
    assert "Gone has an entry but no card file" in results[-1].message


def test_report_appends_to_the_results_file(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    target = tmp_path / "results.jsonl"
    monkeypatch.setenv(ignite.RESULTS_ENV, str(target))
    ignite.report([ignite.Result("clippy:a", "pass")])
    ignite.report([ignite.Result("clippy:b", "fail", "bad", "x.rs:1")])
    lines = [json.loads(line) for line in target.read_text().splitlines()]
    assert lines == [
        {"check": "clippy:a", "outcome": "pass"},
        {"check": "clippy:b", "outcome": "fail", "message": "bad", "at": "x.rs:1"},
    ]


def test_clippy_uses_target_rustflags_on_windows() -> None:
    expected = (
        'target.x86_64-pc-windows-msvc.rustflags=["--cap-lints=warn"]'
        if sys.platform == "win32"
        else 'build.rustflags=["--cap-lints=warn"]'
    )
    assert expected in rust.CLIPPY
