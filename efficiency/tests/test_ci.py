"""CI accounting: the run log and the verdicts on the checks' beads, read into the store and
reported as the `ci` tree, over small synthetic sources."""

from __future__ import annotations

import json
import sqlite3
from collections.abc import Mapping
from pathlib import Path

import pytest

from beryl_efficiency import cilog, cli, cpu, ingest, paths, store
from beryl_efficiency.beads import Beads
from tests.synthetic import bead as bead_record
from tests.synthetic import listing

HOST = "jarvis"
CORES = 8

pytestmark: pytest.MarkDecorator = pytest.mark.usefixtures("utc_days")


def line(
    moment: str,
    check: str,
    outcome: str = "pass",
    seconds: float = 1.0,
    cpu_seconds: float | None = None,
    script: str = "ci",
    lane: str | None = None,
    commit: str = "c1",
    worktree: str | None = None,
) -> str:
    """A run-log line as runlog.py writes one."""
    record: dict[str, object] = {
        "time": moment,
        "host": "JARVIS.local",
        "commit": commit,
        "tree": "t-" + commit,
        "script": script,
        "lane": lane or check.split(":", 1)[0],
        "check": check,
        "outcome": outcome,
        "seconds": seconds,
        "load": 10.0,
        "chosen": "all",
    }
    if worktree is not None:
        record["worktree"] = worktree
    if cpu_seconds is not None:
        record["cpu_seconds"] = cpu_seconds
    return json.dumps(record)


def verdict(
    comment: str, moment: str, verdict_name: str, script: str = "ci", **more: object
) -> dict[str, object]:
    """A verdict comment as verdicts.py records one on a check's bead."""
    entry: dict[str, object] = {
        "time": moment,
        "host": "JARVIS.local",
        "commit": "c1",
        "script": script,
        "verdict": verdict_name,
        **more,
    }
    return {"id": comment, "text": json.dumps(entry)}


def check_bead(check: str, lane: str, *comments: Mapping[str, object]) -> str:
    """A `ci-check` bead as `bd export` prints it."""
    return json.dumps(
        {
            "id": "by-ci-" + check.replace(":", "-")[:10],
            "title": check,
            "status": "closed",
            "labels": ["ci-check", f"ci-lane:{lane}"],
            "comments": list(comments),
        }
    )


def at(day: int, hour: int, minute: int = 0) -> str:
    return f"2026-11-{day:02d}T{hour:02d}:{minute:02d}:00+00:00"


RUST = "rust:beryl_cli::longthink::stops_cost_nothing"
FORGE = "oracle:Subtlety/generated 9/forge"
XMAGE = "oracle:Subtlety/generated 9/xmage"
VERB = "verb:draw/yours/phone"

EXPORT: str = "\n".join(
    [
        json.dumps({"id": "by-1", "title": "not a check", "labels": []}),
        check_bead(
            RUST,
            "rust",
            verdict("v1", at(14, 10), "catch", lane="rust"),
            verdict("v2", at(14, 11), "stale", lane="rust"),
            verdict("v3", at(14, 12), "flaky", lane="rust"),
            # Corrected to a catch by the next comment.
            verdict("v4", at(14, 13), "mixed", lane="rust"),
            {"id": "v5", "text": json.dumps({"corrects": "v4", "verdict": "catch"})},
            verdict("v6", at(14, 14), "miss", lane="rust"),
            {"id": "v7", "text": "Flaked again."},
            # The fast check's verdict counts under the fast check.
            verdict("v8", at(14, 15), "catch", script="check", lane="rust"),
        ),
        check_bead(XMAGE, "oracle", verdict("v9", at(14, 16), "catch", lane="oracle")),
        check_bead(
            VERB, "ui-test", verdict("v10", at(14, 17), "mixed", lane="ui-test")
        ),
    ]
)

BEADS: bytes = listing(
    bead_record(
        "by-f.1",
        title=f"Delete flaky check {RUST}",
        status="open",
        labels=["flaky-check"],
    ),
)


def minute(moment: float, used: dict[str, float]) -> str:
    """A sampler row as `by cpu sample` writes one."""
    return json.dumps(
        {
            "time": moment,
            "seconds": 60.0,
            "cores": CORES,
            "cpu": used,
            "unnamed": {},
            "peak_cores": 4.0,
            "top_cores": 3.0,
            "load": 5.0,
        }
    )


def epoch(moment: str) -> float:
    return cilog.utc(moment).timestamp()


@pytest.fixture
def log(cache: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """The run log and the sources that read it; the test writes the log."""
    path = cilog.runlog_path(cache)
    path.parent.mkdir(parents=True)
    path.write_text("")
    samples = cpu.samples_path(cache)
    samples.parent.mkdir(parents=True)
    samples.write_text("")
    sources: list[ingest.Source] = [
        Beads(lambda: BEADS),
        cpu.Samples(samples, HOST),
        cilog.RunLog(path),
        cilog.Verdicts(lambda: EXPORT.encode()),
    ]
    monkeypatch.setattr(cli, "sources", lambda: sources)
    return path


def append(path: Path, *lines: str) -> None:
    with open(path, "a") as f:
        f.write("".join(l + "\n" for l in lines))


def ingest_now() -> None:
    assert cli.main(["ingest"]) == 0


def db() -> sqlite3.Connection:
    return store.open_read_only(paths.store())


def query(capsys: pytest.CaptureFixture[str], *argv: str) -> dict[str, object]:
    capsys.readouterr()
    assert cli.main(["ci", *argv, "--since", "all", "--no-ingest", "--json"]) == 0
    found = json.loads(capsys.readouterr().out)
    assert isinstance(found, dict)
    return found


def text(capsys: pytest.CaptureFixture[str], *argv: str) -> str:
    capsys.readouterr()
    assert cli.main(["ci", *argv, "--since", "all", "--no-ingest"]) == 0
    return capsys.readouterr().out


def test_families_come_from_check_names() -> None:
    assert cilog.family(RUST) == "rust:beryl_cli"
    assert cilog.family(FORGE) == "oracle:forge"
    assert cilog.family(XMAGE) == "oracle:xmage"
    assert cilog.family("ui-test:Enter on the start screen starts a game") == "ui-test"
    assert cilog.family(VERB) == "verb"
    assert cilog.family("python:pytest") == "python"
    assert cilog.family("whole") == "whole"


def test_daily_rows_add_up_across_ingests(log: Path) -> None:
    append(
        log,
        line(at(14, 10), RUST, seconds=2.0, cpu_seconds=3.0),
        line(at(14, 11), RUST, "fail", seconds=4.0, cpu_seconds=5.0),
    )
    ingest_now()
    # The second ingest starts at the offset the first one recorded: one more line on the
    # same day, one on the next, a skip, a line still being written, and a record of
    # history that is no run.
    append(
        log,
        line(at(14, 9), RUST, seconds=1.0),
        line(at(15, 1), RUST, cpu_seconds=1.0),
        line(at(15, 2), RUST, "skip", seconds=0.0),
        json.dumps({"time": at(15, 3), "outcome": "rewrite", "backfilled": True}),
    )
    with open(log, "a") as f:
        f.write(line(at(15, 4), RUST)[:40])
    ingest_now()
    ingest_now()  # Nothing new: nothing is added twice.
    with db() as found:
        rows = found.execute(
            "SELECT day, host, first, runs, passes, fails, skips, seconds, cpu_seconds, "
            "measured_seconds FROM checks ORDER BY day"
        ).fetchall()
    assert rows == [
        ("2026-11-14", HOST, "2026-11-14T09:00:00.000Z", 3, 2, 1, 0, 7.0, 8.0, 6.0),
        ("2026-11-15", HOST, "2026-11-15T01:00:00.000Z", 1, 1, 0, 1, 1.0, 1.0, 1.0),
    ]


def test_each_failure_is_counted_under_its_verdict(
    log: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    append(log, line(at(14, 10), RUST, cpu_seconds=7200.0))
    ingest_now()
    total = query(capsys)["total"]
    assert isinstance(total, dict)
    assert {k: total[k] for k in ("catch", "stale", "flaky", "mixed", "miss")} == {
        "catch": 3,
        "stale": 1,
        "flaky": 1,
        "mixed": 1,
        "miss": 1,
    }
    # 2 core hours over 3 catches; the fast check's verdict is the fast check's.
    assert total["core_hours"] == 2.0 and total["core_hours_per_catch"] == 0.67
    fast = query(capsys, "--script", "check")["total"]
    assert isinstance(fast, dict) and fast["catch"] == 1 and fast["core_hours"] == 0.0


def test_the_tree_by_lane_family_and_check(
    log: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    append(
        log,
        # Two slow-CI runs (two commits); the second measures no CPU, so its lines are
        # estimated from their lane's CPU per wall second.
        line(at(14, 10), RUST, seconds=10.0, cpu_seconds=20.0),
        line(at(14, 10), FORGE, seconds=30.0, cpu_seconds=30.0),
        line(at(14, 10), XMAGE, seconds=10.0, cpu_seconds=10.0),
        line(at(14, 10), VERB, seconds=5.0, cpu_seconds=5.0, lane="ui-test"),
        line(at(15, 10), RUST, seconds=5.0, commit="c2"),
        line(at(15, 10), FORGE, "skip", seconds=0.0, commit="c2"),
        line(at(15, 10), XMAGE, seconds=10.0, commit="c2"),
    )
    ingest_now()
    found = query(capsys)
    assert found["script_runs"] == 2 and found["level"] == "lane"
    rows = found["rows"]
    assert isinstance(rows, list)
    by_name = {r["name"]: r for r in rows}
    assert list(by_name) == ["oracle", "rust", "ui-test"]
    oracle = by_name["oracle"]
    # 40 s measured over 40 wall s, and 10 wall s more estimated at that rate.
    assert oracle["core_hours"] == round(50 / 3600, 2)
    assert [c["name"] for c in oracle["children"]] == ["oracle:forge", "oracle:xmage"]
    forge = oracle["children"][0]
    assert forge["runs"] == 1 and forge["skipped_percent"] == 50.0
    assert by_name["rust"]["estimated_percent"] == round(100 * 10 / 30, 1)
    # A family with no catch shows the day of its first run.
    lines = text(capsys)
    assert "ui-test" in lines and "never since 2026-11-14" in lines
    assert "Java" not in lines  # No sampler minute in the range.
    families = query(capsys, "--lane", "oracle")
    assert families["level"] == "family"
    engines = families["oracle"]
    assert isinstance(engines, list)
    # Two oracle runs of two scenarios: forge played one and skipped one, xmage played two.
    assert [
        (e["engine"], e["played"], e["skipped"], e["remembered"]) for e in engines
    ] == [
        ("forge", 1, 1, 0),
        ("xmage", 2, 0, 0),
    ]
    checks = query(capsys, "--family", "rust:beryl_cli")
    assert checks["level"] == "check"
    assert [r["name"] for r in checks["rows"]] == [RUST]  # type: ignore[union-attr]


def test_flake_cost_and_text_agree_with_json(
    log: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    append(
        log,
        line(at(14, 10), RUST, seconds=10.0, cpu_seconds=20.0),
        line(at(14, 11), RUST, seconds=30.0, cpu_seconds=40.0),
    )
    ingest_now()
    total = query(capsys)["total"]
    assert isinstance(total, dict)
    # One flaky failure: one rerun at the mean of 30 CPU seconds and 20 wall seconds.
    assert total["flake_cpu_seconds"] == 30.0 and total["flake_wall_seconds"] == 20.0
    shown = text(capsys)
    assert "flake cost 30.0 cpu s, 20.0 wall s" in shown
    assert f"ci runs {1}   cpu {total['core_hours']:.2f} core h" in shown


def test_cpu_by_kind_joins_the_sampler_s_minutes_to_lane_runs(
    log: Path, cache: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    # The oracle lane ran from 10:00 to 10:02 and the rust lane from 10:01 to 10:02, with
    # equal CPU per second; slow CI's minute at 10:01 is shared between them by that rate.
    append(
        log,
        line(at(14, 10, 2), FORGE, seconds=120.0, cpu_seconds=60.0),
        line(at(14, 10, 2), XMAGE, seconds=0.0, cpu_seconds=60.0),
        line(at(14, 10, 2), RUST, seconds=60.0, cpu_seconds=60.0),
    )
    append(
        cpu.samples_path(cache),
        minute(
            epoch(at(14, 10, 1)),
            {"slow_ci/oracle/ci-1": 90.0, "slow_ci/compile/ci-1": 30.0},
        ),
        minute(epoch(at(14, 11)), {"slow_ci/test/-": 36.0}),  # In no run's minute.
    )
    ingest_now()
    found = query(capsys)
    kinds = {k["kind"]: k for k in found["kinds"]}  # type: ignore[union-attr]
    assert kinds["oracle"]["core_hours"] == round(90 / 3600, 2)
    assert kinds["oracle"]["engines"] == {"forge": 0.01, "xmage": 0.01}
    assert kinds["test"]["percent"] == round(100 * 36 / 156, 1)
    oracle_only = query(capsys, "--lane", "oracle")
    kinds = {k["kind"]: k for k in oracle_only["kinds"]}  # type: ignore[union-attr]
    # The oracle runs had 120 CPU s over 120 s, the rust run 60 over 60: equal weights.
    assert kinds["oracle"]["core_hours"] == round(45 / 3600, 2)
    assert "Java: forge" in text(capsys)


def test_the_ci_metrics(log: Path, capsys: pytest.CaptureFixture[str]) -> None:
    append(
        log,
        line(at(14, 10), RUST, seconds=10.0, cpu_seconds=20.0),
        line(at(14, 10), XMAGE, seconds=10.0, cpu_seconds=10.0),
        line(at(14, 12), RUST, seconds=10.0, cpu_seconds=5.0, commit="c2"),
        line(at(14, 13), RUST, cpu_seconds=2.0, script="check", worktree="by-x.1"),
        line(
            at(14, 13), "repo:fmt", cpu_seconds=1.0, script="check", worktree="by-x.1"
        ),
    )
    ingest_now()
    capsys.readouterr()

    def series(name: str) -> list[dict[str, object]]:
        assert (
            cli.main(["metrics", name, "--since", "all", "--no-ingest", "--json"]) == 0
        )
        found = json.loads(capsys.readouterr().out)["series"]
        assert isinstance(found, list)
        return found

    per_run = series("slow-ci.cpu-seconds-per-run")
    # Runs of 30 and 5 CPU seconds, whose median the metric shows without decimals.
    assert [(p["count"], p["median"]) for p in per_run] == [(2, 18.0)]
    assert [p["median"] for p in series("slow-ci.runs-per-day")] == [2.0]
    assert [p["median"] for p in series("fast-check.cpu-seconds")] == [3.0]


def test_the_run_log_reads_the_hour_files_but_not_the_folded_ones(
    tmp_path: Path,
) -> None:
    single = cilog.runlog_path(tmp_path)
    hours = single.parent / "runs"
    hours.mkdir(parents=True)
    for name in (
        "2026-11-16T09.jsonl",
        "2026-11-16T08.jsonl",
        "2026-11-16T08.folded.jsonl",
    ):
        (hours / name).write_text("")
    assert [p.name for p in cilog.RunLog(single).files()] == [
        "2026-11-16T08.jsonl",
        "2026-11-16T09.jsonl",
    ]
    single.write_text("")
    assert cilog.RunLog(single).files()[0] == single
