"""Read Beryl 3D's phone frame log and check the Android smoke's inputs."""

import json
from typing import Any


def lines(text: str) -> list[dict[str, Any]]:
    """A frame log's lines, skipping a torn last line."""
    parsed = []
    for line in text.splitlines():
        try:
            parsed.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return parsed


def logged_inputs(log: list[dict[str, Any]], kind: str) -> list[dict[str, Any]]:
    return [line["input"][kind] for line in log if kind in line.get("input", {})]


def surface_sizes(log: list[dict[str, Any]]) -> list[tuple[float, float]]:
    return [
        (surface["width"], surface["height"])
        for surface in logged_inputs(log, "surface")
    ]


def rotated_and_returned(
    log: list[dict[str, Any]], original: tuple[float, float]
) -> bool:
    sizes = surface_sizes(log)
    swapped = (original[1], original[0])
    return swapped in sizes and original in sizes[sizes.index(swapped) + 1 :]


def android_touch_faults(
    log: str, original: tuple[float, float], callbacks_before: int, callbacks_after: int
) -> list[str]:
    parsed = lines(log)
    faults = []
    touch = [
        (p["id"], p["phase"])
        for p in logged_inputs(parsed, "pointer")
        if p["kind"] == "Touch"
    ]
    if (
        touch.count((1, "Down")) != 2
        or touch.count((1, "Up")) != 2
        or (1, "Move") not in touch
    ):
        faults.append(f"tap and swipe did not use touch id 1: {touch}")
    back = [
        key["phase"] for key in logged_inputs(parsed, "key") if key["code"] == "Back"
    ]
    if back != ["Down", "Up"]:
        faults.append(f"expected Back down and up, got {back}")
    if not rotated_and_returned(parsed, original):
        faults.append(
            f"expected the surface to rotate and return, got {surface_sizes(parsed)}"
        )
    if not any(
        surface.get("top", 0) > 0 for surface in logged_inputs(parsed, "surface")
    ):
        faults.append("no surface had a positive top inset")
    if callbacks_before <= 0 or callbacks_after != callbacks_before:
        faults.append(
            f"expected no frame callbacks during 2 s of idle, got {callbacks_after - callbacks_before}"
        )
    return faults
