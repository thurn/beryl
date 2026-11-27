"""The gaps between pointer events while a button is held, from a frame log's input lines.

A card being dragged freezes on screen when the app draws frames but gets no pointer moves: the
frame counter stays high while the card stands still, then jumps to where the pointer has gone.
The late frames do not show this, so `just client frames` reports the pointer's own gaps. Where
the host knows when the operating system stamped each event (`os_ms`, on macOS), it also says
whether the system sent the events late or the app took them late.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Any

# A gap longer than this between two pointer events during a drag is a stall the player can see:
# six frames at 120 Hz.
STALL_MS = 50.0
# A stall counts when the pointer moved at least this far in it, so that a pointer held still,
# which macOS sends no moves for, is not counted.
JUMP_POINTS = 60.0
# How many of the longest stalls the report lists.
LISTED = 5
# The pointer phases of a drag. A wheel turn or the pointer leaving the window is not part of it.
PHASES = ("Down", "Move", "Up", "Cancel")


@dataclass
class Stall:
    """A gap between two pointer events while a button was held."""

    at_ms: float
    gap_ms: float
    jump: float
    # The gap between the two events' operating-system stamps, where both have one.
    os_gap_ms: float | None


@dataclass
class Gaps:
    """What a log's pointer events while a button was held add up to."""

    events: int = 0
    longest_ms: float = 0.0
    stalls: list[Stall] = field(default_factory=list)
    # From the operating system's stamp to the time the client took the event.
    stamped: int = 0
    longest_delay_ms: float = 0.0
    delays_over: int = 0


def measure(inputs: list[dict[str, Any]]) -> Gaps:
    """The gaps in `inputs`, the frame log's input lines (`{"pointer": {...}, "at_ms": ...}`)
    in order. Lines without `at_ms`, from logs before it was written, are skipped."""
    gaps = Gaps()
    held = False
    previous: dict[str, Any] | None = None
    for line in inputs:
        pointer = line.get("pointer")
        at = line.get("at_ms")
        if pointer is None or at is None or pointer["phase"] not in PHASES:
            continue
        phase = pointer["phase"]
        if phase == "Down":
            held = True
        elif not held:
            continue
        gaps.events += 1
        os_ms = line.get("os_ms")
        if os_ms is not None:
            gaps.stamped += 1
            delay = at - os_ms
            gaps.longest_delay_ms = max(gaps.longest_delay_ms, delay)
            gaps.delays_over += delay > STALL_MS
        if previous is not None and phase != "Down":
            gap = at - previous["at_ms"]
            gaps.longest_ms = max(gaps.longest_ms, gap)
            before = previous["pointer"]
            jump = math.hypot(pointer["x"] - before["x"], pointer["y"] - before["y"])
            if gap > STALL_MS and jump > JUMP_POINTS:
                os_gap = (
                    os_ms - previous["os_ms"]
                    if os_ms is not None and previous.get("os_ms") is not None
                    else None
                )
                gaps.stalls.append(Stall(at, gap, jump, os_gap))
        previous = line
        if phase in ("Up", "Cancel"):
            held = False
            previous = None
    return gaps


def describe(gaps: Gaps) -> list[str]:
    """The report's lines for `gaps`, none when no button was held."""
    if gaps.events == 0:
        return []
    out = [
        f"pointer events while a button was held: {gaps.events}; longest gap"
        f" {gaps.longest_ms:.0f} ms; {len(gaps.stalls)} gaps over {STALL_MS:.0f} ms"
        f" ended in a jump of over {JUMP_POINTS:.0f} points"
    ]
    if gaps.stamped:
        out.append(
            f"  from the system's stamp to the client: longest {gaps.longest_delay_ms:.0f} ms,"
            f" {gaps.delays_over} of {gaps.stamped} events over {STALL_MS:.0f} ms"
        )
        sent_late = [
            s for s in gaps.stalls if s.os_gap_ms is not None and s.os_gap_ms > STALL_MS
        ]
        taken_late = [
            s
            for s in gaps.stalls
            if s.os_gap_ms is not None and s.os_gap_ms <= STALL_MS
        ]
        if gaps.stalls:
            out.append(
                f"  of the stalls, {len(sent_late)} had the same gap between the system's stamps"
                f" (the system sent no events), {len(taken_late)} did not (the app took them late)"
            )
    for stall in sorted(gaps.stalls, key=lambda s: -s.gap_ms)[:LISTED]:
        stamps = (
            f", stamps {stall.os_gap_ms:.0f} ms apart"
            if stall.os_gap_ms is not None
            else ""
        )
        out.append(
            f"  at {stall.at_ms / 1000:.3f} s: {stall.gap_ms:.0f} ms, then a jump of"
            f" {stall.jump:.0f} points{stamps}"
        )
    return out
