from __future__ import annotations

import json

from beryl_tools import ui_counts

TEXT: str = json.dumps(
    {
        "$comment": "kept",
        "fixtures": ["a"],
        "layouts": ["desktop"],
        "tolerance_percent": 3,
        "limits": {
            "a/desktop": {
                "renders": 1000,
                "mutations": 1000,
                "styled": 1000,
                "laidOut": 1000,
            }
        },
    }
)
GATE: ui_counts.Gate = ui_counts.load_gate(TEXT)


def counts(**changed: int) -> dict[str, dict[str, int]]:
    measured = {"renders": 1000, "mutations": 1000, "styled": 1000, "laidOut": 1000}
    return {"a/desktop": {**measured, "commits": 7, **changed}}


def test_judge_fails_a_rise_past_the_tolerance() -> None:
    assert ui_counts.judge(GATE, counts(renders=1030))[0]  # exactly at the tolerance
    ok, lines = ui_counts.judge(GATE, counts(renders=1031))
    assert not ok
    assert any("renders" in line and "over" in line for line in lines)


def test_judge_asks_for_lower_limits_after_a_drop() -> None:
    ok, lines = ui_counts.judge(GATE, counts(styled=900))
    assert ok and any("--update" in line for line in lines)
    ok, lines = ui_counts.judge(GATE, counts(styled=990))
    assert ok and not any("--update" in line for line in lines)


def test_judge_ignores_ungated_metrics() -> None:
    assert ui_counts.judge(GATE, counts(commits=10_000))[0]


def test_judge_fails_a_play_missing_from_either_side() -> None:
    assert not ui_counts.judge(GATE, {})[0]
    assert not ui_counts.judge(GATE, {**counts(), "a/phone": counts()["a/desktop"]})[0]


def test_updated_keeps_the_rest_of_the_file() -> None:
    data = json.loads(ui_counts.updated(TEXT, counts(renders=900)))
    assert data["$comment"] == "kept"
    assert data["limits"] == {
        "a/desktop": {
            "renders": 900,
            "mutations": 1000,
            "styled": 1000,
            "laidOut": 1000,
        }
    }


def test_gate_file_limits_every_gated_play() -> None:
    gate = ui_counts.load_gate(ui_counts.GATE_FILE.read_text())
    plays = {f"{f}/{l}" for f in gate.fixtures for l in gate.layouts}
    assert set(gate.limits) == plays
    for limits in gate.limits.values():
        assert set(limits) == set(ui_counts.GATED_METRICS)


def test_highest_takes_each_metric_from_the_play_that_counted_most() -> None:
    plays = [counts(laidOut=990, styled=1020), counts(laidOut=1010, styled=1000)]
    assert ui_counts.highest(plays)["a/desktop"] == {
        **counts()["a/desktop"],
        "laidOut": 1010,
        "styled": 1020,
    }
