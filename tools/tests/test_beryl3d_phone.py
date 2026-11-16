"""`just b3d ios-smoke` and `android-smoke`: how they wait on the app's frame log. A run passes
when the log holds its summary, and fails at once when the app writes an error or exits.
"""

import json

import pytest

from beryl_tools.beryl3d import phone, phone_log

RUN: str = json.dumps({"run": {"adapter": "a GPU (Vulkan)"}})
SUMMARY: str = json.dumps({"summary": {"frames": 3, "first_shown_ms": 0.0}})


def test_the_wait_ends_with_the_log_once_it_holds_a_summary(
    capsys: pytest.CaptureFixture[str],
) -> None:
    reads = iter([None, RUN, f"{RUN}\n{SUMMARY}\n"])
    log = phone.wait_for_log(
        lambda: next(reads), lambda: True, lambda: "", "smoke", pause=lambda s: None
    )
    assert log == f"{RUN}\n{SUMMARY}\n"
    assert "smoke: adapter a GPU (Vulkan)" in capsys.readouterr().out


def test_an_error_or_an_exit_ends_the_wait_with_no_log(
    capsys: pytest.CaptureFixture[str],
) -> None:
    error = phone.wait_for_log(
        lambda: RUN, lambda: True, lambda: "panic: no surface", "smoke"
    )
    assert error is None
    assert "panic: no surface" in capsys.readouterr().out
    exited = phone.wait_for_log(lambda: None, lambda: False, lambda: "", "smoke")
    assert exited is None
    assert "exited before the recording ended" in capsys.readouterr().out


def test_a_torn_last_line_is_skipped() -> None:
    assert phone_log.lines(f'{RUN}\n{{"summ') == [json.loads(RUN)]


def touch_log(*, top: float, returned: bool) -> str:
    return "\n".join(
        json.dumps({"input": {kind: event}})
        for kind, event in [
            ("surface", {"width": 400, "height": 800, "top": top}),
            *[
                ("pointer", {"id": 1, "kind": "Touch", "phase": phase})
                for phase in ("Down", "Up", "Down", "Move", "Up")
            ],
            ("key", {"code": "Back", "phase": "Down"}),
            ("key", {"code": "Back", "phase": "Up"}),
            ("surface", {"width": 800, "height": 400}),
            *([("surface", {"width": 400, "height": 800})] if returned else []),
        ]
    )


def test_android_touch_log_accepts_input_rotation_insets_and_idle() -> None:
    log = touch_log(top=24, returned=True)
    assert phone_log.android_touch_faults(log, (400, 800), 42, 42) == []


def test_android_touch_log_rejects_missing_input_insets_rotation_or_idle() -> None:
    log = touch_log(top=0, returned=False).replace('"Up"', '"Cancel"')
    faults = phone_log.android_touch_faults(log, (400, 800), 42, 43)
    assert any("touch id 1" in fault for fault in faults)
    assert any("Back down and up" in fault for fault in faults)
    assert any("rotate and return" in fault for fault in faults)
    assert any("top inset" in fault for fault in faults)
    assert any("got 1" in fault for fault in faults)
    assert phone_log.android_touch_faults(
        touch_log(top=24, returned=True), (400, 800), 0, 0
    )


SUSPEND = {"input": {"lifecycle": {"event": "Suspended"}}}
RESUME = {"input": {"lifecycle": {"event": "Resumed"}}}
LOW_MEMORY = {"input": {"lifecycle": {"event": "LowMemory"}}}
SURFACE = {"input": {"surface": {"width": 400, "height": 800}}}
SHOWN = {"lifecycle_shown_ms": 4000}
HOST = "surface-lost\nhaptic\nshare\nopen-url\n"


def test_android_lifecycle_accepts_surface_recreation_and_platform_requests() -> None:
    log = "\n".join(
        [
            RUN,
            *(
                json.dumps(line)
                for line in (SUSPEND, RESUME, SURFACE, SHOWN, LOW_MEMORY)
            ),
            SUMMARY,
        ]
    )
    assert phone_log.android_lifecycle_faults(log, HOST) == []


def test_android_lifecycle_rejects_frames_in_the_background_and_events_out_of_order() -> (
    None
):
    entries = (LOW_MEMORY, SUSPEND, SHOWN, RESUME, SHOWN, SURFACE)
    log = "\n".join([RUN, *(json.dumps(line) for line in entries)])
    assert phone_log.android_lifecycle_faults(
        log, "haptic\nshare failed: IOException\nopen-url\n"
    ) == [
        "a frame was shown while suspended",
        "no LowMemory after Resumed",
        "no frame shown on the resumed surface",
        "the fixture did not finish",
        "host.log did not record surface-lost",
        "host.log did not record share",
    ]
