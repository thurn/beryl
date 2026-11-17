"""Set planning uses first printings, typed drafting gaps and sentence fallback."""

import argparse
import hashlib
import json
import shlex
from pathlib import Path

import pytest

from beryl_tools import card, card_store, paths, phase_data, set_plan

Json = card_store.Json


def spell(name: str, text: str, **fields: object) -> Json:
    return {
        "name": name,
        "first_set": "ths",
        "layout": "normal",
        "faces": [{"name": name, "type_line": "Instant", "oracle_text": text}],
        **fields,
    }


def translation(text: str, effect: Json) -> Json:
    return {
        "oracle_text": text,
        "abilities": [{"kind": "Spell", "description": text, "effect": effect}],
    }


def test_list_and_plan_partition_a_sets_first_printings(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    built = spell("Built", "Draw a card.")
    records: list[Json] = [
        built,
        spell("Exact draft", "Scry 1."),
        spell("Near clause", "Draw two cards."),
        spell("Tokens A", "Create a Cat token."),
        spell("Tokens B", "Create a Bird token."),
        spell("Unparsed", "Bargain away all memories."),
        spell("No shape", "", layout="meld"),
        spell("Escalated", "Draw a card."),
        spell("Census gap", "Bestow {2}.", keywords=["Bestow"]),
        spell("Old printing", "Draw a card.", first_set="lea", set="ths"),
        {
            "name": "Store",
            "first_set": "ths",
            "layout": "normal",
            "faces": [{"type_line": "Creature", "oracle_text": "Flying"}],
        },
    ]
    store = tmp_path / "store"
    card_store.write(
        store,
        [
            *(card_store.line("card", str(row["name"]), row) for row in records),
            card_store.line("card", "Alias", {"alias": "Built"}),
            card_store.line("token", "Token", spell("Token", "")),
        ],
    )
    folder = tmp_path / "cards" / "group" / "ths"
    folder.mkdir(parents=True)
    (folder / "built.rs").touch()
    escalations = tmp_path / "escalations.json"
    escalations.write_text(
        json.dumps({"Escalated": {"mechanic": "Custom rule", "rule": "CR 123"}})
    )
    sources = card.Sources(
        store=store, cards=tmp_path / "cards", escalations=escalations
    )

    assert set_plan.run_list(argparse.Namespace(code="THS"), sources) == 0
    listing = dict(row.split("\t") for row in capsys.readouterr().out.splitlines())
    assert len(listing) == 10
    assert listing["Built"] == "implemented"
    assert listing["Escalated"] == "escalated"
    assert listing["Store"] == "store"
    assert listing["Exact draft"] == "unwritten"

    (tmp_path / "census.json").write_text(
        json.dumps(
            {"uncovered": [{"kind": "keyword:Bestow"}, {"kind": "keyword:Flying"}]}
        )
    )
    (tmp_path / "phase").mkdir()
    (tmp_path / "phase" / "beryl-draft-results.json").write_text(
        json.dumps(
            {
                "cohorts": {
                    "sample": {
                        "cards": {
                            "Exact draft": {
                                "drafted": True,
                                "compiled": True,
                                "exact": False,
                            }
                        }
                    }
                }
            }
        )
    )
    monkeypatch.setattr(paths, "cache_root", lambda: tmp_path)
    monkeypatch.setattr(phase_data, "read_gaps", lambda: {"unparsed"})
    monkeypatch.setattr(
        phase_data,
        "read",
        lambda: {
            "exact draft": translation(
                "Scry 1.", {"type": "Scry", "count": {"type": "Fixed", "value": 1}}
            ),
            "tokens a": translation("Create a Cat token.", {"type": "Token"}),
            "tokens b": translation("Create a Bird token.", {"type": "Token"}),
            "unparsed": translation(
                "Bargain away all memories.", {"type": "OtherEffect"}
            ),
        },
    )
    assert set_plan.run_plan(argparse.Namespace(code="THS", beads=True), sources) == 0
    result = json.loads((tmp_path / "set-plan-ths.json").read_text())
    assert result["implemented"] == ["Built"]
    assert result["tiers"] == {
        "store": ["Store"],
        "drafted": ["Exact draft"],
        "composed": ["Near clause"],
        "vocabulary": ["Tokens A", "Tokens B", "Unparsed"],
        "mechanism": ["Census gap", "Escalated", "No shape"],
    }
    assert [
        (group["kind"], group["label"], group["cards"])
        for group in result["vocabulary"]
    ] == [
        ("phase", "effect:Token", ["Tokens A", "Tokens B"]),
        ("sentence", "bargain away all memories.", ["Unparsed"]),
    ]
    assert result["draft_audit"] == {
        "checked": 1,
        "drafted": 1,
        "compiled": 1,
        "exact": 0,
    }

    output = capsys.readouterr()
    commands = [shlex.split(line) for line in output.out.splitlines()]
    assert all(command[:2] == ["bd", "create"] for command in commands)
    assert [command[-1] for command in commands] == [
        "tier:opus",
        "tier:opus",
        "tier:haiku",
        "tier:sonnet",
    ]
    assert "Largest vocabulary groups" in output.err


def test_keyword_abilities_are_checked_without_a_cached_census(tmp_path: Path) -> None:
    missing = set_plan.missing_keywords(tmp_path, card.Sources())
    assert "bestow" in missing
    assert missing.isdisjoint({"enchant", "flying", "heroic"})


def test_bead_commands_batch_twenty_cards_and_quote_shell_text() -> None:
    plan = set_plan.Plan()
    plan.tiers["drafted"] = [f"Card $(echo '{index}')" for index in range(21)]
    first, last = [shlex.split(line) for line in set_plan.bead_lines("ths", plan)]
    assert plan.tiers["drafted"][19] in first[first.index("--description") + 1]
    assert plan.tiers["drafted"][20] not in first[first.index("--description") + 1]
    assert plan.tiers["drafted"][20] in last[last.index("--description") + 1]


def test_coverage_flags_gaps_in_the_pinned_release(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(paths, "cache_root", lambda: tmp_path)
    folder = tmp_path / "phase"
    folder.mkdir()
    coverage = folder / "coverage-data.json"
    coverage.write_text(
        json.dumps(
            {
                "card_data_hash": "release",
                "cards": [
                    {"card_name": "Unparsed", "supported": False, "gap_count": 1},
                    {"card_name": "Complete", "supported": True, "gap_count": 0},
                ],
            }
        )
    )
    pin = tmp_path / "pin.json"
    pin.write_text(
        json.dumps(
            {
                "card_data_hash": "release",
                "coverage_sha256": hashlib.sha256(coverage.read_bytes()).hexdigest(),
            }
        )
    )
    assert phase_data.read_gaps(pin) == {"unparsed"}
    coverage.write_text("{}")
    assert "differs from pinned Phase release" in str(phase_data.read_gaps(pin))
