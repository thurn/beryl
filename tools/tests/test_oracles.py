import argparse
import json
import subprocess
import sys
import tempfile
from collections.abc import Callable
from pathlib import Path

import pytest

from beryl_tools import deps, oracle_exports, oracles, xmage_java


def git(cwd: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=cwd, capture_output=True, text=True, check=True
    ).stdout.strip()


def upstream(path: Path) -> str:
    """A repository with two commits; returns the first, which the oracle pins."""
    path.mkdir()
    git(path, "init", "--quiet")
    for text in ("pinned", "newer"):
        (path / "README").write_text(text)
        git(path, "add", "README")
        git(path, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", text)
    return git(path, "rev-parse", "HEAD~1")


def test_setup_fetches_and_builds_once(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path / "cache"))
    pinned = upstream(tmp_path / "upstream")
    # The build stands in for Maven: it counts its runs in the checkout.
    oracle = oracles.Oracle(
        name="fake",
        repo=(tmp_path / "upstream").as_uri(),
        commit=pinned,
        build=(sys.executable, "-c", "open('builds', 'a').write('built\\n')"),
    )
    monkeypatch.setattr(oracles, "ORACLES", (oracle,))
    monkeypatch.setattr(oracles, "mvn", lambda *args: list(args))
    monkeypatch.setattr(oracles, "brew_prefix", lambda formula: tmp_path / formula)
    monkeypatch.setattr(oracles, "windows_jdk", lambda: tmp_path / "jdk")
    monkeypatch.setattr(oracles, "windows_maven", lambda: tmp_path)
    monkeypatch.setattr(oracles, "configure_forge", lambda: None)

    def summary() -> dict[str, object]:
        assert oracles.setup(argparse.Namespace()) == 0
        return json.loads(capsys.readouterr().out)["fake"]

    first = summary()
    assert (first["fetched"], first["built"]) == (True, True)
    checkout = oracles.checkout(oracle)
    assert git(checkout, "rev-parse", "HEAD") == pinned
    assert (checkout / "README").read_text() == "pinned"

    second = summary()
    assert (second["fetched"], second["built"]) == (False, False)
    assert (checkout / "builds").read_text() == "built\n"


def test_exporter_gaps_turn_failures_unsupported_and_report_stale_ones(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        oracles,
        "EXPORTER_GAPS",
        {
            ("xmage", "Generated", "Generated 1"): "a gap",
            ("xmage", "Generated", "Generated 2"): "another gap",
            ("xmage", "Generated", "Generated 3"): "an exporter refusal",
            ("forge", "Generated", "Generated 1"): "Forge's gap",
        },
    )
    result: dict[str, object] = {
        "pass": 1,
        "fail": 1,
        "allowlisted": 0,
        "unsupported": 1,
        "scenarios": [
            {
                "card": "Generated",
                "name": "Generated 1",
                "status": "fail",
                "failure": "x",
            },
            {"card": "Generated", "name": "Generated 2", "status": "pass"},
            {
                "card": "Generated",
                "name": "Generated 3",
                "status": "unsupported",
                "reason": "an exporter refusal",
            },
        ],
    }
    # Generated 2 passes, so its entry is stale; Forge's entry is not XMage's.
    assert oracles.apply_gaps("xmage", result) == ["Generated / Generated 2"]
    assert result["fail"] == 0 and result["unsupported"] == 2 and result["pass"] == 1
    gap = result["scenarios"][0]  # pyre-ignore[16]
    assert gap["status"] == "unsupported" and gap["reason"] == "exporter gap: a gap"
    assert "failure" not in gap


def test_generated_scenarios_are_marked() -> None:
    result: dict[str, object] = {
        "scenarios": [
            {"card": "Oust", "name": "generated 1: cast from hand", "status": "pass"},
            {"card": "Oust", "name": "Oust exiles", "status": "pass"},
        ]
    }
    oracles.mark_generated(result)
    marked = [s.get("generated", False) for s in result["scenarios"]]  # pyre-ignore[16]
    assert marked == [True, False]


SCENARIOS: list[dict[str, object]] = [
    {"card": "Bonecrusher Giant", "name": "casts both faces"},
    {"card": "Counterspell", "name": "counters a spell"},
    {"card": "Ponder", "name": "looks at three"},
]


def fake_oracles(monkeypatch: pytest.MonkeyPatch) -> list[list[str]]:
    """Replaces the dump and both oracles with stand-ins that pass every scenario; returns the
    cards each oracle played, then "sim" for each round of Forge's AI games."""
    played: list[list[str]] = []
    monkeypatch.setenv("BERYL_CACHE", tempfile.mkdtemp(prefix="beryl-oracle-test-"))

    def check(exports: oracle_exports.Exports, _work: Path) -> dict[str, object]:
        played.append([str(e["card"]) for e in exports.forge])
        scenarios = [{**e, "status": "pass"} for e in exports.forge]
        return {"pass": len(scenarios), "fail": 0, "scenarios": scenarios}

    monkeypatch.setattr(
        oracles, "dump_scenarios", lambda path, log, cards=(): SCENARIOS
    )

    def prepare(
        entries: list[dict[str, object]], checkout: Path
    ) -> oracle_exports.Exports:
        java = [
            xmage_java.Export(str(e["card"]), str(e["name"]), f"s{i}")
            for i, e in enumerate(entries)
        ]
        return oracle_exports.Exports(java, entries, [])

    monkeypatch.setattr(oracle_exports.Exports, "prepare", prepare)
    monkeypatch.setattr(oracles, "require_built", lambda: None)
    monkeypatch.setattr(oracles, "check_xmage", check)
    monkeypatch.setattr(oracles, "check_forge", check)

    def sim(_log: Path, _games: int) -> list[str]:
        played.append(["sim"])
        return []

    monkeypatch.setattr(oracles, "sim_forge", sim)
    return played


def oracle_args(
    *cards: str,
    no_sim: bool = False,
    lock_wait: float | None = None,
) -> argparse.Namespace:
    return argparse.Namespace(cards=list(cards), no_sim=no_sim, lock_wait=lock_wait)


def test_no_sim_skips_forge_s_games_and_cards_narrow_the_scenarios(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    played = fake_oracles(monkeypatch)
    assert oracles.oracle(oracle_args("Ponder", no_sim=True)) == 0
    assert played == [["Ponder"], ["Ponder"]]
    played.clear()
    assert oracles.oracle(oracle_args()) == 0
    assert played == [["Bonecrusher Giant", "Counterspell", "Ponder"]] * 2 + [["sim"]]


def test_an_adventure_cards_full_name_selects_its_scenarios(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    played = fake_oracles(monkeypatch)
    assert oracles.oracle(oracle_args("Bonecrusher Giant // Stomp", no_sim=True)) == 0
    assert played == [["Bonecrusher Giant"], ["Bonecrusher Giant"]]


def test_a_double_faced_store_name_selects_the_front_faces_scenarios(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    played = fake_oracles(monkeypatch)
    monkeypatch.setattr(
        oracles,
        "dump_scenarios",
        lambda path, log, cards: [
            *SCENARIOS,
            {"card": "Jace, Vryn's Prodigy", "name": "returns transformed"},
        ],
    )
    assert (
        oracles.oracle(
            oracle_args("Jace, Vryn's Prodigy // Jace, Telepath Unbound", no_sim=True)
        )
        == 0
    )
    assert played == [["Jace, Vryn's Prodigy"]] * 2


def test_a_run_waiting_past_the_lock_wait_cannot_run(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    played = fake_oracles(monkeypatch)
    with oracles.locked():
        code = oracles.oracle(oracle_args("Ponder", lock_wait=0.2))
    assert code == deps.CANNOT_RUN
    assert played == []


def test_compiled_exports_of_earlier_runs_are_deleted_before_xmage_runs(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path))
    stale = oracles.compiled_tests() / "GoneCardBerylTest.class"
    stale.parent.mkdir(parents=True)
    stale.write_bytes(b"")
    oracles.clear_compiled_tests()
    assert not stale.exists()
