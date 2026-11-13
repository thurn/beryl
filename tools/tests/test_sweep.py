import json
from datetime import datetime, timezone
from pathlib import Path

from beryl_tools import nightly_failures, slow_ci, sweep, verdicts

NOW = datetime(2026, 11, 13, 15, 0, tzinfo=timezone.utc)


class Runner:
    """A stand-in for the sweep's run in a tree: records what it was given, exits `code`."""

    def __init__(self, code: int = 0) -> None:
        self.code = code
        self.runs: list[tuple[str, str]] = []
        self.refreshed: list[str] = []

    def run(self, commit: str, since: str, output: Path) -> int:
        self.runs.append((commit, since))
        output.write_text("swept\n")
        return self.code

    def refresh(self, commit: str) -> str:
        self.refreshed.append(commit)
        return f"coverage: measuring {commit}"


def fire(
    runner: Runner, tip: str | None = "aaa", load: float = 1.0
) -> dict[str, object]:
    return sweep.run_sweep(
        now=lambda: NOW,
        load=lambda: load,
        cores=lambda: 4,
        tip=lambda: tip,
        run=runner.run,
        refresh=runner.refresh,
    )


def test_a_sweep_that_passes_refreshes_coverage_and_the_same_tip_is_not_swept_again() -> (
    None
):
    runner = Runner()
    record = fire(runner)
    assert (record["outcome"], record["commit"]) == ("ok", "aaa")
    assert runner.runs == [("aaa", "")]
    assert runner.refreshed == ["aaa"]
    again = fire(runner)
    assert again["outcome"] == "skipped"
    assert "swept already" in str(again["reason"])
    assert len(runner.runs) == 1


def test_a_failed_sweep_bisects_from_the_last_pass_and_refreshes_nothing() -> None:
    fire(Runner(), tip="good")
    failing = Runner(code=1)
    assert fire(failing, tip="bad1")["outcome"] == "failed"
    assert fire(failing, tip="bad2")["outcome"] == "failed"
    # Both search from the last sweep that passed, not the last that ran.
    assert failing.runs == [("bad1", "good"), ("bad2", "good")]
    assert failing.refreshed == []
    # A failed tip is not swept again until staging moves.
    assert fire(failing, tip="bad2")["outcome"] == "skipped"


def test_an_overloaded_machine_or_a_failed_fetch_runs_nothing() -> None:
    runner = Runner()
    assert "load average" in str(fire(runner, load=100.0)["reason"])
    assert fire(runner, tip=None)["outcome"] == "failed"
    assert runner.runs == []
    # The overloaded firing is no sweep: the next one runs the same tip.
    assert fire(runner)["outcome"] == "ok"


def test_slow_ci_leaves_the_sweeps_lanes_to_it() -> None:
    assert set(sweep.LANES) == set(slow_ci.SWEPT)
    assert not set(sweep.LANES) & set(slow_ci.LANES)
    assert [lane.name for lane in sweep.lanes()] == list(sweep.LANES)


def test_failed_checks_replay_by_name_or_whole() -> None:
    assert (
        sweep.reproduce("ui-test", ["ui-test:a pile splits", "verb:tap/yours/phone"])
        == "just ui-test 'a pile splits' verb:tap/yours/phone"
    )
    assert sweep.reproduce("shots", ["shots:phone-main"]) == "just ui-shots phone-main"
    assert sweep.reproduce("ui-test", ["ui-test:ui-test"]) == "just ui-test"
    assert sweep.reproduce("mobile", ["mobile:mobile-check"]) == "just mobile-check"


class FakeBd(verdicts.Beads):
    """bd as the sweep's filing uses it, in memory."""

    def __init__(self) -> None:
        self.issues: list[dict[str, object]] = []

    def bd(self, *args: str) -> tuple[int, str]:
        if args[:3] == ("list", "--label", sweep.LABEL):
            return 0, json.dumps(self.issues)
        if args[0] == "create":
            flag = lambda name: args[args.index(name) + 1]  # noqa: E731
            bead = f"by-{len(self.issues) + 1}"
            self.issues.append(
                {
                    "id": bead,
                    "title": flag("--title"),
                    "description": flag("--description"),
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


def test_a_failure_files_a_steward_bead_with_its_culprit_and_a_repeat_comments() -> (
    None
):
    store = FakeBd()
    found = [
        "bisect: ui-test:a pile splits  culprit 1234567890 feat: piles  (3 commits since"
        " the last passing sweep)"
    ]
    lines = sweep.file(
        "bad1", {"ui-test": ["ui-test:a pile splits"]}, "good", found, "log", store
    )
    assert lines == ["sweep: by-1: filed ui-test:a pile splits"]
    [issue] = store.issues
    assert "role:steward" in issue["labels"]  # pyre-ignore[58]
    meta = issue["metadata"]
    assert isinstance(meta, dict)
    assert meta[nightly_failures.CHECKS_KEY] == "ui-test:a pile splits"
    text = str(issue["description"])
    assert "culprit 1234567890" in text
    assert "last sweep that passed was at good" in text
    assert "just ui-test 'a pile splits'" in text
    assert "fast, deterministic test" in text

    sweep.file(
        "bad2",
        {"ui-test": ["ui-test:a pile splits", "ui-test:tokens wear their frame"]},
        "good",
        [],
        "log",
        store,
    )
    first, second = store.issues
    [comment] = first["comments"]  # pyre-ignore[23]
    assert "bad2" in comment and "tokens" not in comment.split("Bisection")[0]
    assert (
        second["title"] == "Fix sweep ui-test failure: ui-test:tokens wear their frame"
    )
