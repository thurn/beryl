"""Read and check a ledger against a synthetic Scryfall cache."""

import argparse
import gzip
import json
from pathlib import Path

import pytest

from beryl_tools import card, card_store, rulings, set_groups

CARD = "Tundra"
FIRST: card_store.Json = {
    "oracle_id": "tundra",
    "published_at": "2024-01-02",
    "comment": "A first ruling.",
}
SECOND: card_store.Json = {**FIRST, "comment": "A different ruling on the same day."}
OTHER: card_store.Json = {
    "oracle_id": "bolt",
    "published_at": "2024-01-03",
    "comment": "A ruling for an unlisted card.",
}


def entry(ruling: card_store.Json, **evidence: object) -> card_store.Json:
    date, digest = rulings.key(ruling)
    return {"published_at": date, "hash": digest, **evidence}


@pytest.fixture
def inputs(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[card.Sources, Path, Path]:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path / "cache"))
    bulk = tmp_path / "cache" / "scryfall" / "rulings.jsonl.gz"
    bulk.parent.mkdir(parents=True)
    with gzip.open(bulk, "wt", encoding="utf-8") as out:
        out.writelines(
            json.dumps(ruling) + "\n" for ruling in (FIRST, SECOND, FIRST, OTHER)
        )

    store = tmp_path / "cards.tsv"
    record: card_store.Json = {
        "name": CARD,
        "oracle_id": "tundra",
        "first_set": "lea",
        "faces": [{"name": CARD}],
    }
    bolt: card_store.Json = {
        "name": "Lightning Bolt",
        "oracle_id": "bolt",
        "first_set": "lea",
    }
    card_store.write(
        store, [card_store.line("card", str(r["name"]), r) for r in (record, bolt)]
    )
    sources = card.Sources(
        store=store, cards=tmp_path / "cards", ledger=tmp_path / "ledger.json"
    )
    card_path = set_groups.SetGroups().card_path(record, sources.cards)
    card_path.parent.mkdir(parents=True)
    card_path.write_text(
        """// Scenario::new("commented out")
const LIVE: Scenario = Scenario::new(
    "ordinary scenario",
);
const RENAMED: Scenario = BASE.titled(r#"renamed "scenario""#);
const fn sample(name: &'static str) -> Scenario { Scenario::new(name) }
const CUSTOM: Scenario = sample("scenario from a builder");
""",
        encoding="utf-8",
    )
    library = tmp_path / "library"
    library.mkdir()
    (library / "mana.rs").write_text(
        'Scenario::new("in the rules library")', encoding="utf-8"
    )
    return sources, bulk, library


def test_check_resolves_scenarios_and_checks_only_listed_cards(
    inputs: tuple[card.Sources, Path, Path],
) -> None:
    sources, bulk, library = inputs
    sources.ledger.write_text(
        json.dumps(
            {
                CARD: [
                    entry(
                        FIRST,
                        scenario=[
                            "ordinary scenario",
                            'renamed "scenario"',
                            "scenario from a builder",
                            "in the rules library",
                        ],
                    ),
                    entry(SECOND, covered="Modification::SetLandTypeWords"),
                ]
            }
        ),
        encoding="utf-8",
    )
    assert (
        rulings.audit(sources.ledger, bulk, sources.store, sources.cards, library) == []
    )


def test_check_reports_missing_and_stale_rulings_and_scenarios(
    inputs: tuple[card.Sources, Path, Path],
) -> None:
    sources, bulk, library = inputs
    sources.ledger.write_text(
        json.dumps(
            {
                CARD: [
                    entry(FIRST, scenario=["commented out", "removed scenario"]),
                    entry(OTHER, untestable=""),
                ]
            }
        ),
        encoding="utf-8",
    )
    errors = rulings.audit(sources.ledger, bulk, sources.store, sources.cards, library)
    assert len(errors) == 5
    assert any(
        rulings.key(SECOND)[1] in line and "has no entry" in line for line in errors
    )
    assert any(
        rulings.key(OTHER)[1] in line and "matches no ruling" in line for line in errors
    )
    assert any("missing scenario 'commented out'" in line for line in errors)
    assert any("missing scenario 'removed scenario'" in line for line in errors)
    assert any("needs an untestable reason" in line for line in errors)


def test_covered_must_name_vocabulary_or_library_scenario(
    inputs: tuple[card.Sources, Path, Path],
) -> None:
    sources, bulk, library = inputs
    sources.ledger.write_text(
        json.dumps(
            {
                CARD: [
                    entry(FIRST, covered="in the rules library"),
                    entry(SECOND, covered="ordinary scenario"),
                ]
            }
        ),
        encoding="utf-8",
    )
    (error,) = rulings.audit(
        sources.ledger, bulk, sources.store, sources.cards, library
    )
    assert "unknown vocabulary entry or library scenario 'ordinary scenario'" in error


@pytest.mark.parametrize(
    ("entries", "message"),
    [
        (
            [entry(FIRST, covered="example"), entry(FIRST, untestable="example")],
            "listed twice",
        ),
        ([entry(FIRST, scenario=["example"], covered="example")], "exactly one"),
    ],
)
def test_check_rejects_ambiguous_entries(entries: object, message: str) -> None:
    with pytest.raises(ValueError, match=message):
        rulings.indexed(entries)


def test_command_reports_and_appends_only_missing_entries(
    inputs: tuple[card.Sources, Path, Path], capsys: pytest.CaptureFixture[str]
) -> None:
    sources, _, _ = inputs
    original = entry(FIRST, scenario=["ordinary scenario"])
    sources.ledger.write_text(json.dumps({CARD: [original]}), encoding="utf-8")
    args = argparse.Namespace(name="tundra", add=False)
    assert card.run_rulings(args, sources) == 0
    shown = capsys.readouterr().out
    assert "A first ruling. [scenario: ordinary scenario]" in shown
    assert "A different ruling on the same day. [unrecorded]" in shown
    assert shown.count("A first ruling.") == 1

    args.add = True
    assert card.run_rulings(args, sources) == 0
    assert "[untestable (unfinished)]" in capsys.readouterr().out
    assert json.loads(sources.ledger.read_text()) == {
        CARD: [original, entry(SECOND, untestable="")]
    }
    snapshot = sources.ledger.read_bytes()
    assert card.run_rulings(args, sources) == 0
    assert sources.ledger.read_bytes() == snapshot


def test_step_logs_missing_cache(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path))
    assert rulings.step() == 0
    assert "rulings: skipped:" in capsys.readouterr().out
