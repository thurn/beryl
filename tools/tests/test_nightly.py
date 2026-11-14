import json
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import IO

import pytest

from beryl_tools import nightly, nightly_failures, runlog, verdicts

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


def test_normal_heavy_load_runs() -> None:
    fake = Fake()
    fire(fake, load=9.0)  # Over twice the four cores, as this machine usually is.
    assert fake.commits == ["abc123"]


def test_overload_skips_and_a_later_firing_retries() -> None:
    fake = Fake()
    record = fire(fake, load=33.0)  # Past the clock's stretch on four cores.
    assert record["outcome"] == "skipped"
    assert "load average" in str(record["reason"])
    assert fake.commits == []
    fire(fake, now=NOW + timedelta(hours=1), load=32.0)  # Exactly at it: runs.
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


def test_the_motion_capture_runs_at_low_priority_and_logs_each_verb(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def in_tree(
        commit: str,
        command: list[str],
        out: IO[str] | None = None,
        env: dict[str, str] | None = None,
    ) -> int:
        # With the moments of cards changing zone, which the fast check leaves to it.
        unwrapped = [sys.executable, "-m", "beryl_tools.cli", "motion", "--more"]
        assert command[-3:] == unwrapped[-3:] and command != unwrapped
        results = Path((env or {})[runlog.RESULTS_ENV])
        with open(results, "a") as f:
            f.write(
                '{"check": "motion:fly/desktop", "outcome": "pass", "seconds": 1}\n'
            )
            f.write(
                '{"check": "motion:emerge/desktop", "outcome": "fail", "seconds": 1}\n'
            )
        return 1

    monkeypatch.setattr(nightly.coverage, "in_tree", in_tree)
    with open(nightly.folder() / "out.txt", "w") as out:
        assert nightly.motion_step("abc123", out) == 1
    records = runlog.read_log(NOW - timedelta(days=3650))
    assert {(r["check"], r["outcome"]) for r in records} == {
        ("motion:fly/desktop", "pass"),
        ("motion:emerge/desktop", "fail"),
    }
    assert all(r["script"] == "nightly" for r in records)


def test_the_ui_counts_gate_runs_at_low_priority_under_its_slow_ci_name(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ran: list[list[str]] = []

    def in_tree(
        commit: str,
        command: list[str],
        out: IO[str] | None = None,
        env: dict[str, str] | None = None,
    ) -> int:
        ran.append(command)
        return 1

    monkeypatch.setattr(nightly.coverage, "in_tree", in_tree)
    assert "ui-counts" in [step.name for step in nightly.STEPS]
    with open(nightly.folder() / "out.txt", "w") as out:
        assert nightly.ui_counts_step("abc123", out) == 1
    [command] = ran
    unwrapped = [sys.executable, "-m", "beryl_tools.cli", "ui-counts", "--gate"]
    assert command[-5:] == unwrapped and command != unwrapped
    [record] = runlog.read_log(NOW - timedelta(days=3650))
    assert (record["check"], record["outcome"]) == ("ui-counts:ui-counts", "fail")
    assert record["script"] == "nightly"


def test_the_oracles_play_every_scenario_at_low_priority_and_log_each(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def in_tree(
        commit: str,
        command: list[str],
        out: IO[str] | None = None,
        env: dict[str, str] | None = None,
    ) -> int:
        # The full sweep: no cards named and no selection.
        unwrapped = [sys.executable, "-m", "beryl_tools.cli", "oracle"]
        assert command[-4:] == unwrapped and command != unwrapped
        with open((env or {})[runlog.RESULTS_ENV], "a") as f:
            f.write('{"check": "oracle:A/a/forge", "outcome": "fail", "seconds": 1}\n')
        return 1

    monkeypatch.setattr(nightly.coverage, "in_tree", in_tree)
    assert "oracle" in [step.name for step in nightly.STEPS]
    with open(nightly.folder() / "out.txt", "w") as out:
        assert nightly.oracle_step("abc123", out) == 1
    [record] = runlog.read_log(NOW - timedelta(days=3650))
    assert (record["check"], record["outcome"]) == ("oracle:A/a/forge", "fail")
    assert (record["script"], record["commit"]) == ("nightly", "abc123")


class FakeBd(verdicts.Beads):
    """bd as the nightly's filing uses it, in memory."""

    def __init__(self) -> None:
        self.issues: list[dict[str, object]] = []

    def bd(self, *args: str) -> tuple[int, str]:
        if args[:3] == ("list", "--label", nightly_failures.LABEL):
            return 0, json.dumps(self.issues)
        if args[0] == "create":
            flag = lambda name: args[args.index(name) + 1]  # noqa: E731
            bead = f"by-{len(self.issues) + 1}"
            self.issues.append(
                {
                    "id": bead,
                    "title": flag("--title"),
                    "description": flag("--description"),
                    "priority": int(flag("--priority")),
                    "labels": flag("--labels").split(","),
                    "metadata": json.loads(flag("--metadata")),
                    "status": "open",
                    "comments": [],
                }
            )
            return 0, bead
        if args[:2] == ("comments", "add"):
            [issue] = [i for i in self.issues if i["id"] == args[2]]
            comments = issue["comments"]
            assert isinstance(comments, list)
            comments.append(args[3])
            return 0, ""
        raise AssertionError(args)


@pytest.fixture
def bd(monkeypatch: pytest.MonkeyPatch) -> FakeBd:
    fake = FakeBd()
    monkeypatch.setattr(verdicts, "STORE", fake)
    return fake


def filed_for(issue: dict[str, object]) -> list[str]:
    """The checks a bead was filed for, from its metadata."""
    meta = issue["metadata"]
    assert isinstance(meta, dict)
    return str(meta[nightly_failures.CHECKS_KEY]).splitlines()


def checking(lane: str, results: dict[str, str]) -> nightly.Step:
    """A step that logs `results` (check: outcome) to the run log as the nightly's steps do,
    failing when any check does."""

    def run(commit: str, out: IO[str]) -> int:
        record = runlog.Run("nightly", commit, place="nightly")
        failed = "fail" in results.values()
        with record.step({}) as (_, path):
            with open(path, "a") as f:
                for check, outcome in results.items():
                    f.write(
                        json.dumps({"check": check, "outcome": outcome, "seconds": 1})
                    )
                    f.write("\n")
            record.log(lane, lane, not failed, 1.0, path)
        out.write(f"{lane} output\n")
        return int(failed)

    return nightly.Step(lane, run)


def night(steps: list[nightly.Step], tip: str, now: datetime) -> dict[str, object]:
    return nightly.run_nightly(
        steps, now=lambda: now, load=lambda: 1.0, cores=lambda: 4, tip=lambda: tip
    )


def test_a_failed_check_files_one_steward_bead_and_a_repeat_comments_on_it(
    bd: FakeBd,
) -> None:
    passing = checking("oracle", {"oracle:Card A/a/forge": "pass"})
    assert night([passing], "good", NOW)["outcome"] == "ok"
    assert bd.issues == [], "a passing night files nothing"

    broken = checking(
        "oracle",
        {"oracle:Card A/a/forge": "fail", "oracle:Card B/b/xmage": "pass"},
    )
    assert night([broken], "bad1", NOW + timedelta(hours=21))["outcome"] == "failed"
    [issue] = bd.issues
    assert "role:steward" in issue["labels"]  # pyre-ignore[58]
    assert issue["priority"] == 1
    assert filed_for(issue) == ["oracle:Card A/a/forge"]
    text = str(issue["description"])
    assert "bad1" in text and "`oracle:Card A/a/forge`" in text
    assert "Card B" not in text.split("Output tail")[0], "only the failed checks"
    assert "just oracle 'Card A'" in text
    assert "last nightly that passed every step was at good" in text
    assert "oracle output" in text, "the tail of the night's output"

    # The next night fails the same check and one more.
    worse = checking(
        "oracle",
        {"oracle:Card A/a/forge": "fail", "oracle:Card B/b/xmage": "fail"},
    )
    night([worse], "bad2", NOW + timedelta(hours=42))
    first, second = bd.issues
    [comment] = first["comments"]  # pyre-ignore[23]
    assert "bad2" in comment and "Card A" in comment and "Card B" not in comment
    assert filed_for(second) == ["oracle:Card B/b/xmage"]


def test_a_closed_bead_does_not_hold_back_a_new_one(bd: FakeBd) -> None:
    broken = checking("fuzz", {"fuzz:games": "fail"})
    night([broken], "bad1", NOW)
    bd.issues[0]["status"] = "closed"
    night([broken], "bad2", NOW + timedelta(hours=21))
    assert len(bd.issues) == 2
    assert "just fuzz" in str(bd.issues[1]["description"])


def test_each_failing_lane_gets_its_own_bead(bd: FakeBd) -> None:
    steps = [
        checking("motion", {"motion:fly/desktop": "fail"}),
        checking("oracle", {"oracle:Card A/a/forge": "fail"}),
    ]
    night(steps, "bad", NOW)
    titles = sorted(str(i["title"]) for i in bd.issues)
    assert titles == [
        "Fix nightly motion failure: motion:fly/desktop",
        "Fix nightly oracle failure: oracle:Card A/a/forge",
    ]
