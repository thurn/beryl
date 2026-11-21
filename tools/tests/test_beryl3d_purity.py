"""Beryl 3D's source lints: scene-purity, lints-match and data-names."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from beryl_tools.beryl3d import purity

CRATES = "rust/beryl3d/crates"
SCENE_CRATES = (
    "beryl_scene",
    "kit_base",
    "kit_text",
    "kit_frame",
    "kit_ui",
    "kit_space",
    "kit_fx",
    "kit_scene",
    "kit_probe",
    "game_table",
    "game_play",
)
LINTS = '[workspace.lints.rust]\nwarnings = "deny"\n'


def write(root: Path, rel: str, text: str) -> None:
    (root / rel).parent.mkdir(parents=True, exist_ok=True)
    (root / rel).write_text(text)


@pytest.mark.parametrize("crate", SCENE_CRATES)
def test_the_scene_names_no_clock_and_a_comment_may(tmp_path: Path, crate: str) -> None:
    scene = f"{CRATES}/{crate}/src/lib.rs"
    write(tmp_path, scene, "// std::time is banned here.\nuse glam::Vec3;\n")
    assert purity.scene_purity(tmp_path) == []
    write(tmp_path, scene, "\nlet t = std::time::Instant::now();\n")
    (found,) = purity.scene_purity(tmp_path)
    assert found.startswith(f"{scene}:2: {crate} names std::time")


def test_only_the_renderer_calls_wgpu(tmp_path: Path) -> None:
    write(tmp_path, f"{CRATES}/beryl_render/src/lib.rs", "wgpu::Device::new();\n")
    assert purity.scene_purity(tmp_path) == []
    write(tmp_path, f"{CRATES}/beryl_host/src/lib.rs", "wgpu::Device::new();\n")
    (found,) = purity.scene_purity(tmp_path)
    assert "beryl_host calls wgpu" in found


def test_the_inspector_has_no_gpu_dependency(tmp_path: Path) -> None:
    manifest = f"{CRATES}/beryl_inspect/Cargo.toml"
    write(tmp_path, manifest, '[dependencies]\nberyl_scene = { path = "x" }\n')
    assert purity.scene_purity(tmp_path) == []
    write(tmp_path, manifest, '[dependencies]\nwgpu = "30.0.2"\n')
    (found,) = purity.scene_purity(tmp_path)
    assert "beryl_inspect depends on wgpu" in found


def test_the_lint_tables_must_match(tmp_path: Path) -> None:
    write(tmp_path, "rust/Cargo.toml", LINTS)
    write(tmp_path, "rust/beryl3d/Cargo.toml", LINTS)
    assert purity.lints_match(tmp_path) == []
    write(tmp_path, "rust/beryl3d/Cargo.toml", LINTS.replace("deny", "warn"))
    assert len(purity.lints_match(tmp_path)) == 1


def test_a_verb_key_names_a_verb_or_a_case(tmp_path: Path) -> None:
    conventions = {"verbs": [{"name": "fly", "cases": {"draw": {}}}]}
    write(tmp_path, "data/ui-conventions.json", json.dumps(conventions))
    space = "data/beryl3d/space.json"
    write(tmp_path, space, json.dumps({"verbs": {"fly": {}, "draw": {}}}))
    assert purity.data_names(tmp_path) == []
    write(tmp_path, space, json.dumps({"verbs": {"flee": {}}}))
    (found,) = purity.data_names(tmp_path)
    assert "verbs.flee names no verb or case" in found


def test_only_the_recorder_file_names_the_recorder(tmp_path: Path) -> None:
    recorder = f"{CRATES}/beryl_record/src/recorder.rs"
    write(tmp_path, recorder, "use beryl_cli::fixture;\n")
    write(
        tmp_path,
        f"{CRATES}/beryl_record/Cargo.toml",
        "[dependencies]\nberyl_cli = {}\n",
    )
    assert purity.scene_purity(tmp_path) == []
    test = f"{CRATES}/beryl_inspect/tests/pack.rs"
    write(tmp_path, test, "use beryl_cli::specs;\n")
    (found,) = purity.scene_purity(tmp_path)
    assert found.startswith(f"{test}:1: names beryl_cli")
    write(tmp_path, test, "")
    write(
        tmp_path,
        f"{CRATES}/beryl_inspect/Cargo.toml",
        "[dev-dependencies]\nberyl_cli = {}\n",
    )
    (found,) = purity.scene_purity(tmp_path)
    assert "beryl_inspect depends on beryl_cli" in found


def test_the_scene_names_no_clock_however_it_is_imported(tmp_path: Path) -> None:
    scene = f"{CRATES}/beryl_scene/src/lib.rs"
    write(
        tmp_path,
        scene,
        "use std::collections::{BTreeMap, VecDeque};\nuse std::{\n    fmt,\n"
        "    time::Duration,\n};\n",
    )
    (found,) = purity.scene_purity(tmp_path)
    assert found.startswith(f"{scene}:4: beryl_scene names std::time")
    write(tmp_path, scene, "use std::collections::*;\ntype Seen = HashSet<u32>;\n")
    (found,) = purity.scene_purity(tmp_path)
    assert found.startswith(f"{scene}:2: beryl_scene names HashSet")
    write(tmp_path, scene, "static COUNT: AtomicU32 = AtomicU32::new(0);\n")
    (found,) = purity.scene_purity(tmp_path)
    assert "names AtomicU32" in found


@pytest.mark.parametrize("crate", SCENE_CRATES)
def test_the_scene_depends_only_on_listed_crates(tmp_path: Path, crate: str) -> None:
    manifest = f"{CRATES}/{crate}/Cargo.toml"
    write(tmp_path, manifest, '[dependencies]\nglam = "0.34"\n')
    assert purity.scene_purity(tmp_path) == []
    write(tmp_path, manifest, '[dependencies]\nglam = "0.34"\nrand = "0.9"\n')
    (found,) = purity.scene_purity(tmp_path)
    assert found.startswith(f"{manifest}: {crate} depends on rand")


def test_only_the_app_and_the_recorder_depend_on_the_engine(tmp_path: Path) -> None:
    session = '[dependencies]\nberyl_session = { path = "x" }\n'
    write(tmp_path, f"{CRATES}/beryl_3d/Cargo.toml", session)
    write(tmp_path, f"{CRATES}/beryl_record/Cargo.toml", session)
    assert purity.scene_purity(tmp_path) == []
    cards = '[dev-dependencies]\nberyl_cards = { path = "x" }\n'
    write(tmp_path, f"{CRATES}/beryl_assets/Cargo.toml", cards)
    (found,) = purity.scene_purity(tmp_path)
    assert "beryl_assets depends on beryl_cards" in found


@pytest.mark.parametrize(
    "source",
    (
        "beryl_scene/src/screens/start.rs",
        "kit_ui/src/widgets/button.rs",
        "game_play/src/hud/counts.rs",
    ),
)
def test_elements_take_their_sizes_from_tokens(tmp_path: Path, source: str) -> None:
    path = f"{CRATES}/{source}"
    write(tmp_path, path, "let a = row().grow(1.0).margin([0.0, 0.5, 2.0, 1.0]);\n")
    assert purity.scene_literals(tmp_path) == []
    write(tmp_path, path, "// gap(9.0)\nlet a = row()\n    .gap(12.0);\n")
    (found,) = purity.scene_literals(tmp_path)
    assert found.startswith(f"{path}:3: the literal 12.0")
    assert "data/design-tokens.json" in found


def test_a_test_module_may_hold_numbers(tmp_path: Path) -> None:
    screen = f"{CRATES}/beryl_scene/src/hud/steps.rs"
    write(
        tmp_path,
        screen,
        "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() { let w = 1600.0; }\n}\nlet b = 3.0;\n",
    )
    (found,) = purity.scene_literals(tmp_path)
    assert f"{screen}:6:" in found
