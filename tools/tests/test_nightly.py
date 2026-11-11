import json
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import IO

import pytest

from beryl_tools import nightly, runlog

NOW = datetime(2026, 11, 11, 3, 0, tzinfo=timezone.utc)


class Fake:
    """A runner that records the commits it was given and exits with `code`."""

    def __init__(self, code: int = 0) -> None:
        self.code = code
        self.commits: list[str] = []

    def step(self) -> nightly.Step:
        def run(commit: str, out: IO[str]) -> int:
            self.commits.append(commit)
            out.write("ran\n")
            return self.code

        return nightly.Step("fake", run)


def fire(
    fake: Fake, now: datetime = NOW, load: float = 1.0, tip: str | None = "abc123"
) -> dict[str, object]:
    return nightly.run_nightly(
        [fake.step()],
        now=lambda: now,
        load=lambda: load,
        cores=lambda: 4,
        tip=lambda: tip,
    )


def log() -> list[dict[str, object]]:
    return [json.loads(line) for line in nightly.log_path().read_text().splitlines()]


def test_a_first_run_measures_the_tip_and_logs_it() -> None:
    fake = Fake()
    record = fire(fake)
    assert fake.commits == ["abc123"]
    assert record["outcome"] == "ok"
    assert record["commit"] == "abc123"
    assert record["steps"] == {"fake": 0}
    assert log() == [record]
    assert "ran" in (nightly.folder() / "last-output.txt").read_text()


def test_a_fresh_success_skips_and_says_why() -> None:
    fake = Fake()
    fire(fake)
    record = fire(fake, now=NOW + timedelta(hours=1))
    assert record["outcome"] == "skipped"
    assert "last success" in str(record["reason"])
    assert fake.commits == ["abc123"]
    assert [r["outcome"] for r in log()] == ["ok", "skipped"]


def test_a_success_over_twenty_hours_old_runs_again() -> None:
    fake = Fake()
    fire(fake)
    fire(fake, now=NOW + timedelta(hours=21))
    assert len(fake.commits) == 2


def test_high_load_skips_and_a_later_firing_retries() -> None:
    fake = Fake()
    record = fire(fake, load=9.0)  # Over twice the four cores.
    assert record["outcome"] == "skipped"
    assert "load average" in str(record["reason"])
    assert fake.commits == []
    fire(fake, now=NOW + timedelta(hours=1), load=8.0)  # Exactly twice: runs.
    assert fake.commits == ["abc123"]


def test_a_failed_run_does_not_count_as_a_success() -> None:
    fake = Fake(code=1)
    assert fire(fake)["outcome"] == "failed"
    assert fire(fake, now=NOW + timedelta(hours=1))["outcome"] == "failed"
    assert len(fake.commits) == 2


def test_a_failed_fetch_is_a_failure_that_runs_nothing() -> None:
    fake = Fake()
    record = fire(fake, tip=None)
    assert record["outcome"] == "failed"
    assert fake.commits == []


def test_the_log_survives_a_torn_line() -> None:
    nightly.log_path().write_text('{"time": "2026-11-11T0')
    fake = Fake()
    assert fire(fake)["outcome"] == "ok"


def test_the_agent_runs_the_main_checkouts_tools_from_three_to_eight() -> None:
    plist = nightly.agent_plist(Path("/repo"), "/bin/uv")
    assert plist["ProgramArguments"] == [
        "/bin/uv",
        "run",
        "--quiet",
        "--project",
        "/repo/tools",
        "by",
        "nightly",
    ]
    hours = [i["Hour"] for i in plist["StartCalendarInterval"]]  # pyre-ignore[16]
    assert hours == [3, 4, 5, 6, 7, 8]


def test_the_coverage_age_line(own_cache: Path) -> None:
    assert "no coverage recorded" in nightly.coverage_age_line(NOW)
    folder = own_cache / "ci" / "coverage"
    folder.mkdir(parents=True)
    latest = folder / "latest.json"
    measured = NOW - timedelta(hours=5)
    latest.write_text(
        json.dumps({"commit": "0123456789abcdef", "time": measured.isoformat()})
    )
    line = nightly.coverage_age_line(NOW)
    assert "01234567" in line and "5 h ago" in line and "stale" not in line
    stale = nightly.coverage_age_line(NOW + timedelta(hours=60))
    assert "stale" in stale


def test_the_fuzz_runs_at_low_priority_and_logs_its_outcome(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ran: list[tuple[str, list[str]]] = []

    def in_tree(commit: str, command: list[str], out: IO[str] | None = None) -> int:
        ran.append((commit, command))
        return 101

    monkeypatch.setattr(nightly.coverage, "in_tree", in_tree)
    with open(nightly.folder() / "out.txt", "w") as out:
        assert nightly.fuzz_step("abc123", out) == 101
    [(commit, command)] = ran
    assert commit == "abc123"
    assert command[-2:] == ["just", "fuzz"]
    assert command != ["just", "fuzz"], "the fuzz runs at low priority"
    [record] = runlog.read_log(NOW - timedelta(days=3650))
    assert record["check"] == "fuzz:games"
    assert record["outcome"] == "fail"
    assert record["script"] == "nightly"
    assert record["commit"] == "abc123"
