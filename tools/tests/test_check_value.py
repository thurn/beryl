import argparse
import json
from collections.abc import Iterable
from datetime import datetime, timedelta, timezone

import pytest

from beryl_tools import check_value, runlog, verdicts

NOW: datetime = datetime.now(timezone.utc)
SINCE: datetime = NOW - timedelta(days=30)


def record(
    check: str,
    commit: str,
    outcome: str = "pass",
    cpu: float | None = 1.0,
    seconds: float = 1.0,
    **more: object,
) -> dict[str, object]:
    r: dict[str, object] = {
        "time": f"{commit}-{check.split(':')[0]}",
        "host": "mac",
        "commit": commit,
        "script": "ci",
        "lane": check.split(":")[0],
        "check": check,
        "outcome": outcome,
        "seconds": seconds,
        **more,
    }
    if cpu is not None:
        r["cpu_seconds"] = cpu
    return r


def catch(commit: str, days_ago: float = 1) -> verdicts.Entry:
    when = (NOW - timedelta(days=days_ago)).isoformat(timespec="seconds")
    return {"time": when, "verdict": "catch", "commit": commit}


def by_name(scores: check_value.Scores) -> dict[tuple[str, ...], check_value.Group]:
    return {g.checks: g for g in scores.groups}


def test_value_is_catches_per_cpu_second_lowest_first() -> None:
    records = [record("rust:cheap", c, cpu=0.5) for c in ("a", "b", "c")] + [
        record("rust:costly", c, cpu=4.0) for c in ("a", "b", "c")
    ]
    records += [record("rust:never", c, cpu=2.0) for c in ("a", "b", "c")]
    history = {
        "rust:cheap": [catch("x")],
        "rust:costly": [catch("y"), catch("z"), catch("old", days_ago=60)],
    }
    scores = check_value.score(records, history, SINCE)
    assert [g.checks[0] for g in scores.groups] == [
        "rust:never",
        "rust:costly",
        "rust:cheap",
    ]
    groups = by_name(scores)
    assert groups[("rust:costly",)].catches == 2
    assert groups[("rust:costly",)].value() == pytest.approx(0.5)
    assert groups[("rust:cheap",)].value() == pytest.approx(2.0)


def test_checks_that_always_fail_together_are_one_group() -> None:
    variants = ("verb:appear/yours", "verb:appear/theirs")
    records: list[dict[str, object]] = []
    for c in ("a", "b", "c", "d"):
        failing = c in ("b", "d")
        records += [
            record(v, c, "fail" if failing else "pass", cpu=2.0) for v in variants
        ]
        # A check that failed once with them is not one of them.
        records.append(record("verb:other", c, "fail" if c == "b" else "pass"))
    history = {v: [catch("b"), catch("d")] for v in variants}
    groups = by_name(check_value.score(records, history, SINCE))
    together = groups[variants[::-1]]
    assert together.cpu == pytest.approx(4.0)
    # A bug they caught together counts once.
    assert together.catches == 2
    assert ("verb:other",) in groups


def test_a_run_broken_as_a_whole_groups_nothing() -> None:
    records: list[dict[str, object]] = []
    for c in ("a", "b", "c"):
        for n in range(check_value.MASS + 1):
            records.append(record(f"ui-test:t{n}", c, "fail" if c != "a" else "pass"))
    groups = check_value.score(records, {}, SINCE).groups
    assert len(groups) == check_value.MASS + 1


def test_cost_without_cpu_is_estimated_from_its_lanes_ratio() -> None:
    records = [record("rust:measured", c, cpu=3.0, seconds=1.0) for c in ("a", "b")]
    records += [record("rust:old", c, cpu=None, seconds=2.0) for c in ("a", "b")]
    groups = by_name(check_value.score(records, {}, SINCE))
    assert groups[("rust:old",)].cpu == pytest.approx(6.0)
    assert groups[("rust:old",)].estimated
    assert not groups[("rust:measured",)].estimated


def test_a_steps_own_failure_record_is_not_a_check() -> None:
    records = [record("rust:a::t", c) for c in ("a", "b")]
    # The step failed though no test did: its own record, beside its tests.
    records.append(record("rust:tests", "b", "fail", cpu=50.0, whole=True))
    # From before the mark: it never passed.
    records.append(record("shots:ui-shots", "c", "fail", cpu=40.0))
    # A step that reports no checks is its own check.
    records += [record("python:pytest", c, cpu=5.0) for c in ("a", "b")]
    groups = by_name(check_value.score(records, {}, SINCE))
    assert set(groups) == {("rust:a::t",), ("python:pytest",)}


def test_cuts_take_the_lowest_value_until_a_full_run_fits() -> None:
    records: list[dict[str, object]] = []
    for c in ("a", "b", "c"):
        records += [
            record("rust:keep", c, cpu=10.0),
            record("rust:big", c, cpu=30.0),
            record("rust:small", c, cpu=5.0),
        ]
    history = {"rust:keep": [catch("x")]}
    scores = check_value.score(records, history, SINCE)
    assert check_value.cuts(scores, 45.0) == []
    assert [g.checks for g in check_value.cuts(scores, 20.0)] == [("rust:big",)]
    assert [g.checks for g in check_value.cuts(scores, 12.0)] == [
        ("rust:big",),
        ("rust:small",),
    ]


def test_a_sampled_lane_costs_one_run_and_none_of_its_cards_is_cut() -> None:
    # Each commit samples another card, at 10 CPU seconds a run: a full run plays one sample,
    # not the three cards the runs sampled between them.
    records = [record("rust:a", c, cpu=5.0) for c in ("a", "b", "c")]
    records += [
        record(f"oracle:{card}/x/xmage", c, cpu=10.0)
        for c, card in (("a", "One"), ("b", "Two"), ("c", "Three"))
    ]
    scores = check_value.score(records, {}, SINCE)
    assert check_value.full_run(scores, set()) == 15.0
    assert [g.checks for g in check_value.cuts(scores, 12.0)] == [("rust:a",)]


def test_bisections_and_the_fast_check_are_left_out() -> None:
    records = [
        record("rust:t", "a"),
        {**record("rust:t", "b", "fail"), "script": "bisect"},
        {**record("rust:fast", "a"), "script": "check"},
    ]
    scores = check_value.score(records, {}, SINCE)
    assert [g.checks for g in scores.groups] == [("rust:t",)]
    assert scores.groups[0].runs == 1


class Store(verdicts.Beads):
    def __init__(self, history: dict[str, list[verdicts.Entry]]) -> None:
        self.found = history

    def checks(self) -> list[str]:
        return list(self.found)

    def history(self, checks: Iterable[str]) -> dict[str, list[verdicts.Entry]]:
        return {c: e for c, e in self.found.items() if c in set(checks)}


def test_the_command_reports_groups_and_cuts(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    stamp = NOW.isoformat(timespec="seconds")
    runlog.append(
        {**record("rust:a", c, cpu=4.0), "time": stamp} for c in ("x", "y", "z")
    )
    monkeypatch.setattr(verdicts, "STORE", Store({}))
    args = argparse.Namespace(days=30, json=True, budget=1.0)
    assert check_value.report(args) == 0
    out = json.loads(capsys.readouterr().out)
    assert out["runs"] == 3 and out["full_run_cpu_seconds"] == 4.0
    assert [g["checks"] for g in out["cuts"]] == [["rust:a"]]
