import argparse
import json
import subprocess
import sys
from pathlib import Path

import pytest

from beryl_tools import oracles


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
            ("forge", "Generated", "Generated 1"): "Forge's gap",
        },
    )
    result: dict[str, object] = {
        "pass": 1,
        "fail": 1,
        "allowlisted": 0,
        "unsupported": 0,
        "scenarios": [
            {
                "card": "Generated",
                "name": "Generated 1",
                "status": "fail",
                "failure": "x",
            },
            {"card": "Generated", "name": "Generated 2", "status": "pass"},
        ],
    }
    # Generated 2 passes, so its entry is stale; Forge's entry is not XMage's.
    assert oracles.apply_gaps("xmage", result) == ["Generated / Generated 2"]
    assert result["fail"] == 0 and result["unsupported"] == 1 and result["pass"] == 1
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
    {"card": "Counterspell", "name": "counters a spell"},
    {"card": "Ponder", "name": "looks at three"},
]


def fake_oracles(monkeypatch: pytest.MonkeyPatch) -> list[list[str]]:
    """Replaces the dump and both oracles with stand-ins that pass every scenario; returns the
    cards each oracle played, then "sim" for each round of Forge's AI games."""
    played: list[list[str]] = []

    def check(entries: list[dict[str, object]], _work: Path) -> dict[str, object]:
        played.append([str(e["card"]) for e in entries])
        scenarios = [{**e, "status": "pass"} for e in entries]
        return {"pass": len(scenarios), "fail": 0, "scenarios": scenarios}

    monkeypatch.setattr(
        oracles, "dump_scenarios", lambda path, log, cards=(): SCENARIOS
    )
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
    select: bool = False,
) -> argparse.Namespace:
    return argparse.Namespace(
        cards=list(cards), no_sim=no_sim, lock_wait=lock_wait, select=select
    )


def test_select_plays_slow_cis_cards_and_logs_why(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    played = fake_oracles(monkeypatch)
    results = tmp_path / "results.jsonl"
    monkeypatch.setenv("BERYL_CHECK_RESULTS", str(results))
    chosen: dict[str, str] | None = {"Ponder": "card file changed"}
    monkeypatch.setattr(oracles.oracle_selection, "slow_ci", lambda every: chosen)
    assert oracles.oracle(oracle_args(select=True, no_sim=True)) == 0
    assert played == [["Ponder"], ["Ponder"]]
    logged = [json.loads(line) for line in results.read_text().splitlines()]
    assert {r["chosen"] for r in logged} == {"card file changed"}
    # Cards named too are a rerun's: they narrow the run.
    played.clear()
    assert oracles.oracle(oracle_args("Counterspell", select=True, no_sim=True)) == 0
    assert played == [["Counterspell"], ["Counterspell"]]
    # None selects every card.
    played.clear()
    chosen = None
    assert oracles.oracle(oracle_args(select=True, no_sim=True)) == 0
    assert played == [["Counterspell", "Ponder"]] * 2


def test_no_sim_skips_forge_s_games_and_cards_narrow_the_scenarios(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    played = fake_oracles(monkeypatch)
    assert oracles.oracle(oracle_args("Ponder", no_sim=True)) == 0
    assert played == [["Ponder"], ["Ponder"]]
    played.clear()
    assert oracles.oracle(oracle_args()) == 0
    assert played == [["Counterspell", "Ponder"]] * 2 + [["sim"]]


def test_a_run_waiting_past_the_lock_wait_skips_and_passes(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    played = fake_oracles(monkeypatch)
    results = tmp_path / "results.jsonl"
    monkeypatch.setenv("BERYL_CHECK_RESULTS", str(results))
    with oracles.locked():
        assert oracles.oracle(oracle_args("Ponder", lock_wait=0.2)) == 0
    assert played == []
    logged = [json.loads(line) for line in results.read_text().splitlines()]
    assert [(r["check"], r["outcome"]) for r in logged] == [
        ("oracle:Ponder/looks at three/xmage", "skip"),
        ("oracle:Ponder/looks at three/forge", "skip"),
    ]
