"""The oracle check families' adapter: check names, each check's own input, the seeds `next`
yields and how the oracles' reports become results. None of these tests needs the oracles.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

from beryl_tools.ignite import oracle as adapter


def repo_with_lists(root: Path, divergences: list[dict[str, str]]) -> Path:
    (root / "data").mkdir()
    (root / "data" / "oracle-divergences.json").write_text(json.dumps(divergences))
    (root / "data" / "oracle-exporter-gaps.json").write_text("[]")
    return root


ENTRIES: list[dict[str, object]] = [
    {"card": "Trinket Mage", "name": "finds an artifact"},
    {"card": "Trinket Mage", "name": "finds nothing"},
    {"card": "Dandân", "name": "attacks / blocks + more"},
]


def digests(root: Path, exports: dict[str, str]) -> dict[str, str]:
    return {
        line["check"]: line["digest"]
        for line in adapter.lines("oracle", ENTRIES, exports, root)
    }


def test_a_check_per_scenario_and_engine_whose_names_round_trip(tmp_path: Path) -> None:
    root = repo_with_lists(tmp_path, [])
    exports = {f"{e['card']} / {e['name']}": "x" for e in ENTRIES}
    names = list(digests(root, exports))
    assert len(names) == len(ENTRIES) * len(adapter.ENGINES)
    assert not any(c.isspace() for name in names for c in name)
    parsed = {adapter.parse_name(name) for name in names}
    assert parsed == {
        (str(e["card"]), str(e["name"]), engine)
        for e in ENTRIES
        for engine in adapter.ENGINES
    }


def test_a_digest_changes_with_its_own_export_and_list_entries_only(
    tmp_path: Path,
) -> None:
    root = repo_with_lists(tmp_path, [])
    exports = {f"{e['card']} / {e['name']}": "x" for e in ENTRIES}
    before = digests(root, exports)
    after = digests(root, {**exports, "Trinket Mage / finds nothing": "y"})
    changed = {name for name in before if before[name] != after[name]}
    assert {adapter.parse_name(n)[:2] for n in changed} == {
        ("Trinket Mage", "finds nothing")
    }
    assert len(changed) == 2

    (root / "data" / "oracle-divergences.json").write_text(
        json.dumps(
            [
                {
                    "oracle": "xmage",
                    "card": "Dandân",
                    "scenario": "attacks / blocks + more",
                    "cr": "CR 1",
                    "reason": "the oracle is wrong",
                }
            ]
        )
    )
    listed = digests(root, exports)
    assert {n for n in before if before[n] != listed[n]} == {
        "oracle:Dand%C3%A2n/attacks+%2F+blocks+%2B+more/xmage"
    }


def fake_oracles(monkeypatch: pytest.MonkeyPatch) -> list[tuple[int, int]]:
    """Stubs out the oracles and the dump: seed s gives "Generated s" unless s is a multiple of
    three, which the runner refuses. Returns the (count, seed) of each dump."""
    dumps: list[tuple[int, int]] = []

    def dump(
        repo: Path, count: int, seed: int | None = None
    ) -> list[dict[str, object]]:
        assert seed is not None
        dumps.append((count, seed))
        card: dict[str, object] = {"card": "Trinket Mage", "name": "finds nothing"}
        generated: list[dict[str, object]] = []
        for s in range(seed, seed + count):
            if s % 3:
                generated.append({"card": adapter.GENERATED, "name": f"Generated {s}"})
        return [card, *generated]

    monkeypatch.setattr(adapter.oracles, "require_built", lambda: None)
    monkeypatch.setattr(adapter, "dump", dump)
    monkeypatch.setattr(
        adapter,
        "exported",
        lambda entries: {f"{e['card']} / {e['name']}": "x" for e in entries},
    )
    return dumps


def test_next_never_yields_a_scenario_it_yielded_before(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    fake_oracles(monkeypatch)
    root = repo_with_lists(tmp_path, [])
    state = tmp_path / "state.json"
    seen: set[str] = set()
    for _ in range(3):
        found = [
            line["check"]
            for line in adapter.next_checks("oracle-generated", root, state, 7)
        ]
        assert found, "a batch yields some scenarios"
        assert all(name.startswith("oracle-generated:") for name in found)
        assert not seen & set(found)
        seen |= set(found)
    assert adapter.list_checks("oracle-generated", root) == []


def test_run_plays_the_seeds_its_checks_name(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    dumps = fake_oracles(monkeypatch)
    entries, wanted = adapter.selected(
        "oracle-generated",
        ["oracle-generated:41/xmage", "oracle-generated:44/forge"],
        tmp_path,
    )
    assert dumps == [(4, 41)]
    assert [e["name"] for e in entries] == ["Generated 41", "Generated 44"]
    assert wanted == {
        "Generated / Generated 41": {"xmage"},
        "Generated / Generated 44": {"forge"},
    }


def scenario(name: str, **fields: object) -> dict[str, object]:
    return {"card": "Trinket Mage", "name": name, **fields}


def test_reports_become_results_for_the_checks_named_only() -> None:
    result: dict[str, object] = {
        "scenarios": [
            scenario("passes", status="pass"),
            scenario("stale", status="pass", stale_allowlist=True),
            scenario(
                "known", status="allowlisted", cr="CR 701.23b", reason="XMage is wrong"
            ),
            scenario("cannot", status="unsupported", reason="XMage lacks Mind Bend"),
            scenario("breaks", status="fail", failures=["life 20, expected 18"]),
            scenario("not named", status="fail", failures=["x"]),
        ]
    }
    names = ["passes", "stale", "known", "cannot", "breaks", "missing"]
    wanted = {f"Trinket Mage / {n}": {"forge"} for n in names}
    lines = adapter.report("oracle", "forge", result, wanted)
    by_name = {adapter.parse_name(line["check"])[1]: line for line in lines}
    assert set(by_name) == {"passes", "stale", "known", "cannot", "breaks"}
    assert by_name["passes"] == {
        "check": "oracle:Trinket+Mage/passes/forge",
        "outcome": "pass",
    }
    assert by_name["stale"]["outcome"] == "fail"
    assert "oracle-divergences.json" in by_name["stale"]["message"]
    assert by_name["known"]["outcome"] == "pass"
    assert "XMage is wrong" in by_name["known"]["message"]
    assert by_name["cannot"] == {
        "check": "oracle:Trinket+Mage/cannot/forge",
        "outcome": "pass",
        "message": "unsupported: XMage lacks Mind Bend",
    }
    assert by_name["breaks"]["outcome"] == "fail"
    assert by_name["breaks"]["message"] == "life 20, expected 18"
    # The xmage checks were not named, so the forge report gives none of them.
    assert adapter.report("oracle", "xmage", result, wanted) == []


def test_a_stale_exporter_gap_fails() -> None:
    line = adapter.verdict(scenario("x", status="pass"), stale_gap=True)
    assert line["outcome"] == "fail"
    assert "oracle-exporter-gaps.json" in line["message"]
