"""`just b3d ios-smoke` and `android-smoke`: how they wait on the app's frame log. A run passes
when the log holds its summary, and fails at once when the app writes an error or exits.
"""

import json

import pytest

from beryl_tools.beryl3d import phone

RUN: str = json.dumps({"run": {"adapter": "a GPU (Vulkan)"}})
SUMMARY: str = json.dumps({"summary": {"frames": 3, "first_shown_ms": 0.0}})


def test_the_wait_ends_with_the_log_once_it_holds_a_summary(
    capsys: pytest.CaptureFixture[str],
) -> None:
    reads = iter([None, RUN, f"{RUN}\n{SUMMARY}\n"])
    log = phone.wait_for_log(lambda: next(reads), lambda: True, lambda: "", "smoke")
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
    assert phone.lines(f'{RUN}\n{{"summ') == [json.loads(RUN)]
