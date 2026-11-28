"""The client's source lints: scene-purity, lints-match and data-names."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from beryl_tools.client import purity

CRATES = "client/crates"
SCENE_CRATES = (
    "kit_base",
    "kit_text",
    "kit_frame",
    "kit_ui",
    "kit_space",
    "kit_fx",
    "kit_scene",
    "kit_probe",
    "game_art",
    "game_table",
    "game_play",
    "game_hud",
    "game_manual",
    "game_probe",
    "app_screens",
    "app_client",
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
    write(tmp_path, manifest, '[dependencies]\napp_client = { path = "x" }\n')
    assert purity.scene_purity(tmp_path) == []
    write(tmp_path, manifest, '[dependencies]\nwgpu = "30.0.2"\n')
    (found,) = purity.scene_purity(tmp_path)
    assert "beryl_inspect depends on wgpu" in found


@pytest.mark.parametrize(
    "settings_fields, name, line",
    [
        (
            "settings: crate::settings::Settings,\n"
            "    transient_settings: app_screens::screens::settings::State,",
            "transient_settings",
            5,
        ),
        (
            "settings: crate::settings::Settings,\n    confirm_concede: bool,",
            "confirm_concede",
            5,
        ),
        ("settings:\n        <SettingsSheet as Feature>::State,", "settings", 4),
    ],
)
def test_client_state_keeps_transient_settings_in_the_registry(
    tmp_path: Path, settings_fields: str, name: str, line: int
) -> None:
    client = f"{CRATES}/app_client/src/client/mod.rs"
    shared = (
        "pub struct Client {\n"
        "    features: Box<features::Features>,\n"
        "    routes: input::Routes,\n"
    )
    check = purity.SOURCE_RULES["client-state"]
    write(tmp_path, client, shared + "    settings: crate::settings::Settings,\n}\n")
    assert check(tmp_path) == []

    write(tmp_path, client, shared + f"    {settings_fields}\n}}\n")
    (found,) = check(tmp_path)
    assert found.startswith(f"{client}:{line}: Client field starting with `{name}`")
    assert "Feature registered in client/features.rs" in found


def test_the_lint_tables_must_match(tmp_path: Path) -> None:
    write(tmp_path, "engine/Cargo.toml", LINTS)
    write(tmp_path, "client/Cargo.toml", LINTS)
    assert purity.lints_match(tmp_path) == []
    write(tmp_path, "client/Cargo.toml", LINTS.replace("deny", "warn"))
    assert len(purity.lints_match(tmp_path)) == 1


def test_a_verb_key_names_a_verb_or_a_case(tmp_path: Path) -> None:
    conventions = {
        "verbs": [{"name": "fly", "cases": {"draw": {}}}],
        "effects": [{"name": "glow"}],
    }
    write(tmp_path, "data/ui-conventions.json", json.dumps(conventions))
    verbs = "data/client/verbs.json"
    write(tmp_path, "data/client/effects/glow.json", "{}")
    write(tmp_path, verbs, json.dumps({"verbs": {"fly": {}, "draw": {}}}))
    assert purity.data_names(tmp_path) == []
    write(tmp_path, verbs, json.dumps({"verbs": {"flee": {}}}))
    (found,) = purity.data_names(tmp_path)
    assert "verbs.flee names no verb or case" in found


def test_each_convention_effect_has_a_definition(tmp_path: Path) -> None:
    conventions = {"verbs": [], "effects": [{"name": "glow"}]}
    write(tmp_path, "data/ui-conventions.json", json.dumps(conventions))
    write(tmp_path, "data/client/verbs.json", '{"verbs": {}}')
    write(tmp_path, "data/client/effects/glo.json", "{}")
    unknown, missing = purity.data_names(tmp_path)
    assert "glo.json: no such effect" in unknown
    assert "glow.json: missing definition" in missing


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


def test_the_app_names_the_recorder_only_in_its_optional_playground(
    tmp_path: Path,
) -> None:
    manifest = f"{CRATES}/beryl_client/Cargo.toml"
    playground = f"{CRATES}/beryl_client/src/playground.rs"
    write(tmp_path, manifest, "[dependencies]\nberyl_cli = { optional = true }\n")
    write(tmp_path, playground, "use beryl_cli::specs;\n")
    assert purity.scene_purity(tmp_path) == []
    session = f"{CRATES}/beryl_client/src/session.rs"
    write(tmp_path, session, "use beryl_cli::specs;\n")
    (found,) = purity.scene_purity(tmp_path)
    assert found.startswith(f"{session}:1: names beryl_cli")
    write(tmp_path, session, "")
    write(tmp_path, manifest, "[dependencies]\nberyl_cli = {}\n")
    (found,) = purity.scene_purity(tmp_path)
    assert "beryl_client depends on beryl_cli" in found


def test_the_scene_names_no_clock_however_it_is_imported(tmp_path: Path) -> None:
    scene = f"{CRATES}/app_client/src/lib.rs"
    write(
        tmp_path,
        scene,
        "use std::collections::{BTreeMap, VecDeque};\nuse std::{\n    fmt,\n"
        "    time::Duration,\n};\n",
    )
    (found,) = purity.scene_purity(tmp_path)
    assert found.startswith(f"{scene}:4: app_client names std::time")
    write(tmp_path, scene, "use std::collections::*;\ntype Seen = HashSet<u32>;\n")
    (found,) = purity.scene_purity(tmp_path)
    assert found.startswith(f"{scene}:2: app_client names HashSet")
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
    write(tmp_path, f"{CRATES}/beryl_client/Cargo.toml", session)
    write(tmp_path, f"{CRATES}/beryl_record/Cargo.toml", session)
    assert purity.scene_purity(tmp_path) == []
    cards = '[dev-dependencies]\nberyl_cards = { path = "x" }\n'
    write(tmp_path, f"{CRATES}/beryl_assets/Cargo.toml", cards)
    (found,) = purity.scene_purity(tmp_path)
    assert "beryl_assets depends on beryl_cards" in found


@pytest.mark.parametrize(
    "source",
    (
        "app_screens/src/screens/start.rs",
        "kit_ui/src/widgets/button.rs",
        "game_table/src/table/rows.rs",
        "game_play/src/hud/counts.rs",
        "game_hud/src/prompts/bar.rs",
        "game_hud/src/screens/game.rs",
        "game_manual/src/manual/lobby.rs",
        "game_manual/src/screens/table.rs",
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


def test_table_geometry_must_come_from_data(tmp_path: Path) -> None:
    path = f"{CRATES}/game_table/src/table/rows.rs"
    write(
        tmp_path,
        path,
        "let rounding = 0.01; // scene-literals: 0.01 covers projection rounding.\n"
        'let label = "block // attacker"; '
        "const WIDTH: f32 = 13.75; // scene-literals: 0.01 covers projection rounding.\n",
    )
    (found,) = purity.scene_literals(tmp_path)
    assert found.startswith(f"{path}:2: the literal 13.75")


def test_a_test_module_may_hold_numbers(tmp_path: Path) -> None:
    screen = f"{CRATES}/game_hud/src/hud/steps.rs"
    write(
        tmp_path,
        screen,
        "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() { let w = 1600.0; }\n}\nlet b = 3.0;\n",
    )
    (found,) = purity.scene_literals(tmp_path)
    assert f"{screen}:6:" in found


def test_protocol_literals_checks_sources_and_tests(tmp_path: Path) -> None:
    write(tmp_path, purity.LEVELS_FILE, '{"protocol_literals": {}}')
    path = f"{CRATES}/app_client/tests/story.rs"
    write(
        tmp_path,
        path,
        "fn view() -> beryl_protocol::protocol::PlayerView {\n"
        "    // ObjectView { name, ..object }\n"
        '    let text = "Step { act, cause }";\n'
        "    let other = TablePress::Step { at, active };\n"
        "    let object = ObjectView { name, ..object };\n"
        "    let step = beryl_protocol::script::Step\n"
        "        { act, cause: None };\n"
        "    PlayerView { ..view }\n"
        "}\n"
        "enum TablePress { Step { at: u8, active: bool } }\n",
    )
    found = purity.protocol_literals(tmp_path)
    assert len(found) == 3
    assert found[0].startswith(f"{path}:5: ObjectView literal")
    assert found[1].startswith(f"{path}:6: Step literal")
    assert found[2].startswith(f"{path}:8: PlayerView literal")


def test_a_prompt_struct_update_is_a_protocol_literal(tmp_path: Path) -> None:
    write(tmp_path, purity.LEVELS_FILE, '{"protocol_literals": {}}')
    path = f"{CRATES}/app_client/tests/prompt.rs"
    write(
        tmp_path,
        path,
        "fn copy(prompt: PromptView) -> PromptView {\n"
        "    PromptView { ..prompt }\n"
        "}\n",
    )
    (found,) = purity.protocol_literals(tmp_path)
    assert found.startswith(f"{path}:2: PromptView literal")


def test_protocol_literal_allowances_hold_exact_counts(tmp_path: Path) -> None:
    path = f"{CRATES}/beryl_inspect/src/stress.rs"
    write(tmp_path, purity.LEVELS_FILE, json.dumps({"protocol_literals": {path: 1}}))
    literal = "let step = Step { act, cause: None };\n"
    write(tmp_path, path, literal)
    assert purity.protocol_literals(tmp_path) == []
    write(tmp_path, path, literal * 2)
    (extra,) = purity.protocol_literals(tmp_path)
    assert extra.startswith(f"{path}:2: Step literal raises the count to 2, above 1")
    write(tmp_path, path, "")
    (stale,) = purity.protocol_literals(tmp_path)
    assert "dropped to 0 from 1" in stale


def test_profile_words_count_identifier_parts_outside_profiles(tmp_path: Path) -> None:
    base = "client/crates"
    write(
        tmp_path,
        f"{base}/game_table/src/layout.rs",
        """
        let phone_dock = DesktopMenu::PHONE;
        let megaphone = "phone";
        let phonebook = r#"desktop"#;
        let r#phone = 0; // phone
        /* desktop /* phone */ desktop */
        """,
    )
    write(tmp_path, f"{base}/game_table/src/profiles/mod.rs", "let phone = 1;")
    write(tmp_path, purity.LEVELS_FILE, '{"profile_words":{"game_table":4}}')
    assert purity.profile_word_counts(tmp_path) == {"game_table": 4}
    assert purity.profile_words(tmp_path) == []
    write(tmp_path, f"{base}/game_table/src/feature.rs", "fn showPhone() {}")
    assert "measured 5, above 4" in purity.profile_words(tmp_path)[0]
