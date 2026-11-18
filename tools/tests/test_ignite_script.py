"""The script, app and mobile check families: their manifest and their adapter."""

from __future__ import annotations

import hashlib
import json
import stat
import tomllib
from pathlib import Path

import pytest

from beryl_tools import paths
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


def test_the_manifest_declares_the_three_families_and_their_product() -> None:
    manifest = tomllib.loads(MANIFEST.read_text())
    assert set(manifest["family"]) == {"script", "app", "mobile"}
    assert manifest["product"]["script-cli"]["outputs"] == ["rust/target"]
    for name, family in manifest["family"].items():
        assert family["adapter"] == "jsonl"
        assert family["kind"] == "regression"
        assert f"list {name}" in family["list"]
        assert "{checks}" in family["run"]
        assert family["inputs"]
    assert manifest["family"]["app"]["platforms"] == ["macos"]
    assert manifest["family"]["mobile"]["platforms"] == ["macos"]
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
#!/bin/sh
case "$1" in
specs-check)
  shift
  for c in "${@:-alpha beta}"; do
    for card in $c; do echo "ok    $card / start  (Start)"; done
  done
  echo "2 moments, 0 candidates skipped" ;;
script-check)
  [ -n "$BERYL_CHECK_RESULTS" ] || exit 9
  echo '{"check":"script:alpha/start/yours","outcome":"pass","seconds":0.1}' >> "$BERYL_CHECK_RESULTS"
  echo '{"check":"script:beta/start/yours","outcome":"fail","seconds":0.2}' >> "$BERYL_CHECK_RESULTS"
  echo "FAILED: script:beta/start/yours"
  echo "    thread panicked: boom"
  exit 1 ;;
esac
"""


def repository(root: Path) -> None:
    cards = root / "rust" / "cards" / "cards_1" / "set"
    cards.mkdir(parents=True)
    (cards / "alpha.rs").write_text(card("Alpha", "{U}"))
    (cards / "beta.rs").write_text(card("Beta"))
    (root / "ui").mkdir()
    (root / "ui" / "fixtures.json").write_text(
        json.dumps(
            {
                "fixtures": [
                    {"name": "table-opening", "table": {}},
                    {"name": "second", "until": "first target decision for seat 1"},
                ]
            }
        )
    )
    binary = root / "rust" / "target" / "debug" / "beryl_cli"
    binary.parent.mkdir(parents=True)
    binary.write_text(CHECKER)
    binary.chmod(binary.stat().st_mode | stat.S_IEXEC)


@pytest.fixture
def repo(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    monkeypatch.delenv("CARGO_TARGET_DIR", raising=False)
    repository(tmp_path)
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
        "mobile:aarch64-linux-android",
    ]
