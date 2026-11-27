from __future__ import annotations

from typing import Any

from beryl_tools.client import pointer_gaps


def pointer(
    phase: str, x: float, at_ms: float, os_ms: float | None = None
) -> dict[str, Any]:
    line: dict[str, Any] = {
        "pointer": {"id": 0, "kind": "Mouse", "phase": phase, "x": x, "y": 500.0},
        "at_ms": at_ms,
    }
    if os_ms is not None:
        line["os_ms"] = os_ms
    return line


def test_a_stall_that_ends_in_a_jump_is_counted_and_a_still_pointer_is_not() -> None:
    inputs = [
        pointer("Move", 0.0, 0.0),  # before the press: not part of a drag
        pointer("Down", 100.0, 10.0),
        pointer("Move", 104.0, 18.0),
        pointer("Move", 104.0, 300.0),  # held still: a gap with no jump
        pointer("Move", 704.0, 908.0),  # a stall: 608 ms, then 600 points
        pointer("Up", 704.0, 916.0),
        pointer("Move", 2000.0, 5000.0),  # after the release
    ]
    gaps = pointer_gaps.measure(inputs)
    assert gaps.events == 5
    assert gaps.longest_ms == 608.0
    assert [(s.gap_ms, s.jump) for s in gaps.stalls] == [(608.0, 600.0)]
    printed = pointer_gaps.describe(gaps)
    assert printed[0] == (
        "pointer events while a button was held: 5; longest gap 608 ms;"
        " 1 gaps over 50 ms ended in a jump of over 60 points"
    )
    assert "at 0.908 s: 608 ms, then a jump of 600 points" in printed[1]


def test_system_stamps_tell_events_sent_late_from_events_taken_late() -> None:
    inputs = [
        pointer("Down", 0.0, 0.0, os_ms=-1.0),
        # The system stamped these 8 ms apart, but the app took the second 400 ms later.
        pointer("Move", 100.0, 8.0, os_ms=7.0),
        pointer("Move", 200.0, 408.0, os_ms=15.0),
        # The system sent nothing for 500 ms.
        pointer("Move", 300.0, 910.0, os_ms=908.0),
        pointer("Up", 300.0, 918.0, os_ms=917.0),
    ]
    printed = pointer_gaps.describe(pointer_gaps.measure(inputs))
    assert printed[1] == (
        "  from the system's stamp to the client: longest 393 ms, 1 of 5 events over 50 ms"
    )
    assert printed[2] == (
        "  of the stalls, 1 had the same gap between the system's stamps"
        " (the system sent no events), 1 did not (the app took them late)"
    )


def test_a_log_with_no_press_reports_nothing() -> None:
    assert (
        pointer_gaps.describe(pointer_gaps.measure([pointer("Move", 0.0, 1.0)])) == []
    )
