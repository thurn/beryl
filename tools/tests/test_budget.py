"""Over-budget checks file beads and fail nothing (budget.py): the fast check one per overrun,
slow CI one per lane naming the checks to cut."""

from __future__ import annotations

import json
from datetime import datetime

from beryl_tools import budget, check_value, verdicts


def labels(issue: dict[str, object]) -> list[str]:
    found = issue.get("labels")
    return [str(label) for label in found] if isinstance(found, list) else []


class FakeBd(verdicts.Beads):
    """bd as budget.py uses it, in memory."""

    def __init__(self) -> None:
        self.issues: list[dict[str, object]] = []

    def bd(self, *args: str) -> tuple[int, str]:
        if args[:2] == ("list", "--label"):
            return 0, json.dumps([i for i in self.issues if args[2] in labels(i)])
        if args[0] == "create":
            flag = lambda name: args[args.index(name) + 1]  # noqa: E731
            bead = f"by-{len(self.issues) + 1}"
            self.issues.append(
                {
                    "id": bead,
                    "status": "open",
                    "title": flag("--title"),
                    "description": flag("--description"),
                    "labels": flag("--labels").split(","),
                    "metadata": json.loads(flag("--metadata")),
                }
            )
            return 0, bead + "\n"
        return 1, ""


LANES = ["rust 120 s (fmt 1 s, clippy 1 s, tests 118 s)", "repo 2 s (just 0 s)"]


def test_an_overrun_files_a_bead_naming_its_lanes() -> None:
    store = FakeBd()
    line = budget.report("check", 60, "191 s", LANES, "load average 6", store)
    assert line == "check: over budget: filed by-1 to audit it"
    [issue] = store.issues
    assert issue["metadata"] == {budget.SCRIPT_KEY: "check"}
    assert budget.LABEL in labels(issue)
    assert "tests 118 s" in str(issue["description"])


def test_an_open_bead_takes_later_overruns_of_its_script() -> None:
    store = FakeBd()
    budget.report("check", 60, "191 s", LANES, "", store)
    again = budget.report("check", 60, "200 s", LANES, "", store)
    assert again == "check: over budget: by-1 is open to audit it"
    budget.report("ci", 300, "400 s", LANES, "", store)
    assert [i["id"] for i in store.issues] == ["by-1", "by-2"]


def test_a_closed_bead_lets_the_next_overrun_file_again() -> None:
    store = FakeBd()
    budget.report("check", 60, "191 s", LANES, "", store)
    store.issues[0]["status"] = "closed"
    budget.report("check", 60, "191 s", LANES, "", store)
    assert len(store.issues) == 2


def run(i: int, checks: dict[str, float], host: str = "mac") -> list[dict[str, object]]:
    """Slow CI's records of its `i`th run, a minute after the last, of `checks` at their CPU
    seconds."""
    records: list[dict[str, object]] = []
    for check, spent in checks.items():
        record: dict[str, object] = {
            "time": f"2026-11-13T10:{i:02}:00+00:00",
            "host": host,
            "commit": f"c{i}",
            "script": "ci",
            "lane": check.split(":")[0],
            "check": check,
            "outcome": "pass",
            "seconds": spent,
            "cpu_seconds": spent,
        }
        records.append(record)
    return records


CHECKS = {
    "rust:costly": 50.0,
    "rust:cheap": 1.0,
    "ui-test:drag": 30.0,
    "ui-test:caught": 20.0,
    "app:smoke": 100.0,
}
CAUGHT: dict[str, list[verdicts.Entry]] = {
    "ui-test:caught": [
        {"time": "2026-11-12T00:00:00+00:00", "verdict": "catch", "commit": "x"}
    ]
}


def runs(
    first: int = 0, count: int = budget.RUNS, **more: float
) -> list[dict[str, object]]:
    return [r for i in range(first, first + count) for r in run(i, {**CHECKS, **more})]


def slow(store: FakeBd, records: list[dict[str, object]], cpu: float) -> list[str]:
    return budget.slow_report(records, CAUGHT, store, cpu, host="mac")


def test_slow_ci_over_its_cpu_budget_files_a_bead_per_lane_naming_the_cuts() -> None:
    store = FakeBd()
    # 201 CPU s a run; cutting `rust:costly` and `ui-test:drag` leaves 121.
    lines = slow(store, runs() + run(99, {"rust:costly": 9999.0}, host="pc"), 125)
    assert lines[0] == "ci budget: a full run uses 201 CPU seconds, over 125"
    rust, ui = store.issues
    assert rust["title"] == "Cut 1 checks from slow CI's rust lane"
    assert rust["metadata"] == {budget.LANE_KEY: "rust"}
    assert budget.CUT_LABEL in labels(rust)
    assert "- delete: `rust:costly`: 10 runs, 0 catches, 50.00 CPU s" in str(
        rust["description"]
    )
    assert "rust:cheap`:" not in str(rust["description"])
    assert ui["title"] == "Cut 1 checks from slow CI's ui-test lane"
    # The app is costlier than anything named, but no cut bead names it.
    assert "- delete: `app:smoke`" not in str(rust["description"]) + str(
        ui["description"]
    )


def test_a_check_that_caught_a_bug_is_moved_to_the_nightly() -> None:
    store = FakeBd()
    slow(store, runs(), 90)
    ui = str(store.issues[-1]["description"])
    assert "- move to the nightly: `ui-test:caught`: 10 runs, 1 catches" in ui
    assert "- delete: `ui-test:drag`" in ui


def test_slow_ci_under_its_budget_files_nothing() -> None:
    store = FakeBd()
    assert slow(store, runs(), 300) == [
        "ci budget: a full run uses 201 of 300 CPU seconds"
    ]
    assert store.issues == []


def test_one_noisy_run_is_not_the_median() -> None:
    store = FakeBd()
    records = runs() + run(budget.RUNS, {**CHECKS, "rust:costly": 5000.0})
    assert slow(store, records, 250)[0].endswith("of 250 CPU seconds")


def test_an_open_cut_bead_holds_back_the_next() -> None:
    store = FakeBd()
    slow(store, runs(), 125)
    filed = len(store.issues)
    assert slow(store, runs(), 125) == [
        "ci budget: by-1 by-2 open to cut checks; nothing more filed"
    ]
    assert len(store.issues) == filed


def test_after_a_cut_closes_only_runs_since_count() -> None:
    store = FakeBd()
    slow(store, runs(), 125)
    for issue in store.issues:
        issue["status"] = "closed"
        issue["closed_at"] = "2026-11-13T10:09:30Z"
    # The cut checks are gone from the runs after the close; until there are enough of those,
    # the runs from before it would only name the same checks again.
    gone = {c: s for c, s in CHECKS.items() if c not in ("rust:costly", "ui-test:drag")}
    after = [r for i in range(10, 13) for r in run(i, gone)]
    assert slow(store, runs() + after, 100) == [
        "ci budget: 3 of 10 runs since a cut bead closed at 2026-11-13T10:09:30+00:00;"
        " not measured yet"
    ]
    after = [r for i in range(10, 20) for r in run(i, gone)]
    slow(store, runs() + after, 100)
    rust, ui = (str(i["description"]) for i in store.issues[2:])
    assert "- delete: `rust:cheap`" in rust and "rust:costly" not in rust
    assert "- move to the nightly: `ui-test:caught`" in ui and "ui-test:drag" not in ui


def test_too_few_runs_measure_nothing() -> None:
    store = FakeBd()
    assert (
        slow(store, runs(count=3), 1)[0] == "ci budget: 3 of 10 runs; not measured yet"
    )


def test_a_full_run_sums_each_checks_median_over_the_checks_still_run() -> None:
    # Slow CI ran the ui-test lane in one run of three: a full run still counts it once.
    records = run(0, {"rust:a": 10.0, "ui-test:b": 40.0}) + run(1, {"rust:a": 12.0})
    records += run(2, {"rust:a": 11.0})
    scores = check_value.score(records, {}, datetime.fromisoformat("2026-10-01T00:00Z"))
    assert check_value.full_run(scores, set()) == 51.0
    assert check_value.full_run(scores, set(), last=1) == 51.0
    assert check_value.full_run(scores, {"ui-test:b"}) == 11.0
