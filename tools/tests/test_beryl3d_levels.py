"""The Beryl 3D client core's dependency order and source budgets."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from beryl_tools.beryl3d import purity

SCENE = "rust/beryl3d/crates/beryl_scene/src"
KIT = "rust/beryl3d/crates/kit_base"
GAME = "rust/beryl3d/crates/game_table"
APP = "rust/beryl3d/crates/app_client"


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
                "modules": {
                    "pose": "kit_base",
                    "table": "game_table",
                    "table/overlay.rs": "app_client",
                },
                "upward": {},
                "protocol": {"kit_base": 0},
            }
        ),
    )
    for crate in (KIT, GAME, APP):
        write(tmp_path, f"{crate}/Cargo.toml", "")
    write(tmp_path, f"{SCENE}/table/mod.rs", "mod overlay;\n")
    write(tmp_path, f"{SCENE}/table/overlay.rs", "pub struct Overlay;\n")
    return tmp_path


def test_references_to_a_higher_module_fail(root: Path) -> None:
    write(root, f"{SCENE}/pose.rs", "type Overlay = crate::table::overlay::Overlay;\n")
    count, reference = purity.levels(root)
    assert "upward.pose measured 1, above 0" in count
    assert reference.startswith(f"{SCENE}/pose.rs:1: crate::table::overlay::Overlay")
    assert reference.endswith("from kit_base to app_client")


def test_grouped_crate_paths_include_each_leaf_and_resolve_root_reexports(
    root: Path,
) -> None:
    write(root, f"{SCENE}/lib.rs", "pub use table::overlay::Overlay as Screen;\n")
    write(
        root,
        f"{SCENE}/pose.rs",
        "use crate::{\n    Screen,\n    table::{self, overlay::{Overlay as Alias, *}},\n};\n",
    )
    count, *references = purity.levels(root)
    assert "upward.pose measured 4, above 0" in count
    assert [line.split(": ", 1)[1] for line in references] == [
        "crate::Screen points upward from kit_base to app_client",
        "crate::table points upward from kit_base to game_table",
        "crate::table::overlay::Overlay points upward from kit_base to app_client",
        "crate::table::overlay::* points upward from kit_base to app_client",
    ]


def test_comments_and_literals_do_not_create_references(root: Path) -> None:
    write(
        root,
        f"{SCENE}/pose.rs",
        "// use crate::table;\n/* /* nested */ use crate::table; */\n"
        'const TEXT: &str = r#"crate::table::overlay"#;\n'
        'const WARNING: &str = "crate::table::overlay"; // beryl_protocol\n',
    )
    assert purity.levels(root) == []


def test_upward_manifest_dependency_fails_even_under_an_alias(root: Path) -> None:
    write(
        root,
        "rust/beryl3d/Cargo.toml",
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
    assert "just b3d levels --update" in found
    assert purity.levels(root, update=True) == []
    assert json.loads(path.read_text())["cap_allowances"]["game_table"] == {
        "lines": 6,
        "reason": reason,
    }
    write(root, source, "// a line of source\n" * 5)
    assert purity.levels(root, update=True) == []
    assert json.loads(path.read_text())["cap_allowances"] == {}


def test_only_app_client_can_use_the_web_scaffold(root: Path) -> None:
    path = root / purity.LEVELS_FILE
    config = json.loads(path.read_text())
    config["crates"]["scaffold_web"] = {"level": "scaffold", "order": 3}
    path.write_text(json.dumps(config))
    scaffold = "rust/beryl3d/crates/scaffold_web/Cargo.toml"
    write(root, scaffold, '[dependencies]\ngame_table = { path = "../game_table" }\n')
    dependency = '[dependencies]\nscaffold_web = { path = "../scaffold_web" }\n'
    write(root, f"{APP}/Cargo.toml", dependency)
    assert purity.levels(root) == []
    write(root, f"{GAME}/Cargo.toml", dependency)
    (found,) = purity.levels(root)
    assert found == f"{GAME}/Cargo.toml: game_table depends upward on scaffold_web"


def test_update_only_lowers_recorded_counts(root: Path) -> None:
    path = root / purity.LEVELS_FILE
    config = json.loads(path.read_text())
    config["upward"] = {"pose": 1}
    path.write_text(json.dumps(config))
    (found,) = purity.levels(root)
    assert "upward.pose measured 0, below 1; run `just b3d levels --update`" in found
    assert purity.levels(root, update=True) == []
    assert json.loads(path.read_text())["upward"] == {}
    write(root, f"{SCENE}/pose.rs", "use crate::table;\n")
    assert purity.levels(root, update=True)
    assert json.loads(path.read_text())["upward"] == {}


def test_protocol_lines_keep_their_allowance_when_a_module_moves(root: Path) -> None:
    path = root / purity.LEVELS_FILE
    config = json.loads(path.read_text())
    config["protocol"]["kit_base"] = 1
    path.write_text(json.dumps(config))
    source = "use beryl_protocol::types as beryl_protocol;\n"
    write(root, f"{SCENE}/pose.rs", source)
    assert purity.levels(root) == []
    (root / f"{SCENE}/pose.rs").unlink()
    write(root, f"{KIT}/src/pose.rs", source)
    write(
        root, f"{KIT}/Cargo.toml", '[dependencies]\nberyl_protocol = { path = "x" }\n'
    )
    assert purity.levels(root) == []
    (root / f"{KIT}/src/pose.rs").unlink()
    (found,) = purity.levels(root, update=True)
    assert (
        found == f"{KIT}/Cargo.toml: toolkit crate kit_base depends on beryl_protocol"
    )
    assert json.loads(path.read_text())["protocol"] == {"kit_base": 0}
