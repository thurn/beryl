import argparse
import json
import os
import subprocess
from collections.abc import Iterable
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest

from beryl_tools import checks, oracles, paths, runlog, verdicts


def logged() -> list[dict[str, object]]:
    return [json.loads(line) for line in runlog.log_path().read_text().splitlines()]


def reports(*results: tuple[str, str, float], then: str = "true") -> list[str]:
    """A step that reports these checks' results (check, outcome, seconds), as a test binary or
    browser script would, then runs `then`."""
    lines = "".join(
        json.dumps({"check": c, "outcome": o, "seconds": s}) + "\n"
        for c, o, s in results
    )
    return ["sh", "-c", f'cat >> "$BERYL_CHECK_RESULTS" <<"EOF"\n{lines}EOF\n{then}']


def test_each_check_a_step_reports_is_logged_and_a_bare_step_is_one_check() -> None:
    lane = (
        checks.Lane("ui-test")
        .step(
            "browser",
            reports(
                ("ui-test:drag a card", "pass", 1.5),
                ("ui-test:scry", "skip", 0),
            ),
        )
        .step("tsc", ["true"])
    )
    assert checks.run_lanes([lane], "ci", 60, strict=True) == 0
    records = logged()
    assert [(r["check"], r["outcome"]) for r in records] == [
        ("ui-test:drag a card", "pass"),
        ("ui-test:scry", "skip"),
        ("ui-test:tsc", "pass"),
    ]
    first = records[0]
    assert first["seconds"] == 1.5
    assert {k: first[k] for k in ("script", "lane", "chosen")} == {
        "script": "ci",
        "lane": "ui-test",
        "chosen": "all",
    }
    head = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=paths.REPO, capture_output=True, text=True
    ).stdout.strip()
    assert first["commit"] == head
    assert len(str(first["tree"])) == 40
    assert datetime.fromisoformat(str(first["time"])).tzinfo == timezone.utc
    assert isinstance(first["load"], float) and isinstance(first["host"], str)


def test_a_step_that_fails_outside_its_checks_is_logged_as_failing() -> None:
    passing = ("shots:board", "pass", 2.0)
    failing = ("shots:hand", "fail", 2.0)
    lanes = [
        checks.Lane("a").step("errors", reports(passing, then="false")),
        checks.Lane("b").step("fails", reports(failing, then="false")),
        checks.Lane("c").step("build", ["false"]).step("never", ["true"]),
    ]
    assert checks.run_lanes(lanes, "check", 60, strict=False) == 1
    outcomes = {(r["check"], r["outcome"]) for r in logged()}
    assert outcomes == {
        ("shots:board", "pass"),
        ("a:errors", "fail"),
        ("shots:hand", "fail"),
        ("c:build", "fail"),
    }


def test_a_rerun_logs_its_own_results_on_the_same_tree(tmp_path: Path) -> None:
    fails = reports(("ui-test:scry", "fail", 1.0), then="echo 'FAILED: scry'; false")
    passes = reports(("ui-test:scry", "pass", 1.0))
    once = f"if [ -e ran ]; then {passes[2]}; else touch ran; {fails[2]}; fi"
    lane = checks.Lane("ui-test").step(
        "ui-test", ["sh", "-c", once], tmp_path, rerun=checks.failed_titles
    )
    assert checks.run_lanes([lane], "ci", 60, strict=True) == 0
    records = logged()
    assert [(r["check"], r["outcome"]) for r in records] == [
        ("ui-test:scry", "fail"),
        ("ui-test:scry", "pass"),
    ]
    assert records[0]["tree"] == records[1]["tree"]


def test_a_log_that_cannot_be_written_fails_no_check(
    own_cache: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    (own_cache / "ci").write_text("a file where the log's folder goes")
    lane = checks.Lane("repo").step("just", ["true"])
    assert checks.run_lanes([lane], "check", 60, strict=False) == 0
    assert "run log: not written" in capsys.readouterr().err


def test_the_tree_hash_names_the_working_tree_untracked_files_and_all(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    def git(*args: str) -> None:
        subprocess.run(["git", *args], cwd=tmp_path, check=True, capture_output=True)

    git("init", "-q")
    (tmp_path / ".gitignore").write_text("ignored\n")
    (tmp_path / "a").write_text("one")
    git("add", "-A")
    git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "a")
    monkeypatch.setattr(paths, "REPO", tmp_path)
    committed = runlog.tree_hash()
    assert len(committed) == 40
    (tmp_path / "ignored").write_text("anything")
    assert runlog.tree_hash() == committed
    (tmp_path / "b").write_text("new")
    untracked = runlog.tree_hash()
    assert untracked != committed
    assert runlog.tree_hash() == untracked
    # The real index is left as it was: b is still untracked.
    status = subprocess.run(
        ["git", "status", "--porcelain"], cwd=tmp_path, capture_output=True, text=True
    ).stdout
    assert status == "?? b\n"


def record(
    check: str, outcome: str, seconds: float, days_ago: float = 0, **more: object
) -> dict[str, object]:
    when = datetime.now(timezone.utc) - timedelta(days=days_ago)
    return {
        "time": when.isoformat(timespec="seconds"),
        "lane": check.split(":")[0],
        "check": check,
        "outcome": outcome,
        "seconds": seconds,
        **more,
    }


def test_the_report_totals_each_check_and_lane_costliest_first(
    capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    now = datetime.now(timezone.utc)

    class Verdicts(verdicts.Beads):
        """The checks' beads hold one recent flake and one catch from before the window."""

        def checks(self) -> list[str]:
            return ["rust:beryl_rules::tests::a"]

        def history(self, checks: Iterable[str]) -> dict[str, list[verdicts.Entry]]:
            return {
                "rust:beryl_rules::tests::a": [
                    {
                        "time": (now - timedelta(days=40)).isoformat(),
                        "verdict": "catch",
                    },
                    {"time": now.isoformat(), "verdict": "flaky"},
                ]
            }

    monkeypatch.setattr(verdicts, "STORE", Verdicts())
    runlog.append(
        [
            record("rust:beryl_rules::tests::a", "pass", 1.0),
            record("rust:beryl_rules::tests::a", "fail", 2.0),
            record("ui-test:drag", "pass", 10.0),
            record("ui-test:drag", "skip", 0.0),
            record("shots:board", "fail", 100.0, days_ago=40),
        ]
    )
    with open(runlog.log_path(), "a") as f:
        f.write("not a record\n")
    args = argparse.Namespace(
        days=30, json=True, quarantined=False, pending=False, prune=False
    )
    assert runlog.report(args) == 0
    out = json.loads(capsys.readouterr().out)
    assert list(out["checks"]) == ["ui-test:drag", "rust:beryl_rules::tests::a"]
    assert out["checks"]["rust:beryl_rules::tests::a"] == {
        "runs": 2,
        "failures": 1,
        "skips": 0,
        "seconds": 3.0,
        "verdicts": {"flaky": 1},
        "backfilled": 0,
    }
    assert out["checks"]["ui-test:drag"]["skips"] == 1
    assert out["lanes"]["ui-test"]["seconds"] == 10.0
    assert list(out["lanes"]) == ["ui-test", "rust"]
    assert (
        runlog.report(
            argparse.Namespace(
                days=60, json=False, quarantined=False, pending=False, prune=False
            )
        )
        == 0
    )
    table = capsys.readouterr().out
    assert "6 runs" not in table and "4 runs of 3 checks" in table
    assert table.index("shots:board") < table.index("ui-test:drag")


def test_backfilled_failures_count_as_runs_and_rewrites_as_stale_updates(
    capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    class Verdicts(verdicts.Beads):
        """The failure's verdict, backfilled into its bead."""

        def checks(self) -> list[str]:
            return ["ui-test:drag"]

        def history(self, checks: Iterable[str]) -> dict[str, list[verdicts.Entry]]:
            when = datetime.now(timezone.utc).isoformat()
            entry = {"time": when, "verdict": "catch", "backfilled": True}
            return {"ui-test:drag": [entry]}

    monkeypatch.setattr(verdicts, "STORE", Verdicts())
    runlog.append(
        [
            record("ui-test:drag", "pass", 4.0),
            record("ui-test:drag", "fail", 0.0, days_ago=3, backfilled=True),
            record("ui-test:drag", runlog.REWRITE, 0.0, days_ago=2, backfilled=True),
            record("ui-test:drag", runlog.REWRITE, 0.0, days_ago=1, backfilled=True),
        ]
    )
    args = argparse.Namespace(
        days=30, json=True, quarantined=False, pending=False, prune=False
    )
    assert runlog.report(args) == 0
    assert json.loads(capsys.readouterr().out)["checks"]["ui-test:drag"] == {
        "runs": 2,
        "failures": 1,
        "skips": 0,
        "seconds": 4.0,
        "verdicts": {"stale": 2, "catch": 1},
        "backfilled": 4,
    }
    args.json = False
    assert runlog.report(args) == 0
    assert "ui-test:drag  catch 1, stale 2 (4 backfilled)" in capsys.readouterr().out


def test_nextest_junit_gives_each_test_a_check() -> None:
    xml = """<testsuites><testsuite name="beryl_cli::exe">
      <testcase name="plays::a_game" classname="beryl_cli::exe" time="0.5"/>
      <testcase name="plays::loses" classname="beryl_cli::exe" time="1.25">
        <failure message="assertion failed"/></testcase>
      <testcase name="slow" classname="beryl_rules" time="0"><skipped/></testcase>
    </testsuite></testsuites>"""
    assert checks.junit_results(xml) == [
        ("rust:beryl_cli::exe::plays::a_game", "pass", 0.5),
        ("rust:beryl_cli::exe::plays::loses", "fail", 1.25),
        ("rust:beryl_rules::slow", "skip", 0.0),
    ]


def test_oracle_scenarios_share_their_engines_seconds(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    results = tmp_path / "results.jsonl"
    monkeypatch.setenv(runlog.RESULTS_ENV, str(results))
    scenario = lambda name, status: {"card": "Ponder", "name": name, "status": status}
    oracles.log_results(
        {
            "xmage": {
                "scenarios": [
                    scenario("draws", "pass"),
                    scenario("shuffles", "allowlisted"),
                    scenario("odd", "unsupported"),
                ]
            },
            "forge": {"scenarios": [scenario("draws", "fail")]},
        },
        {"xmage": 4.0, "forge": 3.0},
    )
    assert [
        (r["check"], r["outcome"], r["seconds"]) for r in runlog.read_results(results)
    ] == [
        ("oracle:Ponder/draws/xmage", "pass", 2.0),
        ("oracle:Ponder/shuffles/xmage", "pass", 2.0),
        ("oracle:Ponder/odd/xmage", "skip", 0.0),
        ("oracle:Ponder/draws/forge", "fail", 3.0),
    ]


def test_the_report_counts_selection_misses_per_100_landed_commits(
    capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    now = datetime.now(timezone.utc)

    class Verdicts(verdicts.Beads):
        """Two misses in the window and one before it, beside a catch."""

        def checks(self) -> list[str]:
            return ["ui-test:drag", "shots:board"]

        def history(self, checks: Iterable[str]) -> dict[str, list[verdicts.Entry]]:
            old = (now - timedelta(days=40)).isoformat()
            return {
                "ui-test:drag": [
                    {"time": now.isoformat(), "verdict": "miss"},
                    {"time": now.isoformat(), "verdict": "catch"},
                    {"time": old, "verdict": "miss"},
                ],
                "shots:board": [{"time": now.isoformat(), "verdict": "miss"}],
            }

    monkeypatch.setattr(verdicts, "STORE", Verdicts())
    monkeypatch.setattr(runlog, "landed", lambda since: 150)
    args = argparse.Namespace(
        days=30, json=True, quarantined=False, pending=False, prune=False
    )
    assert runlog.report(args) == 0
    out = json.loads(capsys.readouterr().out)
    assert out["misses"] == {"misses": 2, "landed": 150, "per_100": 1.33}
    assert out["checks"]["ui-test:drag"]["verdicts"] == {"miss": 1, "catch": 1}
    args.json = False
    assert runlog.report(args) == 0
    assert (
        "ci-report: 2 selection misses in 150 landed commits, 1.33 per 100"
        in capsys.readouterr().out
    )


def test_landed_commits_are_master_s_since_the_window_began(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    repo: Path = tmp_path / "repo"
    repo.mkdir()

    def git(*args: str, date: str = "") -> None:
        env = {"GIT_COMMITTER_DATE": date, "GIT_AUTHOR_DATE": date} if date else {}
        subprocess.run(
            ["git", "-c", "user.name=CI", "-c", "user.email=ci@example.com", *args],
            cwd=repo,
            check=True,
            capture_output=True,
            env={**os.environ, **env},
        )

    git("init", "-q", "-b", "master")
    git("commit", "-q", "--allow-empty", "-m", "old", date="2020-01-01T00:00:00Z")
    for n in range(3):
        git("commit", "-q", "--allow-empty", "-m", f"new {n}")
    git("update-ref", "refs/remotes/origin/master", "HEAD")
    monkeypatch.setattr(paths, "REPO", repo)
    assert runlog.landed(datetime.now(timezone.utc) - timedelta(days=1)) == 3
