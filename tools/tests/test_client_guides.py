"""Guide sizes and recipes are checked from the help and map users see."""

import json
from pathlib import Path

import pytest

from beryl_tools.client import cli, guides, new, purity


def test_recipes_are_the_help_users_get(capsys: pytest.CaptureFixture[str]) -> None:
    assert cli.main(["new"]) == 0
    listing = capsys.readouterr().out
    recipes = new.help_by_kind()
    for kind, help_text in recipes.items():
        assert f"  {kind} " in listing
        with pytest.raises(SystemExit) as exit:
            cli.main(["new", kind, "--help"])
        assert exit.value.code == 0
        assert capsys.readouterr().out == help_text
        assert len(help_text.splitlines()) <= 30
    assert "timing to the effects list in data/ui-conventions.json" in recipes["effect"]


def test_guides_checks_caps_companions_and_map_recipes(tmp_path: Path) -> None:
    root_guide = tmp_path / "client/AGENTS.md"
    crate_guide = tmp_path / "client/crates/example/AGENTS.md"
    map_source = tmp_path / guides.MAP_SOURCE
    config = tmp_path / purity.LEVELS_FILE
    for path in (crate_guide, map_source, config):
        path.parent.mkdir(parents=True, exist_ok=True)
    root_guide.write_text("line\n" * 151)
    crate_guide.write_text("line\n" * 61)
    map_source.write_text(
        'Kind::scaffold("features", "feature", "...", entries::features)'
    )
    config.write_text("{}")
    failures = purity.guides(tmp_path)
    assert len(failures) == 3
    assert any("151 lines" in failure for failure in failures)
    assert any("61 lines" in failure for failure in failures)
    assert any("CLAUDE.md" in failure for failure in failures)

    allowances = {
        p.relative_to(tmp_path).as_posix(): n
        for p, n in ((root_guide, 151), (crate_guide, 61))
    }
    config.write_text(json.dumps({"guide_allowances": allowances}))
    crate_guide.with_name("CLAUDE.md").write_text("@AGENTS.md\n")
    assert purity.guides(tmp_path) == []

    root_guide.write_text("line\n" * 150)
    map_source.write_text(
        'Kind::scaffold("widgets", "unknown", "...", entries::widgets)'
    )
    failures = purity.guides(tmp_path)
    assert len(failures) == 2
    assert any("lower or remove" in failure for failure in failures)
    assert any(
        "map widgets: missing scaffold recipe 'unknown'" in failure
        for failure in failures
    )


def test_a_recipe_cannot_exceed_its_cap(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    top = tmp_path / "client/AGENTS.md"
    for file in (top, tmp_path / purity.LEVELS_FILE, tmp_path / guides.MAP_SOURCE):
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text("{}")
    monkeypatch.setattr(new, "help_by_kind", lambda: {"story": "line\n" * 31})
    assert purity.guides(tmp_path) == [
        "just client new story --help: 31 lines exceeds the 30-line limit"
    ]


def test_a_new_core_crate_needs_a_guide(tmp_path: Path) -> None:
    for file in (
        "client/AGENTS.md",
        guides.MAP_SOURCE,
        purity.LEVELS_FILE,
    ):
        target = tmp_path / file
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text("{}")
    config = tmp_path / purity.LEVELS_FILE
    data = {"crates": {"example": {"level": "toolkit"}}}
    config.write_text(json.dumps(data))
    assert purity.guides(tmp_path) == ["client/crates/example/AGENTS.md: missing guide"]
