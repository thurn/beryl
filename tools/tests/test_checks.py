import pytest

from beryl_tools import checks


def test_only_lanes_that_build_warm_the_caches(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    warmed: list[bool] = []
    monkeypatch.setattr(checks.warm, "ensure", lambda: warmed.append(True))
    docs = checks.Lane("docs").step("true", ["true"])
    assert checks.run_lanes([docs], "check", 60, strict=False) == 0
    assert warmed == []
    build = checks.Lane("build", warm=True).step("true", ["true"])
    assert checks.run_lanes([docs, build], "check", 60, strict=False) == 0
    assert warmed == [True]
