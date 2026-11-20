"""The counts, fuzz, exhaust, scale, ignite and ignite-lint check families: their manifest and their adapter."""

from __future__ import annotations

import json
import re
import stat
import tomllib
from pathlib import Path

import pytest

from beryl_tools import count_trend, paths
from beryl_tools.ignite import rest as adapter

CHECKS: Path = paths.REPO / "ignite" / "checks"
MANIFEST: Path = CHECKS / "rest.toml"
FORBIDDEN: list[str] = [
    name
    for name in (
        "tokens lanes step_runner selection rust_fit python_fit runlog runstore verdicts budget "
        "hang flaky bisect slow_ci sweep nightly warm caches incidents deadline clock "
        "check_value checks"
    ).split()
]


def manifest() -> dict[str, dict[str, dict[str, object]]]:
    return tomllib.loads(MANIFEST.read_text())


def test_the_manifest_declares_the_families_and_products() -> None:
    declared = manifest()
    assert set(declared["family"]) == {
        "counts",
        "fuzz",
        "exhaust",
        "scale",
        "iroh",
        "ignite",
        "ignite-platform",
        "ignite-lint",
    }
    assert set(declared["product"]) == {
        "fuzz-tests",
        "exhaust-tests",
        "ignite-tests",
        "iroh-tests",
    }
    for name, family in declared["family"].items():
        assert family["inputs"], name
        if family["adapter"] == "jsonl":
            assert f"rest list {name}" in str(family["list"])
            assert "{checks}" in str(family["run"])
    counts = declared["family"]["counts"]
    assert counts["platforms"] == ["macos"] and counts["resources"] == ["bench-vm"]
    for name in ("fuzz", "exhaust"):
        family = declared["family"][name]
        assert family["kind"] == "exploration"
        assert f"rest next {name}" in str(family["next"])
    assert declared["family"]["iroh"]["adapter"] == "nextest"
    # The release branch waits on the `ignite` family (ignite.toml's `release_family`), so it
    # must hold Ignite's tests, and they must reuse the `ignite-tests` build.
    ignite = declared["family"]["ignite"]
    assert ignite["adapter"] == "nextest" and ignite["needs"] == ["ignite-tests"]
    assert "--workspace" in str(ignite["list"]) and "--workspace" in str(ignite["run"])


def test_the_adapter_imports_no_orchestration_module() -> None:
    source = Path(adapter.__file__).read_text()
    for name in FORBIDDEN:
        assert not re.search(rf"^\s*from \.+ import .*\b{name}\b", source, re.M), name
        assert f"beryl_tools.{name}" not in source


def test_count_trend_imports_no_ticket_orchestration() -> None:
    source = Path(count_trend.__file__).read_text()
    assert "from . import paths\n" in source
    # `verdicts` is imported only inside the function that serves today's CI.
    assert not re.search(r"^from \. import .*verdicts", source, re.M)


def matches(pattern: str, path: str) -> bool:
    """Whether a path input of a manifest covers `path`: a glob, a file or a directory."""
    if pattern.startswith("crate:") or pattern == "crate-closure":
        return False
    if pattern.endswith("/**"):
        return path.startswith(pattern[:-2])
    return path == pattern or path.startswith(pattern.rstrip("/") + "/")


def test_a_change_under_ignite_selects_only_ignites_families() -> None:
    declared = [tomllib.loads(file.read_text()) for file in CHECKS.glob("*.toml")]
    products = {n: p for d in declared for n, p in d.get("product", {}).items()}
    selected: set[str] = set()
    for name, family in (
        (n, f) for d in declared for n, f in d.get("family", {}).items()
    ):
        # `crate-closure` names the crates of the workspace that the family's products build in.
        dirs = {
            products[p].get("dir") for p in family.get("needs", []) if p in products
        }
        if any(
            matches(str(i), "ignite/crates/ignite_core/src/lib.rs")
            or (i == "crate-closure" and "ignite" in dirs)
            for i in family["inputs"]
        ):
            selected.add(name)
    assert selected == {"ignite", "ignite-platform", "ignite-lint"}


def card(name: str) -> str:
    return (
        "// Generated from the card store by `just card header`.\n"
        f"//! {name}\n//! Creature\n"
        "// End of the generated card header.\n"
    )


def repository(root: Path) -> None:
    cards = root / "rust" / "cards" / "cards_1" / "set"
    cards.mkdir(parents=True)
    (cards / "alpha.rs").write_text(card("Alpha"))
    (cards / "beta_gamma.rs").write_text(card("Beta Gamma"))
    gate = root / adapter.GATE_FILE
    gate.parent.mkdir(parents=True)
    gate.write_text(
        'benchmark = "b"\ninstructions = 1\n[family.blank]\nratio = 1.25\n'
        "[family.layers]\nratio = 1.25\n"
    )


def test_counts_lists_the_rollouts_and_each_family_with_a_bar(tmp_path: Path) -> None:
    repository(tmp_path)
    assert [c["check"] for c in adapter.list_checks("counts", tmp_path)] == [
        "counts:rollouts",
        "counts:family/blank",
        "counts:family/layers",
    ]


def test_the_ignite_lint_family_lists_fmt_and_clippy(tmp_path: Path) -> None:
    assert [c["check"] for c in adapter.list_checks("ignite-lint", tmp_path)] == [
        "ignite-lint:fmt",
        "ignite-lint:clippy",
    ]


def test_a_manifest_from_before_the_lint_split_still_lists_its_three_checks(
    tmp_path: Path,
) -> None:
    # The daemon reads the main checkout's manifest, which may still declare the `ignite`
    # family as this adapter's while a tree holds the newer adapter.
    assert [c["check"] for c in adapter.list_checks("ignite", tmp_path)] == [
        "ignite:fmt",
        "ignite:clippy",
        "ignite:test",
    ]


def test_fuzz_batches_never_overlap(tmp_path: Path) -> None:
    state = tmp_path / "state" / "fuzz.json"
    batches: list[list[tuple[int, int]]] = []
    for _ in range(3):
        batch = [adapter.parse_fuzz(c["check"]) for c in adapter.next_fuzz(state)]
        assert {target for target, _, _ in batch} == set(adapter.FUZZ_TARGETS)
        batches.append([(first, first + games) for _, first, games in batch])
    # A batch's checks start at one seed, and the next batch starts after all of them.
    for earlier, later in zip(batches, batches[1:]):
        assert max(end for _, end in earlier) <= min(first for first, _ in later)
    assert batches[0][0][0] % 4 == 0 and adapter.WIDTH % 4 == 0
    # The state is kept on disk: a later process continues where the last batch ended.
    again = adapter.next_fuzz(state)
    assert adapter.parse_fuzz(again[0]["check"])[1] == batches[-1][0][0] + adapter.WIDTH


def test_no_fuzz_batch_plays_more_seeds_than_it_reserved() -> None:
    assert max(games for _, games in adapter.FUZZ_TARGETS.values()) <= adapter.WIDTH


def test_exhaust_yields_a_check_per_card_with_the_card_file_as_input(
    tmp_path: Path,
) -> None:
    repository(tmp_path)
    found = adapter.next_exhaust(tmp_path)
    assert found == [
        {
            "check": "exhaust:alpha/depth6-nodes20000",
            "inputs": ["rust/cards/cards_1/set/alpha.rs"],
        },
        {
            "check": "exhaust:beta-gamma/depth6-nodes20000",
            "inputs": ["rust/cards/cards_1/set/beta_gamma.rs"],
        },
    ]
    assert adapter.card_names(tmp_path)["beta-gamma"] == "Beta Gamma"
    assert adapter.parse_exhaust(found[1]["check"]) == ("beta-gamma", 6, 20000)


def test_a_regression_family_has_no_next(tmp_path: Path) -> None:
    with pytest.raises(ValueError):
        adapter.next_checks("counts", tmp_path)


def test_scale_results_are_named_for_the_family() -> None:
    line = json.dumps(
        {"check": "pytest:tools/tests/test_scale.py::t", "outcome": "pass"}
    )
    assert adapter.renamed([line]) == [
        {"check": "scale:tools/tests/test_scale.py::t", "outcome": "pass"}
    ]


# --- the count gate ---

GATE = """\
#!/bin/sh
# A stand-in for `count-gate`: reads its arguments and answers as FAKE_GATE says.
case "$FAKE_GATE" in
cannot) echo "no beryl-bench VM: run just bench-count once" >&2; exit 125 ;;
fail)
  [ "$1" = "--measure" ] && echo '{"measured": 1010, "bar": 1000, "ok": false, "message": "over"}' > "$2"
  exit 1 ;;
*)
  [ "$1" = "--measure" ] && echo '{"measured": 995, "bar": 1000, "ok": true, "message": "ok"}' > "$2"
  exit 0 ;;
esac
"""


@pytest.fixture
def gate(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    script = tmp_path / "bin" / "gate"
    script.parent.mkdir()
    script.write_text(GATE)
    script.chmod(script.stat().st_mode | stat.S_IEXEC)
    monkeypatch.setattr(
        adapter, "count_gate_command", lambda *args: [str(script), *args]
    )
    return script


def lines(results: Path) -> list[dict[str, str]]:
    return [json.loads(t) for t in results.read_text().splitlines()]


def test_a_passing_count_is_a_pass_and_never_files_when_it_is_not_a_tip(
    tmp_path: Path, gate: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.delenv("FAKE_GATE", raising=False)
    monkeypatch.delenv("IGNITE_CLASS", raising=False)
    trended: list[tuple[str | None, int, int, bool]] = []
    monkeypatch.setattr(
        count_trend,
        "report_at",
        lambda head, measured, bar, ok, store: trended.append((head, measured, bar, ok))
        or [],
    )
    results = tmp_path / "results.jsonl"
    adapter.run_counts([adapter.ROLLOUTS], tmp_path, results)
    assert lines(results) == [{"check": "counts:rollouts", "outcome": "pass"}]
    assert trended == [(None, 995, 1000, True)]


def test_a_count_over_the_bar_fails_with_the_gates_message(
    tmp_path: Path, gate: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("FAKE_GATE", "fail")
    monkeypatch.setattr(count_trend, "report_at", lambda *a, **k: [])
    results = tmp_path / "results.jsonl"
    adapter.run_counts([adapter.ROLLOUTS], tmp_path, results)
    assert lines(results) == [
        {"check": "counts:rollouts", "outcome": "fail", "message": "over"}
    ]


def test_a_gate_that_cannot_run_is_incomplete_with_the_reason(
    tmp_path: Path, gate: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("FAKE_GATE", "cannot")
    results = tmp_path / "results.jsonl"
    adapter.run_counts([adapter.ROLLOUTS, "counts:family/blank"], tmp_path, results)
    found = lines(results)
    assert [(r["check"], r["outcome"]) for r in found] == [
        ("counts:rollouts", "incomplete"),
        ("counts:family/blank", "incomplete"),
    ]
    assert all(r["message"].startswith("cannot-run: no beryl-bench VM") for r in found)


def test_a_family_check_runs_the_gate_for_that_family(
    tmp_path: Path, gate: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.delenv("FAKE_GATE", raising=False)
    results = tmp_path / "results.jsonl"
    adapter.run_counts(["counts:family/layers"], tmp_path, results)
    assert lines(results) == [{"check": "counts:family/layers", "outcome": "pass"}]


# --- the trend files its tickets through `ignite ticket` ---

IGNITE = """\
#!/bin/sh
echo "$@" >> "$IGNITE_LOG"
case "$2" in
list) echo '[]' ;;
create) echo "Tnew1" ;;
esac
"""


def test_the_trend_files_a_count_creep_ticket_and_fails_nothing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    script = tmp_path / "ignite"
    script.write_text(IGNITE)
    script.chmod(script.stat().st_mode | stat.S_IEXEC)
    log = tmp_path / "log"
    monkeypatch.setenv("IGNITE_BIN", str(script))
    monkeypatch.setenv("IGNITE_LOG", str(log))
    history = tmp_path / "counts.jsonl"
    history.write_text(
        count_trend.dump(count_trend.Record("old", 1_000_000, 1_100_000)) + "\n"
    )
    monkeypatch.setattr(count_trend, "history_path", lambda: history)
    monkeypatch.setattr(
        count_trend, "proper_ancestor", lambda head: lambda c: c == "old"
    )
    monkeypatch.setattr(
        count_trend, "rust_commits", lambda old, new: ["abc1234 feat: x"]
    )
    said = count_trend.report_at(
        "newcommit", 1_005_000, 1_100_000, True, adapter.IgniteTickets()
    )
    assert any("count creep" in line and "filed Tnew1" in line for line in said)
    calls = log.read_text().splitlines()
    assert calls[0].startswith("ticket list --label count-creep")
    assert calls[1].startswith("ticket create Win back the rollout benchmark")
    assert "-p P2" in calls[1] and "--labels count-creep,tier:opus" in calls[1]
    # The verified commit's count is stored.
    assert count_trend.load(history.read_text())[-1] == count_trend.Record(
        "newcommit", 1_005_000, 1_100_000
    )


def test_a_missing_ticket_client_files_nothing_and_raises_nothing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("IGNITE_BIN", str(tmp_path / "absent"))
    tickets = adapter.IgniteTickets()
    assert tickets.list_issues("--label", "count-creep") == []
    assert tickets.create_issue(title="t", body="b", priority="2", labels="x")[0] != 0


def test_only_a_clean_tip_tree_stores_a_count(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("IGNITE_CLASS", "land")
    assert adapter.verified_commit(tmp_path) is None
    monkeypatch.setenv("IGNITE_CLASS", "tip")
    monkeypatch.setattr(adapter, "git", lambda root, *args: "")
    assert adapter.verified_commit(tmp_path) is None  # No commit to name.
    monkeypatch.setattr(
        adapter, "git", lambda root, *args: "dirty" if args[0] == "status" else "abc"
    )
    assert adapter.verified_commit(tmp_path) is None
    monkeypatch.setattr(
        adapter, "git", lambda root, *args: "" if args[0] == "status" else "abc"
    )
    assert adapter.verified_commit(tmp_path) == "abc"


def test_judged_follows_cargos_exit_codes() -> None:
    assert adapter.judged(0, "", "x") == {"outcome": "pass"}
    assert adapter.judged(101, "thread panicked at seed 7", "x") == {
        "outcome": "fail",
        "message": "thread panicked at seed 7",
    }
    assert adapter.judged(137, "", "x") is None
