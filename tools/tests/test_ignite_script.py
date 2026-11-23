"""The script, app and mobile check families: their manifest and their adapter."""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import tomllib
from collections.abc import Callable
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import pytest

from beryl_tools import android_sdk, paths
from beryl_tools.ignite import script as adapter

MANIFEST: Path = paths.REPO / "ignite" / "checks" / "script.toml"
FORBIDDEN: list[str] = [
    name
    for name in (
        "tokens lanes step_runner selection rust_fit python_fit runlog runstore verdicts budget "
        "hang flaky bisect slow_ci sweep nightly warm caches incidents deadline clock "
        "check_value checks"
    ).split()
]


def test_the_manifest_declares_the_families_and_their_products() -> None:
    manifest = tomllib.loads(MANIFEST.read_text())
    assert set(manifest["family"]) == {"script", "app", "mobile", "android"}
    assert manifest["family"]["script"]["needs"] == ["cli", "python-env"]
    for name, family in manifest["family"].items():
        assert family["adapter"] == "jsonl"
        assert family["kind"] == "regression"
        assert f"list {name}" in family["list"]
        assert "{checks}" in family["run"]
        assert family["inputs"]
    assert manifest["family"]["app"]["platforms"] == ["macos", "windows"]
    assert manifest["family"]["mobile"]["platforms"] == ["macos"]
    assert manifest["family"]["android"]["platforms"] == ["macos", "windows"]
    # A card is its own check's input, so the family must not read every card's file.
    assert not any(
        p.startswith("rust/cards/") and p.count("/") >= 4
        for p in manifest["family"]["script"]["inputs"]
    )


def test_the_adapter_imports_no_orchestration_module() -> None:
    source = (Path(adapter.__file__)).read_text()
    for name in FORBIDDEN:
        assert f"from . import {name}" not in source
        assert f"beryl_tools.{name}" not in source.replace("beryl_tools.ignite", "")


def test_slugs_follow_the_checker() -> None:
    assert adapter.slug("Currency Converter") == "currency-converter"
    assert adapter.slug("Jace, Vryn's Prodigy") == "jace-vryns-prodigy"
    assert adapter.slug("Dandân") == "dandn"


def card(name: str, cost: str = "") -> str:
    return (
        "// Generated from the card store by `just card header`.\n"
        f"//! {name}{' ' + cost if cost else ''}\n//! Creature\n"
        "// End of the generated card header.\n"
    )


CHECKER = """\
import os
import sys
from pathlib import Path

if sys.argv[1] == 'specs-check':
    for card in sys.argv[2:] or ['alpha', 'beta']:
        print(f'ok    {card} / start  (Start)')
    print('2 moments, 0 candidates skipped')
elif sys.argv[1] == 'script-check':
    with Path(os.environ['BERYL_CHECK_RESULTS']).open('a') as out:
        print('{"check":"script:alpha/start/yours","outcome":"pass","seconds":0.1}', file=out)
        print('{"check":"script:beta/start/yours","outcome":"fail","seconds":0.2}', file=out)
    print('FAILED: script:beta/start/yours')
    print('    thread panicked: boom')
    sys.exit(1)
"""


def repository(root: Path) -> None:
    cards = root / "rust" / "cards" / "cards_1" / "set"
    cards.mkdir(parents=True)
    (cards / "alpha.rs").write_text(card("Alpha", "{U}"))
    (cards / "beta.rs").write_text(card("Beta"))
    (root / "data" / "beryl3d").mkdir(parents=True)
    (root / "data/beryl3d/fixtures.json").write_text(
        json.dumps(
            {
                "fixtures": [
                    {"name": "table-opening", "table": {}},
                    {"name": "second", "until": "first target decision for seat 1"},
                ]
            }
        )
    )
    binary = adapter.binary(root)
    binary.parent.mkdir(parents=True)
    binary.write_text(CHECKER)


@pytest.fixture
def repo(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    monkeypatch.delenv("CARGO_TARGET_DIR", raising=False)
    repository(tmp_path)
    binary: str = str(adapter.binary(tmp_path))
    run: Callable[..., subprocess.CompletedProcess[Any]] = subprocess.run

    def launch(command: list[str], **kwargs: Any) -> subprocess.CompletedProcess[Any]:
        if command[0] == binary:
            command = [sys.executable, *command]
        return run(command, **kwargs)

    monkeypatch.setattr(subprocess, "run", launch)
    return tmp_path


def test_a_card_asked_for_lists_its_checks_with_its_own_file(repo: Path) -> None:
    listed = adapter.list_checks("script", repo, ["Alpha"])
    assert listed == [
        {
            "check": f"script:alpha/start/{seat}",
            "inputs": ["rust/cards/cards_1/set/alpha.rs"],
        }
        for seat in ("yours", "theirs")
    ]


def test_a_split_card_slug_maps_to_its_card_file(repo: Path) -> None:
    folder = repo / "rust/cards/cards_1/set"
    (folder / "fire_ice.rs").write_text(card("Fire", "{1}{R}"))
    alpha = folder / "alpha.rs"
    alpha.write_text(alpha.read_text() + "const OTHER: &str = Card::FireIce;\n")

    assert adapter.list_checks("script", repo, ["Fire // Ice"]) == [
        {
            "check": f"script:fire-ice/start/{seat}",
            "inputs": ["rust/cards/cards_1/set/fire_ice.rs"],
        }
        for seat in ("yours", "theirs")
    ]
    assert adapter.list_checks("script", repo, ["Alpha"])[0]["inputs"] == [
        "rust/cards/cards_1/set/alpha.rs",
        "rust/cards/cards_1/set/fire_ice.rs",
    ]


def test_every_card_and_fixture_is_listed_without_a_card(repo: Path) -> None:
    names = [c["check"] for c in adapter.list_checks("script", repo, [])]
    assert "script:beta/start/theirs" in names
    assert "script:fixtures/table-opening/yours" in names
    assert "script:fixtures/second/theirs" in names


def test_an_unknown_card_is_an_error(repo: Path) -> None:
    with pytest.raises(ValueError):
        adapter.list_checks("script", repo, ["gamma"])


def digest(root: Path, paths_read: list[str]) -> str:
    return hashlib.sha256(
        b"".join(p.encode() + (root / p).read_bytes() for p in paths_read)
    ).hexdigest()


def test_editing_another_cards_file_leaves_a_checks_inputs_unchanged(
    repo: Path,
) -> None:
    def hashes() -> dict[str, str]:
        return {
            c["check"]: digest(repo, c["inputs"])
            for c in adapter.list_checks("script", repo, [])
            if c["check"].startswith("script:alpha/") or "beta" in c["check"]
        }

    before = hashes()
    (repo / "rust/cards/cards_1/set/beta.rs").write_text(card("Beta", "{R}"))
    after = hashes()
    assert after["script:alpha/start/yours"] == before["script:alpha/start/yours"]
    assert after["script:beta/start/yours"] != before["script:beta/start/yours"]


def test_run_reports_each_result_with_the_panic_of_a_failure(repo: Path) -> None:
    results = repo / "results.jsonl"
    adapter.run_checks(
        "script",
        ["script:alpha/start/yours", "script:beta/start/yours"],
        repo,
        results,
    )
    lines = [json.loads(line) for line in results.read_text().splitlines()]
    assert lines == [
        {"check": "script:alpha/start/yours", "outcome": "pass"},
        {
            "check": "script:beta/start/yours",
            "outcome": "fail",
            "message": "thread panicked: boom",
        },
    ]


def test_app_and_mobile_list_one_check_per_target(repo: Path) -> None:
    assert [c["check"] for c in adapter.list_checks("app", repo, [])] == ["app:smoke"]
    assert [c["check"] for c in adapter.list_checks("mobile", repo, [])] == [
        "mobile:aarch64-apple-ios-sim",
    ]
    assert [c["check"] for c in adapter.list_checks("android", repo, [])] == [
        "android:aarch64-linux-android",
    ]


def test_android_uses_the_pinned_windows_tools(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sdk = tmp_path / "Android SDK"
    monkeypatch.setattr(android_sdk, "sys", SimpleNamespace(platform="win32"))
    monkeypatch.setenv("ANDROID_HOME", str(sdk))
    monkeypatch.setenv("NDK_HOME", str(sdk / "ndk" / "other"))
    env = android_sdk.ndk_env()
    tools = sdk / "ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64/bin"
    assert Path(env["CC_aarch64_linux_android"]) == tools / "clang.exe"
    assert Path(env["AR_aarch64_linux_android"]) == tools / "llvm-ar.exe"
    assert env["CFLAGS_aarch64_linux_android"] == "--target=aarch64-linux-android24"


def test_only_the_ios_simulator_check_clears_swift_caches(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    cleared: list[Path] = []
    environments: list[dict[str, str] | None] = []
    android_env = {"CC_aarch64_linux_android": "clang"}

    def cargo(
        command: list[str], *, env: dict[str, str] | None, **kwargs: Any
    ) -> subprocess.CompletedProcess[str]:
        environments.append(env)
        return subprocess.CompletedProcess[str](command, 0, "", "")

    monkeypatch.setattr(adapter, "clear_swift_module_caches", cleared.append)
    monkeypatch.setattr(android_sdk, "ndk_env", lambda: android_env)
    monkeypatch.setattr(subprocess, "run", cargo)

    assert adapter.run_mobile("aarch64-apple-ios-sim", tmp_path) == {"outcome": "pass"}
    assert adapter.run_mobile("aarch64-linux-android", tmp_path) == {"outcome": "pass"}
    assert cleared == [tmp_path]
    assert environments == [None, android_env]


def test_a_card_whose_scenarios_use_another_card_lists_that_cards_file_too(
    repo: Path,
) -> None:
    alpha = repo / "rust/cards/cards_1/set/alpha.rs"
    alpha.write_text(
        alpha.read_text() + 'Scenario::new("steals").hand(P1, &[Card::Beta]);\n'
    )
    listed = adapter.list_checks("script", repo, ["Alpha"])
    assert listed[0]["inputs"] == [
        "rust/cards/cards_1/set/alpha.rs",
        "rust/cards/cards_1/set/beta.rs",
    ]
    # Beta's checks do not read Alpha's file.
    assert adapter.list_checks("script", repo, ["Beta"])[0]["inputs"] == [
        "rust/cards/cards_1/set/beta.rs"
    ]


def test_a_card_named_by_its_spelled_out_name_or_through_another_card_is_read(
    repo: Path,
) -> None:
    folder = repo / "rust/cards/cards_1/set"
    (folder / "gamma.rs").write_text(
        card("Gamma") + "const FOUND: &str = names::DELTA;\n"
    )
    (folder / "delta.rs").write_text(card("Delta"))
    (folder / "omega.rs").write_text(card("Omega"))
    # Beta's scenarios name Omega, which Alpha's scenarios never play.
    beta = folder / "beta.rs"
    beta.write_text(
        beta.read_text()
        + 'pub mod scenarios {\n  Scenario::new("x").hand(P0, &[Card::Omega]);\n}\n'
    )
    alpha = folder / "alpha.rs"
    alpha.write_text(
        alpha.read_text()
        + 'Scenario::new("Alpha meets Beta").hand(P1, &["Beta", Token::Gamma]);\n'
    )
    listed = adapter.list_checks("script", repo, ["Alpha"])
    assert listed[0]["inputs"] == [
        f"rust/cards/cards_1/set/{name}.rs"
        for name in ("alpha", "beta", "delta", "gamma")
    ]


def test_card_references_match_names_variants_and_constants() -> None:
    assert adapter.letters("Jace, Vryn's Prodigy") == adapter.letters(
        "JaceVrynsProdigy"
    )
    assert adapter.letters("MANA_CRYPT") == adapter.letters("Mana Crypt")


def test_a_fixture_check_reads_native_definitions_and_every_card_file(
    repo: Path,
) -> None:
    checks = {c["check"]: c for c in adapter.list_checks("script", repo, [])}
    assert checks["script:fixtures/table-opening/yours"]["inputs"] == [
        "data/beryl3d/fixtures.json",
        "rust/cards",
    ]


def test_a_card_named_only_in_a_comment_is_not_a_reference() -> None:
    text = (
        '/// Copies "Mana Crypt" like Card::Ponder would.\n'
        "const A: &str = Card::Opt; // Card::Brainstorm\n"
        '/* Card::Divination */ const B: &str = "Lightning // Bolt";\n'
    )
    assert adapter.references(text) == {"opt", "lightningbolt"}
