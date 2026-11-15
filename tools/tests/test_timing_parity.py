"""The comparison `just timing-parity` makes between the two printers' lines."""

from __future__ import annotations

import json
from typing import Any

from beryl_tools.timing_parity import differences


def line(step: int, **timing: Any) -> str:
    base: dict[str, Any] = {"case": "tap", "duration": 370, "after": [], "effects": []}
    return json.dumps(
        {
            "fixture": "verbs-zones",
            "seat": "yours",
            "view": 2,
            "step": step,
            "timing": base | timing,
        }
    )


def test_equal_lines_and_numbers_within_a_thousandth_agree() -> None:
    rust = "\n".join([line(1), line(2, duration=370.0004)])
    web = "\n".join([line(1), line(2)])
    assert differences(rust, web) == []


def test_a_differing_field_is_named_with_both_values() -> None:
    found = differences(line(1, stagger=300), line(1, stagger=0))
    assert found == [
        "verbs-zones yours view 2 step 1 (tap): stagger: session 300, web 0"
    ]


def test_a_field_one_side_leaves_out_differs() -> None:
    found = differences(line(1, hold=290), line(1))
    assert found == [
        "verbs-zones yours view 2 step 1 (tap): hold: session 290, web null"
    ]


def test_a_step_only_one_side_times_differs() -> None:
    assert differences(line(1), "") == [
        "verbs-zones yours view 2 step 1: only the session times it"
    ]
    assert differences("", line(1)) == [
        "verbs-zones yours view 2 step 1: only the web interface times it"
    ]


def test_a_flag_is_no_number() -> None:
    found = differences(line(1, opens=True), line(1, opens=1))
    assert len(found) == 1
