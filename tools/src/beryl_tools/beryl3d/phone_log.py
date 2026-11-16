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


def suspension_bounds(log: list[dict[str, Any]]) -> tuple[int, int] | None:
    """Find the first return from suspension, for both polling and verification."""
    suspended = None
    for i, line in enumerate(log):
        event = line.get("input", {}).get("lifecycle", {}).get("event")
        if event == "Suspended" and suspended is None:
            suspended = i
        if event == "Resumed" and suspended is not None:
            return suspended, i
    return None


def android_lifecycle_faults(log: str, host: str) -> list[str]:
    parsed = lines(log)
    faults = []
    cycle = suspension_bounds(parsed)
    if cycle is None:
        faults.append("no Suspended followed by Resumed")
    else:
        suspended, resumed = cycle
        if any(
            "lifecycle_shown_ms" in line or "late" in line or "first_shown_ms" in line
            for line in parsed[suspended + 1 : resumed]
        ):
            faults.append("a frame was shown while suspended")

        after = resumed_entries(parsed, resumed)
        if not any(
            event["event"] == "LowMemory" for event in logged_inputs(after, "lifecycle")
        ):
            faults.append("no LowMemory after Resumed")
        if not logged_inputs(after, "surface"):
            faults.append("no surface after Resumed")
        elif not resumed_surface_was_shown(after):
            faults.append("no frame shown on the resumed surface")

    if not any("summary" in line for line in parsed):
        faults.append("the fixture did not finish")
    for event in ("surface-lost", "haptic", "share", "open-url"):
        if event not in host.splitlines():
            faults.append(f"host.log did not record {event}")
    return faults


def resumed_entries(log: list[dict[str, Any]], resumed: int) -> list[dict[str, Any]]:
    """The entries after a resume until the next suspension."""
    result = []
    for line in log[resumed + 1 :]:
        if line.get("input", {}).get("lifecycle", {}).get("event") == "Suspended":
            break
        result.append(line)
    return result


def resumed_surface_was_shown(entries: list[dict[str, Any]]) -> bool:
    surface = False
    for line in entries:
        if "surface" in line.get("input", {}):
            surface = True
        if surface and "lifecycle_shown_ms" in line:
            return True
    return False
