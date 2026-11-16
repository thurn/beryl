import json
from datetime import datetime, timedelta, timezone

from beryl_tools import runlog, runstore

NOW = datetime(2026, 11, 16, 8, 30, tzinfo=timezone.utc)


def line(
    check: str, outcome: str, seconds: float, when: datetime, script: str = "check"
) -> str:
    record = {
        "time": when.isoformat(timespec="seconds"),
        "script": script,
        "lane": check.split(":")[0],
        "check": check,
        "outcome": outcome,
        "seconds": seconds,
    }
    return json.dumps(record) + "\n"


def put(when: datetime, *lines: str) -> None:
    """Writes lines into the hour file of `when`, as an append in that hour would."""
    runstore.hours_dir().mkdir(parents=True, exist_ok=True)
    with open(runstore.hour_file(when.strftime(runstore.HOUR)), "a") as f:
        f.writelines(lines)


def test_lane_tallies_match_a_scan_of_every_line() -> None:
    yesterday = NOW - timedelta(days=1)
    earlier = NOW - timedelta(hours=2)
    put(yesterday, line("rust:a", "pass", 2.0, yesterday))
    put(yesterday, line("rust:a", "fail", 4.0, yesterday))
    put(yesterday, line("ui-test:x", "skip", 0.0, yesterday))
    put(earlier, line("rust:a", "pass", 1.0, earlier))
    put(earlier, line("rust:a", "fail", 9.0, earlier, script="bisect"))
    put(NOW, line("rust:b", "pass", 3.0, NOW), "a torn line")
    # A worktree on tools from before the split still appends to the single file.
    with open(runstore.legacy_path(), "w") as f:
        f.write(line("rust:b", "pass", 5.0, NOW))
    since = NOW - timedelta(days=30)
    found = runstore.lane_tallies(since, "rust", NOW)
    assert found["rust:a"] == runstore.Tally(runs=3, failures=1, seconds=7.0)
    assert found["rust:b"] == runstore.Tally(runs=2, seconds=8.0)
    # Yesterday and the finished hours are summed now, and the second read uses the sums.
    assert (runstore.sums_dir() / yesterday.strftime(runstore.DAY) / "DONE").exists()
    assert (runstore.sums_dir() / earlier.strftime(runstore.HOUR) / "DONE").exists()
    runstore.hour_file(yesterday.strftime(runstore.HOUR)).unlink()
    assert runstore.lane_tallies(since, "rust", NOW) == found
    assert runstore.lane_tallies(since, "ui-test", NOW) == {
        "ui-test:x": runstore.Tally(skips=1)
    }


def test_an_append_folds_the_old_single_file_into_hour_files_and_resums() -> None:
    old = datetime.now(timezone.utc) - timedelta(days=2)
    put(old, line("rust:a", "pass", 1.0, old))
    since = old - timedelta(days=1)
    assert runstore.lane_tallies(since, "rust")["rust:a"].runs == 1
    with open(runstore.legacy_path(), "w") as f:
        f.write(line("rust:a", "pass", 1.0, old))
        f.write("not a record\n")
    runlog.append([json.loads(line("rust:a", "fail", 1.0, old + timedelta(days=2)))])
    assert not runstore.legacy_path().exists()
    assert runstore.folded_file(old.strftime(runstore.HOUR)).exists()
    # The folded line went to its own day, whose sums were built again.
    assert runstore.lane_tallies(since, "rust")["rust:a"] == runstore.Tally(
        runs=3, failures=1, seconds=3.0
    )
    assert len(runlog.read_log(since)) == 3


def test_reading_the_log_skips_hour_files_before_the_window() -> None:
    put(NOW - timedelta(days=3), line("rust:a", "pass", 1.0, NOW - timedelta(days=3)))
    put(NOW, line("rust:b", "pass", 1.0, NOW))
    lines = list(runstore.read_lines(NOW - timedelta(hours=1)))
    assert [json.loads(x)["check"] for x in lines] == ["rust:b"]


def test_prune_keeps_only_the_windows_readers_use() -> None:
    old = NOW - timedelta(days=runstore.RAW_DAYS + 1)
    put(old, line("rust:a", "pass", 1.0, old))
    put(NOW, line("rust:a", "pass", 1.0, NOW))
    runstore.write_sums(runstore.sums_dir() / old.strftime(runstore.DAY), {})
    runstore.prune(NOW)
    assert [p.stem for p in runstore.hours_dir().glob("*.jsonl")] == [
        NOW.strftime(runstore.HOUR)
    ]
    assert not list(runstore.sums_dir().iterdir())


def test_roll_folds_and_sums() -> None:
    then = datetime.now(timezone.utc) - timedelta(days=1)
    runstore.root().mkdir(parents=True)
    with open(runstore.legacy_path(), "w") as f:
        f.write(line("rust:a", "pass", 1.0, then))
    assert "folded 1 lines" in runstore.roll()
    assert (runstore.sums_dir() / then.strftime(runstore.DAY) / "rust.json").exists()
