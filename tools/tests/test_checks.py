from pathlib import Path

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


def test_card_names_are_found_as_whole_words(tmp_path: Path) -> None:
    (tmp_path / "a.ts").write_text(
        'const PONDER = "Ponder";\n// Pondering, and Jace, the Mind Sculptor\'s +2\n'
    )
    names = ["Ponder", "Jace, the Mind Sculptor", "Jace"]
    assert checks.card_names_in(tmp_path, names) == [
        f"{tmp_path / 'a.ts'}:1: Ponder",
        f"{tmp_path / 'a.ts'}:2: Jace, the Mind Sculptor",
    ]
