"""The launcher writes complete scaffold sets and leaves existing files alone."""

from pathlib import Path
from shutil import copyfile

import pytest

from beryl_tools.client import cli, new

ROOT: Path = Path(__file__).resolve().parents[2]
CRATES = "client/crates/"
PROTOCOL = "engine/crates/beryl_protocol/src/"
INSPECT_TESTS: str = CRATES + "beryl_inspect/tests/all/main.rs"
REGISTRY: str = CRATES + "app_client/src/client/features.rs"

CASES: dict[str, tuple[list[str], list[str], str]] = {
    "feature": (
        [
            REGISTRY,
            CRATES + "app_screens/src/screens/mod.rs",
            CRATES + "app_client/tests/all/main.rs",
        ],
        [
            CRATES + "app_screens/src/screens/test_scaffold/mod.rs",
            CRATES + "app_screens/src/screens/test_scaffold/view.rs",
            CRATES + "app_client/tests/all/feature_test_scaffold.rs",
            CRATES + "app_client/src/client/features/test_scaffold.rs",
        ],
        "app_screens/test-scaffold",
    ),
    "widget": (
        [
            CRATES + "kit_ui/src/widgets/mod.rs",
            CRATES + "kit_ui/src/widgets/catalog.rs",
            CRATES + "kit_probe/src/story/style.rs",
            CRATES + "kit_ui/tests/all/main.rs",
        ],
        [
            CRATES + "kit_ui/src/widgets/test_scaffold.rs",
            CRATES + "kit_ui/tests/all/widget_test_scaffold.rs",
        ],
        "kit_ui/test-scaffold",
    ),
    "policy": (
        [
            CRATES + tail
            for tail in (
                "kit_space/src/arrange.rs",
                "kit_space/src/arrange/layout.rs",
                "kit_space/tests/all/arrange.rs",
                "kit_probe/src/story/mod.rs",
                "kit_probe/src/story/parse.rs",
            )
        ],
        [],
        "kit_space/test-scaffold",
    ),
    "profile": (
        [
            "data/client/profiles/wide-compact.json",
            CRATES + "game_table/src/profiles/mod.rs",
            CRATES + "beryl_inspect/src/pages.rs",
            INSPECT_TESTS,
        ],
        [
            "data/client/profiles/test-scaffold.json",
            CRATES + "game_table/src/profiles/test_scaffold.rs",
            CRATES + "beryl_inspect/tests/all/profile_test_scaffold.rs",
        ],
        "game_table/test-scaffold",
    ),
    "effect": (
        ["data/client/effects/glow.json", INSPECT_TESTS],
        [
            "data/client/effects/test-scaffold.json",
            CRATES + "beryl_inspect/tests/all/effect_test_scaffold.rs",
        ],
        "kit_fx/test-scaffold",
    ),
    "verb": (
        [
            PROTOCOL + "script/" + file
            for file in ("verb.rs", "act.rs", "table.rs", "text.rs", "timing.rs")
        ]
        + [
            CRATES + "game_play/src/verbs/mod.rs",
            CRATES + "game_play/src/sound.rs",
            "data/ui-conventions.json",
            INSPECT_TESTS,
        ],
        [
            CRATES + "game_play/src/verbs/test_scaffold.rs",
            CRATES + "beryl_inspect/tests/all/verb_test_scaffold.rs",
        ],
        "game_play/test-scaffold",
    ),
    "story": ([], [], "game_table/test-scaffold"),
    "prompt-shape": (
        [
            REGISTRY,
            CRATES + "game_hud/src/prompts/mod.rs",
            CRATES + "game_probe/src/signature.rs",
            PROTOCOL + "prompt/view.rs",
            PROTOCOL + "prompt/text.rs",
            CRATES + "app_client/src/client/features/prompt.rs",
            INSPECT_TESTS,
        ],
        [
            CRATES + "game_hud/src/prompts/test_scaffold/mod.rs",
            CRATES + "game_hud/src/prompts/test_scaffold/view.rs",
            CRATES + "beryl_inspect/tests/all/prompt_test_scaffold.rs",
        ],
        "game_hud/prompt-test-scaffold",
    ),
}


@pytest.mark.parametrize("kind", CASES)
def test_every_kind_writes_its_story_and_files_once(
    kind: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    copied, created, scene = CASES[kind]
    for file in copied:
        target = tmp_path / file
        target.parent.mkdir(parents=True, exist_ok=True)
        copyfile(ROOT / file, target)
    monkeypatch.setattr(new.paths, "REPO", tmp_path)
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path / "cache"))
    args = ["new", kind, "test-scaffold"]
    if kind == "story":
        args += ["--kind", "table"]
    assert cli.main(args) == 0
    output = capsys.readouterr().out
    assert output.splitlines()[-1] == f"just client tree story/{scene}"
    for file in created:
        assert (tmp_path / file).is_file()
        assert file in output.splitlines()
    crate, name = scene.split("/")
    story = next((tmp_path / CRATES / crate).rglob(f"{name}.story"))
    assert story.read_text().startswith("story ")
    assert story.relative_to(tmp_path).as_posix() in output.splitlines()
    before = {path: path.read_bytes() for path in tmp_path.rglob("*") if path.is_file()}
    assert cli.main(args) == 2
    assert capsys.readouterr().err
    assert {path: path.read_bytes() for path in before} == before


def test_story_checks_its_name_and_capture_options(tmp_path: Path) -> None:
    for name in ("../outside", "match", "bad--name"):
        assert new.run(["story", name, "--kind", "table"], tmp_path) == 2
    assert new.run(["story", "half", "--kind", "table", "--at", "end"], tmp_path) == 2
    assert not (tmp_path / CRATES).exists()
