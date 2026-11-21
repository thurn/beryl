"""The XMage exporter on scenarios from `beryl_scenario dump`, and `oracle`'s summary."""

import argparse
import json
from pathlib import Path
from typing import cast

import pytest

from beryl_tools import (
    card_store,
    deps,
    oracles,
    oracle_exports,
    xmage,
    card_facts,
    xmage_export,
    xmage_java,
)

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
    return {e.scenario: e for e in xmage_export.export_all(dump, faces, known)}


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


def test_xmage_cannot_start_the_main_phase_with_a_setup_trigger_pending() -> None:
    name = "Get Lost on a creature gives two Maps; one explores a land into hand"
    entry = next(e for e in json.loads(FIXTURE.read_text()) if e["name"] == name)
    entry["trace"][0]["stack"] = [
        {"name": "Elite Spellbinder", "spell": False, "controller": 0}
    ]
    faces = xmage_java.Faces.load(card_store.STORE)
    export = xmage_export.export_one(entry, 1, faces, XMAGE_CARDS)
    assert export.unsupported == "a setup trigger pending at the first main phase"
    assert export.body == []


def test_a_decision_answered_for_a_controlled_player_is_scripted_as_theirs() -> None:
    # While player 1 (playerA) controls player 0 (CR 724.5), the scenario names player 1 as
    # the one who casts Mana Drain, but XMage's test players act for themselves, so playerB
    # casts it and chooses its target.
    name = (
        "Mana Drain: the countered spell's mana value at the caster's next main phase"
    )
    dump = json.loads(FIXTURE.read_text())
    entry = next(e for e in dump if e["name"] == name)
    i = next(i for i, a in enumerate(entry["actions"]) if a.get("card") == "Mana Drain")
    entry["actions"][i]["player"] = 1
    entry["trace"][i] |= {"decider": 1, "decides_for": 0}
    for forced in entry["trace"][i + 1]["forced"]:
        forced |= {"player": 1, "decides_for": 0}
    faces = xmage_java.Faces.load(card_store.STORE)
    (export,) = xmage_export.export_all([entry], faces, XMAGE_CARDS)
    assert export.unsupported is None, export.unsupported
    drain = (
        'castSpell(1, PhaseStep.PRECOMBAT_MAIN, playerB, "Mana Drain", '
        'TestPlayer.NO_TARGET, "Timetwister");'
    )
    assert drain in export.body
    assert 'addTarget(playerB, "Timetwister");' in export.body


def test_payments_replay_the_lands_beryl_tapped() -> None:
    # Get Lost costs {1}{W}: Beryl taps two of player 0's three Plains, which XMage taps by
    # name, as another card may have an ability of the same text.
    lines = body("Get Lost on a creature gives two Maps; one explores a land into hand")
    tap = 'tapSource(1, PhaseStep.PRECOMBAT_MAIN, playerB, "Plains", "{T}: Add {W}");'
    cast = 'castSpell(1, PhaseStep.PRECOMBAT_MAIN, playerB, "Get Lost");'
    assert lines.count(tap) == 2
    assert lines.index(tap) < lines.index(cast)


def test_payments_are_left_to_xmage_where_land_types_change() -> None:
    # With a card that sets or adds land types, as Urborg, a Plains may tap for {B}, which
    # its own text doesn't write: XMage pays from its own choice of sources.
    name = "Get Lost on a creature gives two Maps; one explores a land into hand"
    dump = json.loads(FIXTURE.read_text())
    entry = next(e for e in dump if e["name"] == name)
    entry["cards"]["Plains"]["faces"][0]["abilities"].append(
        {"index": "Own 1", "kind": "Static", "changes_land_types": True}
    )
    faces = xmage_java.Faces.load(card_store.STORE)
    export = xmage_export.export_one(entry, 1, faces, XMAGE_CARDS)
    assert export.unsupported is None, export.unsupported
    assert not any(line.startswith("tapSource(") for line in export.body)

    # If player 0 casts a second spell, XMage's first payment may already
    # have spent the sources that the second needs.
    entry["actions"][5:] = entry["actions"][1:3]
    entry["trace"][5:] = entry["trace"][1:3]
    repeated = xmage_export.export_one(entry, 1, faces, XMAGE_CARDS)
    assert repeated.unsupported == "a payment after one XMage chose the sources of"


def test_an_accented_card_is_exported_in_xmages_spelling() -> None:
    # XMage lists Dandân as "Dandan", so a scenario naming it is neither unsupported nor
    # written into the Java with the accent.
    name = "Get Lost on a creature gives two Maps; one explores a land into hand"
    dump = json.loads(FIXTURE.read_text())
    entry = json.loads(json.dumps(next(e for e in dump if e["name"] == name)))
    entry = json.loads(json.dumps(entry).replace("Island", "Islând"))
    faces = xmage_java.Faces.load(card_store.STORE)
    export = xmage_export.export_one(entry, 1, faces, XMAGE_CARDS)
    assert export.unsupported is None, export.unsupported
    java = "\n".join(export.body)
    assert '"Island"' in java
    assert "â" not in java
    assert xmage_java.xmage_spelling("Dandân") == "Dandan"
    assert xmage_java.xmage_spelling("Æther Vial") == "Aether Vial"


def test_tapped_assertions_are_skipped_after_xmage_chose_a_payments_sources() -> None:
    # Beryl paid Get Lost with Urborg and a Plains where XMage may tap two Plains, so whether
    # Urborg is tapped afterwards depends on XMage's choice, not on the rules.
    name = "Get Lost on a creature gives two Maps; one explores a land into hand"
    dump = json.loads(FIXTURE.read_text())
    entry = next(e for e in dump if e["name"] == name)
    entry["cards"]["Plains"]["faces"][0]["abilities"].append(
        {"index": "Own 1", "kind": "Static", "changes_land_types": True}
    )
    # Replaces the assertion just after the cast, keeping the trace's positions in step.
    assert entry["actions"][2]["action"] == "Assert"
    entry["actions"][2]["assertion"] = {
        "assert": "Tapped",
        "player": 0,
        "card": "Plains",
        "count": 1,
    }
    faces = xmage_java.Faces.load(card_store.STORE)
    export = xmage_export.export_one(entry, 1, faces, XMAGE_CARDS)
    assert export.unsupported is None, export.unsupported
    assert not any("assertTapped" in line for line in export.body)
    assert "Tapped: after XMage chose a payment's sources" in export.skipped


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


def test_a_payment_xmage_chose_onto_a_stack_makes_later_taps_by_name_unsupported() -> (
    None
):
    # Thirst for Discovery is cast while The Wandering Emperor's ability is on the stack, so
    # XMage chooses which lands pay its {2}{U}. Blood Artist then taps both Swamps by name,
    # which fails whenever XMage paid Thirst with a Swamp: a flaky scenario, not a rules
    # difference.
    dump = json.loads(
        (FIXTURE.parent / "scenario-xmage-paid-onto-stack.json").read_text()
    )
    named = card_facts.named_cards(dump)
    faces = xmage_java.Faces.load(card_store.STORE, named)
    [export] = xmage_export.export_all(dump, faces, set(named))
    assert export.unsupported == (
        "a payment tapping a source by name after one XMage chose the sources of"
    )


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
    summary = oracles.check_xmage(
        oracle_exports.Exports([e for e in found if e.card != "Mana Tithe"], [], []),
        log,
    )
    counts = {k: summary[k] for k in ("pass", "fail", "allowlisted", "unsupported")}
    assert counts == {"pass": 1, "fail": 1, "allowlisted": 1, "unsupported": 1}
    rows = {r["name"]: r for r in scenarios(summary)}
    assert rows[crypt.scenario]["failure"] == "no flip"
    assert rows[get_lost.scenario]["failure"] == "no Map"
    assert rows[drain.scenario]["test"] == f"ManaDrainBerylTest.{drain.method}"
    assert summary["stale_allowlist"] == 0

    # A passing scenario on the allowlist is a stale entry.
    allowlist(allowed, drain.card, drain.scenario)
    stale = oracles.check_xmage(
        oracle_exports.Exports([e for e in found if e.card != "Mana Tithe"], [], []),
        log,
    )
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
    # Not built at its pin is a run that could not happen, not a failure.
    assert oracles.oracle(argparse.Namespace(cards=[])) == deps.CANNOT_RUN


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
    # The effect can choose an opponent's lands, so their payment can also change the picks.
    writer.payments.left_to_xmage.add("playerB")
    with pytest.raises(
        card_facts.Unsupported, match="an untap choice after XMage chose a payment"
    ):
        writer.answer(
            {
                "player": 0,
                "kind": "ChooseUpTo",
                "pick": {"pick": "Card", "card": "Tundra"},
            },
            position,
        )


def test_a_returning_cards_copy_choice_is_queued_for_xmage() -> None:
    copier = "Clever Impersonator"
    entry: card_facts.Json = {
        "setup": {"turn": 1, "active": 0, "step": "PrecombatMain"},
        "actions": [],
    }
    export = xmage_java.Export(card=copier, scenario="returning copy", method="copy")
    writer = xmage.Exporter(entry, xmage_java.Faces.load(card_store.STORE), export)
    position = xmage.Position.of(
        {
            "turn": 1,
            "step": "End",
            "decider": 0,
            "kind": "ChooseUpTo",
            "purpose": "choose",
            "stack": [{"name": "Worldgorger Dragon", "spell": False}],
            "enter_as_copy": True,
            "flips": [],
            "taps": [],
            "forced": [],
        }
    )
    choice: card_facts.Json = {"action": "Answer", "player": 0, "kind": "ChooseUpTo"}
    writer.act(choice | {"pick": {"pick": "Card", "card": "Leonin Scimitar"}}, position)
    writer.act(choice | {"pick": {"pick": "Done"}}, position)
    writer.forced(
        choice | {"pick": {"pick": "Done"}, "enter_as_copy": True},
        xmage.Position.of(
            {
                "turn": 1,
                "step": "End",
                "decider": 0,
                "kind": "Priority",
                "stack": [],
                "flips": [],
                "taps": [],
                "forced": [],
            }
        ),
    )
    assert export.body == [
        "setChoice(playerA, true);",
        'setChoice(playerA, "Leonin Scimitar");',
        "setChoice(playerA, false);",
        "setChoice(playerA, false);",
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


def test_a_blocked_trampler_queues_lethal_damage_for_its_lone_blocker() -> None:
    # XMage asks how Colossal Dreadmaw divides its damage even with one blocker; Beryl gives
    # the 2/1 lethal damage and the rest to the player, so playerA answers 1.
    attacker, blocker = "Colossal Dreadmaw", "Snapcaster Mage"
    entry: card_facts.Json = {
        "setup": {"turn": 1, "active": 0, "step": "PrecombatMain"},
        "actions": [],
        "cards": {},
    }
    export = xmage_java.Export(card=attacker, scenario="trample", method="trample")
    faces = xmage_java.Faces.load(card_store.STORE, [attacker, blocker])
    writer = xmage.Exporter(entry, faces, export)
    writer.attackers = {1: [attacker]}
    position = xmage.Position(
        turn=1,
        step="DeclareBlockers",
        decider=1,
        kind="Blocker",
        purpose="block",
        stack=[],
        flips=[],
        taps=[],
        forced=[],
        flashback=[],
        face_down=[],
        creatures=[1, 1],
    )
    writer.answer(
        {"player": 1, "kind": "Blocker", "pick": {"pick": "Card", "card": blocker}},
        position,
    )
    assert export.body == [
        'block(1, playerB, "Snapcaster Mage", "Colossal Dreadmaw");',
        "setChoiceAmount(playerA, 1);",
    ]


@pytest.mark.parametrize(
    ("pick", "expected"),
    [
        # Kept: XMage asks whether to change the target, and Beryl's pick is the one it has.
        ({"pick": "Player", "player": 1}, ["setChoice(playerB, false);"]),
        # Changed: yes, then the new target.
        (
            {"pick": "Player", "player": 0},
            ["setChoice(playerB, true);", "addTarget(playerB, playerB);"],
        ),
    ],
)
def test_a_copys_new_target_answers_xmages_change_question(
    pick: card_facts.Json, expected: list[str]
) -> None:
    # Fork's copy of a Lightning Bolt aimed at player 1; player 0 (playerB) controls it.
    entry: card_facts.Json = {
        "setup": {"turn": 1, "active": 1, "step": "PrecombatMain"},
        "actions": [],
        "cards": {},
    }
    export = xmage_java.Export(card="Fork", scenario="fork", method="fork")
    faces = xmage_java.Faces.load(card_store.STORE, ["Fork", "Lightning Bolt"])
    writer = xmage.Exporter(entry, faces, export)
    position = xmage.Position(
        turn=1,
        step="PrecombatMain",
        decider=0,
        kind="Target",
        purpose="target",
        stack=[{"name": "Lightning Bolt", "spell": True, "controller": 1}],
        flips=[],
        taps=[],
        forced=[],
        flashback=[],
        face_down=[],
        creatures=[0, 0],
        kept={"pick": "Player", "player": 1},
    )
    writer.answer({"player": 0, "kind": "Target", "pick": pick}, position)
    assert export.body == expected
    assert writer.retargeted == "playerB"


@pytest.mark.parametrize(
    ("card", "answer"),
    [
        ("Pithing Needle", "Black Lotus"),
        ("Painter's Servant", "[choice_skip]"),
        ("Cavern of Souls", "[choice_skip]"),
    ],
)
def test_setup_answers_as_enters_choices_without_changing_the_game(
    card: str, answer: str
) -> None:
    entry: card_facts.Json = {
        "setup": {"turn": 1, "active": 0, "step": "PrecombatMain"},
        "actions": [],
    }
    export = xmage_java.Export(card="Generated", scenario="setup", method="setup")
    faces = xmage_java.Faces.load(card_store.STORE, [card])
    writer = xmage.Exporter(entry, faces, export)
    permanent: card_facts.Json = {
        "card": card,
        "face": False,
        "tapped": False,
        "counters": None,
    }
    writer.permanent("playerB", permanent, "Upkeep")
    assert export.body == [
        f'addCard(Zone.BATTLEFIELD, playerB, "{card}", 1, false);',
        f'setChoice(playerB, "{answer}");',
    ]


def test_player_as_tapped_permanents_are_tapped_after_xmages_untap_step() -> None:
    # XMage plays turn 1 from its untap step, which untapped a Thieving Skydiver the
    # scenario started tapped, so Builder's Blessing gave it +0/+2 in XMage only.
    entry: card_facts.Json = {
        "setup": {"turn": 1, "active": 0, "step": "PrecombatMain"},
        "actions": [],
    }
    export = xmage_java.Export(card="Generated", scenario="setup", method="setup")
    faces = xmage_java.Faces.load(card_store.STORE, ["Wetland Sambar"])
    writer = xmage.Exporter(entry, faces, export)
    permanent: card_facts.Json = {
        "card": "Wetland Sambar",
        "face": False,
        "tapped": True,
        "counters": None,
    }
    writer.permanent("playerA", permanent, "PrecombatMain")
    writer.permanent("playerB", permanent, "PrecombatMain")
    assert export.body == [
        'addCard(Zone.BATTLEFIELD, playerA, "Wetland Sambar", 1, false);',
        'tapAtStart(playerA, PhaseStep.PRECOMBAT_MAIN, "Wetland Sambar");',
        'addCard(Zone.BATTLEFIELD, playerB, "Wetland Sambar", 1, true);',
    ]


def test_xmage_cards_skips_set_entries_that_are_commented_out(tmp_path: Path) -> None:
    # XMage's set files list unimplemented cards on commented-out lines. Counting them
    # exported scenarios naming Mind Bend, which XMage then could not find.
    sets = tmp_path / "Mage.Sets" / "src" / "mage" / "sets"
    sets.mkdir(parents=True)
    (sets / "Mirage.java").write_text(
        '        cards.add(new SetCardInfo("Mind Stone", 1, Rarity.COMMON, X.class));\n'
        '//        cards.add(new SetCardInfo("Mind Bend", 77, Rarity.UNCOMMON, Y.class));\n'
    )
    assert xmage_java.xmage_cards(tmp_path) == {"Mind Stone"}


def test_a_permanent_enters_with_the_counters_its_text_says() -> None:
    oracle = (
        "Dark Depths enters with ten ice counters on it.\n{3}: Remove an ice counter."
    )
    assert xmage_java.entering_counters(oracle, "Ice") == 10
    assert xmage_java.entering_counters(oracle, "Time") == 0
    assert xmage_java.entering_counters("Draw a card.", "Ice") == 0


def test_a_draw_is_checked_once_the_game_has_run_and_a_win_is_unsupported() -> None:
    draw: card_facts.Json = {"assert": "Outcome", "outcome": {"ending": "Draw"}}
    assert xmage_java.outcome_lines(draw) == [
        'org.junit.Assert.assertTrue("a draw", playerA.hasDrew() && playerB.hasDrew());'
    ]
    win: card_facts.Json = {
        "assert": "Outcome",
        "outcome": {"ending": "Win", "player": 0},
    }
    with pytest.raises(card_facts.Unsupported, match="a Win outcome assertion"):
        xmage_java.outcome_lines(win)
