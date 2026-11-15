"""Beryl 3D's source lints: scene-purity, lints-match and data-names."""

from __future__ import annotations

import json
from pathlib import Path

from beryl_tools.beryl3d import purity

CRATES = "rust/beryl3d/crates"
LINTS = '[workspace.lints.rust]\nwarnings = "deny"\n'


def write(root: Path, rel: str, text: str) -> None:
    (root / rel).parent.mkdir(parents=True, exist_ok=True)
    (root / rel).write_text(text)


def test_the_scene_names_no_clock_and_a_comment_may(tmp_path: Path) -> None:
    scene = f"{CRATES}/beryl_scene/src/lib.rs"
    write(tmp_path, scene, "// std::time is banned here.\nuse glam::Vec3;\n")
    assert purity.scene_purity(tmp_path) == []
    write(tmp_path, scene, "\nlet t = std::time::Instant::now();\n")
    (found,) = purity.scene_purity(tmp_path)
    assert found.startswith(f"{scene}:2: beryl_scene names std::time")


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
    recorder = f"{CRATES}/beryl_inspect/src/recorder.rs"
    write(tmp_path, recorder, "use beryl_cli::fixture;\n")
    write(
        tmp_path,
        f"{CRATES}/beryl_inspect/Cargo.toml",
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
        f"{CRATES}/beryl_3d/Cargo.toml",
        "[dev-dependencies]\nberyl_cli = {}\n",
    )
    (found,) = purity.scene_purity(tmp_path)
    assert "beryl_3d depends on beryl_cli" in found
