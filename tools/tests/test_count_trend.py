from pathlib import Path
from typing import Callable

import pytest

from beryl_tools import count_trend
from beryl_tools.count_trend import Record

BAR = 1_000_000


def ancestors(*commits: str) -> Callable[[str], bool]:
    return lambda c: c in commits


def test_records_round_trip_and_bad_lines_are_skipped() -> None:
    r = Record("abc", 1_001_000, BAR)
    assert count_trend.load(count_trend.dump(r) + "\nnot json\n{}\n") == [r]


def test_a_quiet_change_files_nothing() -> None:
    records = [Record("a", 1_000_000, BAR)]
    f = count_trend.judge(records, 1_002_000, BAR, ancestors("a"))
    assert f.previous == records[0] and f.creep is None and f.lower is None
    assert f.step is not None and round(f.step, 2) == 0.2


def test_one_range_adding_more_than_the_step_is_creep() -> None:
    records = [Record("a", 1_000_000, BAR)]
    f = count_trend.judge(records, 1_003_000, BAR, ancestors("a"))
    assert f.creep is not None and "+0.30% since a" in f.creep


def test_small_steps_that_add_up_past_the_drift_are_creep() -> None:
    records = [
        Record("a", 995_000, BAR),
        Record("b", 997_000, BAR),
        Record("c", 999_000, BAR),
    ]
    f = count_trend.judge(records, 1_001_000, BAR, ancestors("a", "b", "c"))
    assert f.creep is not None and "lowest count" in f.creep


def test_the_floor_counts_only_measurements_under_the_same_bar() -> None:
    records = [Record("a", 900_000, 950_000), Record("b", 999_000, BAR)]
    f = count_trend.judge(records, 1_001_000, BAR, ancestors("a", "b"))
    assert f.floor == 999_000 and f.creep is None


def test_only_an_ancestor_is_compared_with() -> None:
    records = [Record("a", 1_000_000, BAR), Record("branch", 900_000, 900_000)]
    f = count_trend.judge(records, 1_001_000, BAR, ancestors("a"))
    assert f.previous == records[0]


def test_a_drop_past_the_lower_threshold_asks_for_a_lower_bar() -> None:
    assert count_trend.judge([], 994_000, BAR, ancestors()).lower is not None
    assert count_trend.judge([], 996_000, BAR, ancestors()).lower is None


class TicketStore:
    def __init__(self, history: Path) -> None:
        self.history = history
        self.tickets: dict[str, dict[str, object]] = {}
        self.closed: dict[str, str] = {}

    def list_issues(self, *filters: str, include_closed: bool = False) -> object:
        return [
            ticket
            for ticket in self.tickets.values()
            if isinstance(labels := ticket["labels"], list) and filters[1] in labels
        ]

    def add_comment(self, issue: str, comment: str) -> tuple[int, str]:
        return 0, ""

    def create_issue(
        self, *, title: str, body: str, priority: str, labels: str
    ) -> tuple[int, str]:
        issue = f"T{len(self.tickets) + 1}"
        self.tickets[issue] = {
            "id": issue,
            "body": body,
            "labels": labels.split(","),
            "status": "open",
            "created_by": "ignite",
            "comments": [{"author": "ignite"}],
            "history": [],
        }
        return 0, issue

    def inspect_issues(self, *issues: str) -> tuple[int, list[dict[str, object]]]:
        return 7, [self.tickets[issue] for issue in issues]

    def close_issues(self, sequence: int, reasons: dict[str, str]) -> bool:
        assert sequence == 7
        self.closed.update(reasons)
        return True


@pytest.fixture
def tickets(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> TicketStore:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path))
    monkeypatch.setattr(
        count_trend, "proper_ancestor", lambda head: lambda commit: commit != head
    )
    monkeypatch.setattr(count_trend, "rust_commits", lambda old, new: [])
    history = count_trend.history_path("mac", "original")
    history.parent.mkdir(parents=True)
    return TicketStore(history)


@pytest.mark.parametrize(
    ("counts", "baseline"),
    [
        ([1_000_000, 1_003_000], 1_000_000),
        ([995_000, 997_000, 999_000, 1_001_000], 995_000),
    ],
)
def test_creep_closes_when_the_count_reaches_its_original_baseline(
    tickets: TicketStore, counts: list[int], baseline: int
) -> None:
    tickets.history.write_text(
        "".join(
            count_trend.dump(Record(str(i), count, BAR)) + "\n"
            for i, count in enumerate(counts[:-1])
        )
    )
    count_trend.report_at("rise", counts[-1], BAR, True, tickets, "mac", "original")
    count_trend.report_at(
        "partial", baseline + 1, BAR, True, tickets, "mac", "original"
    )
    assert not tickets.closed
    lines = count_trend.report_at(
        "recovery", baseline, BAR, True, tickets, "mac", "original"
    )
    assert tickets.closed == {
        "T1": f"Closed by count_trend: the count at recovery is {baseline:,}, "
        f"at or below {baseline:,} before the rise."
    }
    assert f"closed T1: {tickets.closed['T1']}" in lines


def test_lower_closes_across_calibrations_for_its_own_verifier(
    tickets: TicketStore,
) -> None:
    count_trend.report_at("drop", 994_000, BAR, True, tickets, "mac", "original")
    count_trend.report_at("drop", 994_000, BAR, True, tickets, "pc", "other")
    tickets.tickets["T1"]["history"] = [
        {
            "actor": "local-operator",
            "change": {"status": ["pending_approval", "open"], "version": [1, 2]},
        }
    ]
    count_trend.report_at("partial", 994_000, 994_001, True, tickets, "mac", "partial")
    assert not tickets.closed
    count_trend.report_at("recovery", 994_000, 994_000, True, tickets, "mac", "new")
    assert tickets.closed == {
        "T1": "Closed by count_trend: the count at recovery is 994,000, "
        "and mac's bar is at or below it at 994,000."
    }
    history = count_trend.history_path("mac", "new")
    assert count_trend.load(history.read_text()) == [
        Record("recovery", 994_000, 994_000)
    ]


@pytest.mark.parametrize(
    "changes",
    [
        {"comments": [{"author": "local-operator"}]},
        {"status": "in_progress", "assignee": "worker"},
        {"claimed_at": "earlier"},
        {
            "history": [
                {"actor": "worker", "change": {"status": ["open", "in_progress"]}}
            ]
        },
        {"history": [{"actor": "local-operator", "change": {"title": ["old", "new"]}}]},
    ],
)
def test_a_ticket_someone_took_up_stays_open(
    tickets: TicketStore, changes: dict[str, object]
) -> None:
    count_trend.report_at("drop", 994_000, BAR, True, tickets, "mac", "original")
    tickets.tickets["T1"].update(changes)
    count_trend.report_at("recovery", 994_000, 994_000, True, tickets, "mac", "new")
    assert not tickets.closed


def test_only_a_verified_tip_can_close_a_ticket(
    tickets: TicketStore, monkeypatch: pytest.MonkeyPatch
) -> None:
    count_trend.report_at("drop", 994_000, BAR, True, tickets, "mac", "original")
    monkeypatch.setattr(count_trend, "git", lambda *args: (0, "local"))
    count_trend.report_at(None, 994_000, 994_000, True, tickets, "mac", "new")
    assert not tickets.closed


def test_a_tracked_count_is_stored_with_its_step_and_files_nothing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(
        count_trend, "history_path", lambda verifier, digest: tmp_path / "gated.jsonl"
    )
    monkeypatch.setattr(count_trend, "git", lambda *args: (0, "tip"))
    monkeypatch.setattr(count_trend, "proper_ancestor", lambda head: lambda c: c == "a")
    path = count_trend.tracked_path("dandan", "mac", "lima")
    assert path != tmp_path / "gated.jsonl" and path.name == "dandan-lima.jsonl"
    assert count_trend.track_at("dandan", "a", 1_000_000, "mac", "lima") == []
    lines = count_trend.track_at("dandan", "b", 1_010_000, "mac", "lima")
    assert lines and "+1.00% since a" in lines[0] and "not gated" in lines[0]
    assert [r.instructions for r in count_trend.load(path.read_text())] == [
        1_000_000,
        1_010_000,
    ]
