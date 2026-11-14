from typing import Callable

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
