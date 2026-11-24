"""The frame trend compares the saved board counts and reports findings to Ignite."""

import argparse
import json
from datetime import UTC, datetime, timedelta
from pathlib import Path

import pytest

from beryl_tools.client import trend
from beryl_tools.ignite import client, client_frame_trend

BOARD = "stress-255"
COUNTER = "total_elements"
START = datetime(2026, 11, 1, tzinfo=UTC)
PROVENANCE = trend.Provenance("test-verifier", "native-digest", "instruction-digest")


@pytest.fixture(autouse=True)
def calibration(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.setattr(trend, "current_provenance", lambda: PROVENANCE)
    monkeypatch.setattr(
        trend, "series_path", lambda _profile: tmp_path / "frames.jsonl"
    )


def series(path: Path, samples: list[tuple[int, int | None]]) -> None:
    with path.open("w", encoding="utf-8") as out:
        for day, count in samples:
            counts = {COUNTER: count} if count is not None else {}
            out.write(
                json.dumps(
                    {
                        "commit": f"commit-{day}",
                        "time": (START + timedelta(days=day)).isoformat(),
                        "board": BOARD,
                        "counts": counts,
                        "provenance": vars(PROVENANCE),
                    }
                )
                + "\n"
            )


@pytest.mark.parametrize(
    ("samples", "status", "wording"),
    [
        ([(0, 100), (1, 106)], 1, "from 100 (commit-0) to 106 (commit-1)"),
        (
            [(0, 100), (2, 103), (4, 107), (7, 111)],
            1,
            "from 100 (commit-0) to 111 (commit-7), over the 10% seven-day threshold",
        ),
        ([(0, 100), (1, 95)], 0, ""),
        ([(0, 100), (1, None), (2, 106)], 0, ""),
        ([(0, 100), (1, 105)], 0, ""),
        ([(0, 100), (2, 107), (8, 111)], 0, ""),
        ([(0, 0), (1, 1)], 1, "from 0 (commit-0) to 1 (commit-1)"),
        ([(0, 100), (3, 104), (5, 108), (7, 110)], 0, ""),
    ],
)
def test_check_compares_the_two_baselines(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    samples: list[tuple[int, int | None]],
    status: int,
    wording: str,
) -> None:
    path = tmp_path / "frames.jsonl"
    series(path, samples)
    parser = argparse.ArgumentParser()
    trend.arguments(parser)
    assert trend.run(parser.parse_args(["check"])) == status
    output = capsys.readouterr().out
    if status:
        assert f"{BOARD} {COUNTER} rose {wording}" in output
    else:
        assert output == ""


def test_record_keeps_one_row_per_board_and_all_native_counts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = tmp_path / "frames.jsonl"
    series(path, [(0, 100)])
    monkeypatch.setattr(trend, "current_commit", lambda: "current")
    native: trend.Boards = {BOARD: {COUNTER: 106}, "pay-default": {COUNTER: 29}}
    instructions: trend.Boards = {
        BOARD: {"total_instructions": 200},
        "other": {"total_instructions": 4},
    }
    history = trend.record(
        path,
        "current",
        lambda: trend.with_instructions(native, instructions),
        PROVENANCE,
    )
    assert trend.load(path) == history
    assert len(path.read_text().splitlines()) == 3
    assert history[-1].provenance == PROVENANCE
    assert history[-1].boards == {
        BOARD: {COUNTER: 106, "total_instructions": 200},
        "pay-default": {COUNTER: 29},
    }


def test_exploration_lists_unmeasured_head_and_reports_its_jump(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    path = tmp_path / "frames.jsonl"
    initial = trend.Measurement(
        "previous", datetime.now(UTC) - timedelta(hours=1), {BOARD: {COUNTER: 100}}
    )
    path.write_text(
        json.dumps(
            {
                "commit": initial.commit,
                "time": initial.time.isoformat(),
                "board": BOARD,
                "counts": initial.boards[BOARD],
                "provenance": vars(PROVENANCE),
            }
        )
        + "\n"
    )
    results = tmp_path / "results.jsonl"
    monkeypatch.setenv("IGNITE_RESULTS", str(results))
    commit = "c" * 40
    family = client_frame_trend.FAMILY
    check = f"{family}:{commit}"
    monkeypatch.setattr(trend, "current_commit", lambda: commit)
    monkeypatch.setattr(client_frame_trend, "collect", lambda: {BOARD: {COUNTER: 106}})
    for verb in ("list", "next"):
        assert client.main([family, verb]) == 0
        assert json.loads(capsys.readouterr().out) == {
            "check": check,
            "digest": f"{commit}:{PROVENANCE.digest}",
        }

    for _ in range(2):
        assert client.main([family, "run", check]) == 1
    reports = [json.loads(line) for line in results.read_text().splitlines()]
    assert len(reports) == 2
    for report in reports:
        assert report["check"] == check
        assert report["outcome"] == "fail"
        assert f"from 100 (previous) to 106 ({commit})" in report["message"]
    assert len(trend.load(path)) == 2
    assert client.main([family, "next"]) == 0
    assert capsys.readouterr().out == ""


def test_a_saved_check_cannot_measure_a_different_commit(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = tmp_path / "frames.jsonl"
    monkeypatch.setattr(trend, "current_commit", lambda: "current")
    results = client_frame_trend.run([f"{client_frame_trend.FAMILY}:old"])
    assert [result.outcome for result in results] == ["incomplete"]
    assert not path.exists()


def test_trend_cannot_compare_samples_from_other_calibrations() -> None:
    different = trend.Provenance("test-verifier", "other-native", "instruction-digest")
    history = [
        trend.Measurement("old", START, {BOARD: {COUNTER: 1}}, different),
        trend.Measurement(
            "new", START + timedelta(days=1), {BOARD: {COUNTER: 100}}, PROVENANCE
        ),
    ]
    assert (
        trend.regressions(history, {"adjacent_percent": 5, "weekly_percent": 10}) == []
    )
