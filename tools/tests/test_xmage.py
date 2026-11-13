"""The XMage exporter on scenarios from `beryl_scenario dump`, and `oracle`'s summary."""

import argparse
import json
from pathlib import Path
from typing import cast

import pytest

from beryl_tools import card_store, checks, oracles, xmage, card_facts, xmage_java

FIXTURE: Path = Path(__file__).parent / "fixtures" / "scenarios-xmage.json"
XMAGE_CARDS = {
    "Adeline, Resplendent Cathar",
    "Elite Spellbinder",
    "Get Lost",
    "Island",
    "Mana Crypt",
    "Mana Drain",
    "Mana Tithe",
    "Plains",
    "Strip Mine",
    "Teferi, Time Raveler",
    "Timetwister",
}


def exports(known: set[str] = XMAGE_CARDS) -> dict[str, xmage_java.Export]:
    dump = json.loads(FIXTURE.read_text())
    faces = xmage_java.Faces.load(card_store.STORE)
    return {e.scenario: e for e in xmage.export_all(dump, faces, known)}


def body(name: str) -> list[str]:
    export = exports()[name]
    assert export.unsupported is None, export.unsupported
    return export.body


def test_the_setups_active_player_is_player_a_on_turn_1() -> None:
    # Beryl's turn 2 with player 1 active: Timetwister's caster is playerA, Mana Drain's
    # playerB, and Beryl's turn 3 is XMage's turn 2.
    lines = body(
        "Mana Drain: the countered spell's mana value at the caster's next main phase"
    )
    assert "removeAllCardsFromLibrary(playerA);" in lines
    assert 'addCard(Zone.HAND, playerA, "Timetwister");' in lines
    cast = 'castSpell(1, PhaseStep.PRECOMBAT_MAIN, playerA, "Timetwister");'
    # The response waits for the spell it targets; its only target was chosen silently.
    drain = (
        'castSpell(1, PhaseStep.PRECOMBAT_MAIN, playerB, "Mana Drain", '
        'TestPlayer.NO_TARGET, "Timetwister");'
    )
    target = 'addTarget(playerB, "Timetwister");'
    assert lines.index(cast) < lines.index(drain) < lines.index(target)
    mana = (
        'runCode("Mana", 2, PhaseStep.PRECOMBAT_MAIN, playerB, (info, player, game) -> '
        "assertManaPool(playerB, ManaType.COLORLESS, 3));"
    )
    assert mana in lines
    assert lines[-2:] == ["setStopAt(2, PhaseStep.BEGIN_COMBAT);", "execute();"]


def test_a_command_after_the_opponents_spell_passes_priority_first() -> None:
    # Player 0 (playerB) casts Get Lost in player 1's (playerA's) main phase, then playerA
    # sacrifices a Map: without the pass, playerA would sacrifice before Get Lost was cast.
    lines = body("Get Lost on a creature gives two Maps; one explores a land into hand")
    get_lost = 'castSpell(1, PhaseStep.PRECOMBAT_MAIN, playerB, "Get Lost");'
    passing = "passPriority(1, PhaseStep.PRECOMBAT_MAIN, playerA);"
    sacrifice = (
        'activateAbility(1, PhaseStep.PRECOMBAT_MAIN, playerA, "{1}, {T}, Sacrifice");'
    )
    assert lines.index(get_lost) < lines.index(passing) < lines.index(sacrifice)
    assert lines.count(passing) == 1


def test_payments_replay_the_lands_beryl_tapped() -> None:
    # Get Lost costs {1}{W}: Beryl taps two of player 0's three Plains, which XMage taps by
    # name, as another card may have an ability of the same text.
    lines = body("Get Lost on a creature gives two Maps; one explores a land into hand")
    tap = 'tapSource(1, PhaseStep.PRECOMBAT_MAIN, playerB, "Plains", "{T}: Add {W}");'
    cast = 'castSpell(1, PhaseStep.PRECOMBAT_MAIN, playerB, "Get Lost");'
    assert lines.count(tap) == 2
    assert lines.index(tap) < lines.index(cast)


def test_coin_flips_replay_beryls_results() -> None:
    lines = body("Mana Crypt's flip lost: 3 damage to its controller")
    assert "setFlipCoinResult(playerB, false);" in lines
    assert "resolveTop(2, PhaseStep.UPKEEP, playerB);" in lines


def test_xmages_unless_pays_question_is_answered_no_where_beryl_does_not_ask() -> None:
    # Player 0 cannot pay for Mana Tithe, so Beryl asks nothing, but XMage asks "Pay {1}?"
    # anyway: playerA, Mana Crypt's controller, answers no.
    unpaid = body("Mana Tithe: a controller who can't pay isn't asked")
    answers = [line for line in unpaid if line.startswith("setOptionalChoice(")]
    assert len(answers) == 1
    assert answers[0].startswith("setOptionalChoice(playerA, ")
    assert answers[0].endswith('"No");')
    # With Strip Mine to pay with, Beryl asks and the scenario answers yes: no extra answer.
    paid = body("Mana Tithe: the controller pays {1}")
    assert "setChoice(playerA, true);" in paid
    assert not any("setOptionalChoice" in line for line in paid)


def test_what_the_exporter_cannot_say_is_unsupported_with_the_reason() -> None:
    adeline = exports()["Adeline's Human attacks a planeswalker"]
    assert adeline.unsupported == (
        "choosing what a creature put onto the battlefield attacks"
    )
    assert adeline.body == []
    missing = exports(XMAGE_CARDS - {"Mana Drain"})
    reason = missing[
        "Mana Drain: the countered spell's mana value at the caster's next main phase"
    ].unsupported
    assert reason == "XMage lacks Mana Drain"


def test_write_makes_a_class_per_card_with_a_supported_scenario(tmp_path: Path) -> None:
    (tmp_path / "Stale.java").write_text("old")
    classes = xmage_java.write(list(exports().values()), tmp_path)
    assert sorted(classes) == [
        "GetLostBerylTest",
        "ManaCryptBerylTest",
        "ManaDrainBerylTest",
        "ManaTitheBerylTest",
    ]
    files = sorted(p.name for p in tmp_path.iterdir())
    assert files == sorted([f"{c}.java" for c in classes] + ["BerylScenarioTest.java"])
    java = (tmp_path / "ManaDrainBerylTest.java").read_text()
    assert "public class ManaDrainBerylTest extends BerylScenarioTest {" in java
    assert java.count("@Test") == 1


def surefire(path: Path, name: str, cases: dict[str, str | None]) -> None:
    """A Surefire report for one test class: each method passes or fails with a message."""
    rows = []
    for method, failure in cases.items():
        inner = (
            "" if failure is None else f'<failure message="{failure}">trace</failure>'
        )
        rows.append(f'<testcase name="{method}" classname="x">{inner}</testcase>')
    path.mkdir(parents=True, exist_ok=True)
    report = path / f"TEST-{xmage_java.PACKAGE}.{name}.xml"
    report.write_text(f"<testsuite>{''.join(rows)}</testsuite>")


def scenarios(summary: dict[str, object]) -> list[dict[str, object]]:
    return cast(list[dict[str, object]], summary["scenarios"])


def allowlist(path: Path, card: str, scenario: str) -> None:
    entry = {
        "oracle": "xmage",
        "card": card,
        "scenario": scenario,
        "cr": "CR",
        "reason": "r",
    }
    path.write_text(json.dumps([entry]))


def test_the_summary_counts_passes_failures_and_allowlisted_divergences(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path))
    dump = [e for e in json.loads(FIXTURE.read_text()) if e["card"] != "Mana Tithe"]
    found = list(exports().values())
    drain: xmage_java.Export = next(e for e in found if e.card == "Mana Drain")
    crypt: xmage_java.Export = next(e for e in found if e.card == "Mana Crypt")
    get_lost: xmage_java.Export = next(e for e in found if e.card == "Get Lost")

    def maven(*args: object, **kwargs: object) -> str:
        # Stands in for Maven: Mana Drain passes, Mana Crypt and Get Lost fail.
        surefire(oracles.reports(), "ManaDrainBerylTest", {drain.method: None})
        surefire(oracles.reports(), "ManaCryptBerylTest", {crypt.method: "no flip"})
        surefire(oracles.reports(), "GetLostBerylTest", {get_lost.method: "no Map"})
        return ""

    monkeypatch.setattr(oracles, "run", maven)
    monkeypatch.setattr(oracles, "java_home", lambda: tmp_path)
    monkeypatch.setattr(xmage_java, "xmage_cards", lambda checkout: XMAGE_CARDS)
    allowed = tmp_path / "divergences.json"
    monkeypatch.setattr(oracles, "DIVERGENCES", allowed)
    log = tmp_path / "oracle.log"

    allowlist(allowed, crypt.card, crypt.scenario)
    summary = oracles.check_xmage(dump, log)
    counts = {k: summary[k] for k in ("pass", "fail", "allowlisted", "unsupported")}
    assert counts == {"pass": 1, "fail": 1, "allowlisted": 1, "unsupported": 1}
    rows = {r["name"]: r for r in scenarios(summary)}
    assert rows[crypt.scenario]["failure"] == "no flip"
    assert rows[get_lost.scenario]["failure"] == "no Map"
    assert rows[drain.scenario]["test"] == f"ManaDrainBerylTest.{drain.method}"
    assert summary["stale_allowlist"] == 0

    # A passing scenario on the allowlist is a stale entry.
    allowlist(allowed, drain.card, drain.scenario)
    stale = oracles.check_xmage(dump, log)
    assert stale["stale_allowlist"] == 1
    row = next(r for r in scenarios(stale) if r["name"] == drain.scenario)
    assert row["stale_allowlist"] is True


def test_the_allowlists_entries_cite_the_rules() -> None:
    for oracle in ("xmage", "forge"):
        allowed = oracles.forge.divergences(oracles.DIVERGENCES, oracle)
        for divergence in allowed.values():
            assert divergence.cr.startswith("CR ")


def test_oracle_needs_the_oracles_built(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path))
    # Not built at its pin is no flake: slow CI does not rerun it.
    assert oracles.oracle(argparse.Namespace(cards=[])) == checks.CANNOT_RUN


def test_untap_shortcut_is_optional_before_explicit_land_picks() -> None:
    # XMage asks to untap all only when tapped lands <= the limit. With three tapped
    # lands and a limit of two, a mandatory No consumes the first land choice.
    source = "Untapper"
    untap: card_facts.Json = {"kind": "UntapUpTo", "count": 2, "player": "You"}
    ability: card_facts.Json = {"index": "Own 0", "kind": "Spell"}
    ability |= {"instructions": [untap]}
    described = {source: {"faces": [{"name": source, "abilities": [ability]}]}}
    entry: card_facts.Json = {
        "setup": {"turn": 1, "active": 0, "step": "PrecombatMain"},
        "actions": [],
        "cards": described,
    }
    export = xmage_java.Export(
        card=source, scenario="untap choices", method="untap_choices"
    )
    writer = xmage.Exporter(entry, xmage_java.Faces.load(card_store.STORE), export)
    position = xmage.Position(
        turn=1,
        step="End",
        decider=0,
        kind="ChooseUpTo",
        purpose="untap",
        stack=[{"name": source}],
        flips=[],
        taps=[],
        forced=[],
        flashback=[],
        face_down=[],
        creatures=[0, 0],
    )
    for card in ["Tundra", "Island"]:
        writer.answer(
            {"player": 0, "kind": "ChooseUpTo", "pick": {"pick": "Card", "card": card}},
            position,
        )
    assert export.body == [
        'setOptionalChoice(playerA, 1, PhaseStep.END_TURN, "No");',
        'setChoice(playerA, "Tundra");',
        'setChoice(playerA, "Island");',
    ]


def test_what_pays_a_cost_is_told_by_the_prompts_purpose() -> None:
    # Beryl asks a waterbend's permanents as a mana payment and a crew's creatures as a choice
    # of an object, as it asks an effect's: the trace's purpose says which they are.
    entry: card_facts.Json = {
        "setup": {"turn": 1, "active": 0, "step": "PrecombatMain"},
        "actions": [],
    }
    export = xmage_java.Export(card="Aang's Iceberg", scenario="costs", method="costs")
    writer = xmage.Exporter(entry, xmage_java.Faces.load(card_store.STORE), export)
    position = xmage.Position(
        turn=1,
        step="PrecombatMain",
        decider=0,
        kind="ManaPayment",
        purpose="waterbend",
        stack=[{"name": "Aang's Iceberg"}],
        flips=[],
        taps=[],
        forced=[],
        flashback=[],
        face_down=[],
        creatures=[1, 0],
    )
    crypt: card_facts.Json = {"pick": "Card", "card": "Mana Crypt"}
    writer.answer({"player": 0, "kind": "ManaPayment", "pick": crypt}, position)
    writer.answer(
        {"player": 0, "kind": "ManaPayment", "pick": {"pick": "Done"}}, position
    )
    # XMage asks a cost's one answer, and an effect's only when it stands for several cards.
    forced: card_facts.Json = {"player": 0, "kind": "ChooseObject", "turn": 1}
    forced |= {"step": "PrecombatMain", "pick": {"pick": "Card", "card": "Island"}}
    writer.forced(forced | {"purpose": "crew"}, position)
    writer.forced(forced | {"purpose": "discard"}, position)
    assert export.body == [
        'setChoice(playerA, "Mana Crypt");',
        "setChoice(playerA, TestPlayer.CHOICE_SKIP);",
        'setChoice(playerA, "Island");',
        'setOptionalChoice(playerA, 1, PhaseStep.PRECOMBAT_MAIN, "Island");',
    ]
