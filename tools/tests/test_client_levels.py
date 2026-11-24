"""The the client client core's dependency order and source budgets."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from beryl_tools.client import purity

KIT = "client/crates/kit_base"
GAME = "client/crates/game_table"
APP = "client/crates/app_client"


def write(root: Path, relative: str, content: str) -> None:
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


@pytest.fixture
def root(tmp_path: Path) -> Path:
    write(
        tmp_path,
        purity.LEVELS_FILE,
        json.dumps(
            {
                "crates": {
                    "kit_base": {"level": "toolkit", "order": 0},
                    "game_table": {"level": "game", "order": 1},
                    "app_client": {"level": "app", "order": 2},
                },
                "cap_lines": 5,
                "cap_allowances": {},
            }
        ),
    )
    for crate in (KIT, GAME, APP):
        write(tmp_path, f"{crate}/Cargo.toml", "")
    return tmp_path


@pytest.mark.parametrize("name", ("beryl_assets", "beryl_render"))
def test_drawing_crates_can_only_use_toolkit_members(root: Path, name: str) -> None:
    manifest = f"client/crates/{name}/Cargo.toml"
    write(root, manifest, '[dependencies]\nkit_base = { path = "../kit_base" }\n')
    assert purity.levels(root) == []
    write(root, manifest, '[dependencies]\napp_client = { path = "../app_client" }\n')
    assert purity.levels(root) == [f"{manifest}: {name} depends upward on app_client"]


def test_system_font_path_is_not_a_game_zone(root: Path) -> None:
    write(
        root,
        f"{KIT}/build.rs",
        'const PATH: &str = "/System/Library/Fonts/Supplemental";\n',
    )
    assert purity.levels(root) == []


def test_upward_manifest_dependency_fails_even_under_an_alias(root: Path) -> None:
    write(
        root,
        "client/Cargo.toml",
        '[workspace.dependencies]\nboard = { package = "game_table", path = "crates/game_table" }\n',
    )
    write(
        root,
        f"{KIT}/Cargo.toml",
        "[target.'cfg(unix)'.dev-dependencies]\nboard.workspace = true\n",
    )
    (found,) = purity.levels(root)
    assert found == f"{KIT}/Cargo.toml: kit_base depends upward on game_table"


def test_crate_cannot_exceed_its_source_cap(root: Path) -> None:
    write(root, f"{GAME}/src/lib.rs", "// a line of source\n" * 5)
    assert purity.levels(root) == []
    write(root, f"{GAME}/src/table.rs", "// one more line\n")
    (found,) = purity.levels(root)
    assert found.startswith(
        f"{GAME}/src: game_table has 6 source lines, over the 5-line cap"
    )
    assert "Look for a natural seam first" in found


def allow_game_table(root: Path, lines: int, reason: str) -> Path:
    path = root / purity.LEVELS_FILE
    config = json.loads(path.read_text())
    config["cap_allowances"]["game_table"] = {"lines": lines, "reason": reason}
    path.write_text(json.dumps(config))
    return path


def test_cap_allowance_requires_and_reports_a_reason(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    write(root, f"{GAME}/src/lib.rs", "// a line of source\n" * 6)
    allow_game_table(root, 6, " \t")
    (found,) = purity.levels(root)
    assert "cap_allowances.game_table needs a non-empty reason" in found
    reason = "Placement and layout share the same private state."
    allow_game_table(root, 6, reason)
    assert purity.levels(root, verbose=True) == []
    assert f"Cap allowance for game_table: 6 lines. {reason}" in capsys.readouterr().out


def test_update_lowers_or_removes_a_cap_allowance_without_raising_it(
    root: Path,
) -> None:
    reason = "The row and stack layout use the same placement solver."
    path = allow_game_table(root, 7, reason)
    source = f"{GAME}/src/lib.rs"
    write(root, source, "// a line of source\n" * 8)
    (found,) = purity.levels(root, update=True)
    assert "above its 7-line allowance" in found
    assert json.loads(path.read_text())["cap_allowances"]["game_table"]["lines"] == 7
    write(root, source, "// a line of source\n" * 6)
    (found,) = purity.levels(root)
    assert "just client levels --update" in found
    assert purity.levels(root, update=True) == []
    assert json.loads(path.read_text())["cap_allowances"]["game_table"] == {
        "lines": 6,
        "reason": reason,
    }
    write(root, source, "// a line of source\n" * 5)
    assert purity.levels(root, update=True) == []
    assert json.loads(path.read_text())["cap_allowances"] == {}


@pytest.mark.parametrize(
    "manifest",
    (
        "[dependencies]\nberyl_protocol.workspace = true\n",
        '[build-dependencies]\nprotocol = { package = "beryl_protocol", path = "x" }\n',
        "[target.'cfg(unix)'.dev-dependencies]\nprotocol.workspace = true\n",
    ),
)
def test_toolkit_manifest_cannot_depend_on_protocol(root: Path, manifest: str) -> None:
    write(
        root,
        "client/Cargo.toml",
        '[workspace.dependencies]\nprotocol = { package = "beryl_protocol", path = "x" }\n',
    )
    write(root, f"{KIT}/Cargo.toml", manifest)
    assert purity.levels(root, update=True) == [
        f"{KIT}/Cargo.toml: toolkit crate kit_base depends on beryl_protocol"
    ]


@pytest.mark.parametrize(
    ("path", "source", "word"),
    (
        ("src/pose.rs", 'const PLACE: &str = "battlefield.yours";', "battlefield"),
        ("src/pose.rs", "const EXILE_ZONE_AUDIT: bool = true;", "exile"),
        ("src/pose.rs", "enum Zone { Graveyard }", "graveyard"),
        ("src/pose.rs", "let your_library = 3;", "library"),
        ("src/pose.rs", "use beryl_protocol::protocol::Side;", "beryl_protocol"),
        ("tests/all/story.rs", "// Look for mana counters.", "mana"),
        ("stories/check.story", 'button "View Battlefield"', "battlefield"),
    ),
)
def test_toolkit_content_cannot_name_game_terms(
    root: Path, path: str, source: str, word: str
) -> None:
    write(root, f"{GAME}/{path}", source)
    assert purity.levels(root) == []
    write(root, f"{KIT}/{path}", "\n" + source)
    assert purity.levels(root, update=True) == [
        f"{KIT}/{path}:2: toolkit source names {word}"
    ]


def test_web_source_citations_can_only_decrease(root: Path) -> None:
    path = root / purity.LEVELS_FILE
    config = json.loads(path.read_text())
    config["web_citations"] = {"game_table": 2}
    path.write_text(json.dumps(config))
    write(
        root,
        f"{GAME}/src/table.rs",
        "/* /* nested */ ui/src/game/table.ts */\n"
        'const REFERENCE: &str = r#"ui/src/game/Stage.tsx"#;\n'
        "// Look up the widget in crates/kit_ui/src/widgets.\n"
        "fn ratio(ui: f32, src: f32) -> f32 { ui/src }\n",
    )
    assert purity.levels(root) == []

    write(root, f"{GAME}/tests/extra.rs", "// ui/src/game/Stage.tsx\n")
    (extra,) = purity.levels(root, update=True)
    assert "web_citations.game_table measured 3, above 2" in extra
    assert json.loads(path.read_text())["web_citations"] == {"game_table": 2}

    (root / f"{GAME}/src/table.rs").unlink()
    (stale,) = purity.levels(root)
    assert "web_citations.game_table measured 1, below 2" in stale
    assert purity.levels(root, update=True) == []
    assert json.loads(path.read_text())["web_citations"] == {"game_table": 1}
