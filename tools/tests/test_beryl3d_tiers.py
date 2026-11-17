"""`just b3d tiers` reads each run's frame log for the tier chosen at startup and its drop."""

from __future__ import annotations

from typing import Any

from beryl_tools.beryl3d import tiers

AUTOMATIC, ENHANCED, FAILS = tiers.RUNS


def log(starts: str, steps: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [
        {"run": {}},
        {"tier": {"setting": "automatic", "tier": starts, "interval_ms": 16.7}},
        {"summary": {"steps": steps}},
    ]


def drop(frame: int) -> dict[str, Any]:
    return {"at_ms": frame * 25.0, "hz": None, "frame": frame}


def rate(frame: int) -> dict[str, Any]:
    return {"at_ms": frame * 25.0, "hz": 30.0, "frame": frame}


def test_an_enhanced_start_that_drops_in_time_before_the_rate_passes() -> None:
    assert tiers.problems(AUTOMATIC, log("enhanced", [drop(125), rate(128)])) == []


def test_a_late_drop_a_rate_step_first_or_no_drop_fails() -> None:
    latest = 1 + tiers.SETTLE_FRAMES + tiers.DROP_WITHIN
    assert tiers.problems(AUTOMATIC, log("enhanced", [drop(latest + 1)])) == [
        f"the tier dropped at frame {latest + 1}, after frame {latest}"
    ]
    assert tiers.problems(AUTOMATIC, log("enhanced", [rate(124), drop(130)]))[
        0
    ].startswith("the rate stepped before the tier dropped")
    assert tiers.problems(AUTOMATIC, log("enhanced", [rate(124)]))[0].startswith(
        "the tier never dropped"
    )


def test_a_fixed_enhanced_tier_never_drops_and_a_failed_measurement_starts_baseline() -> (
    None
):
    assert tiers.problems(ENHANCED, log("enhanced", [rate(125)])) == []
    assert tiers.problems(ENHANCED, log("enhanced", [drop(125)])) == [
        "the tier dropped at frame 125"
    ]
    assert tiers.problems(FAILS, log("baseline", [rate(125)])) == []
    assert tiers.problems(FAILS, log("enhanced", [])) == [
        "the tier started enhanced, not baseline"
    ]
