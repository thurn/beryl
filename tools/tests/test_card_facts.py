"""The XMage exporter's rules over the dump's card descriptions, on made-up cards: each rule
stands for a difference between Beryl's questions and XMage's, read from what a card does.
"""

import pytest

from beryl_tools.card_facts import Cards, Json, Unsupported

TAP: Json = {"action": "Tap", "of": "This"}
SACRIFICE: Json = {"action": "Sacrifice", "of": "This"}


def card(*abilities: Json, **face: object) -> Json:
    return {"faces": [{"name": "Gadget", "abilities": list(abilities), **face}]}


def ability(i: int, kind: str, *instructions: Json, **rest: object) -> Json:
    return {
        "index": f"Own {i}",
        "kind": kind,
        "instructions": list(instructions),
        **rest,
    }


def does(kind: str, **rest: object) -> Json:
    return {"kind": kind, **rest}


def mana(
    i: int, *colors: str, amount: int = 1, parts: list[Json] | None = None
) -> Json:
    cost = {"parts": parts if parts is not None else [TAP]}
    produces = {"colors": list(colors), "amount": amount}
    return ability(i, "Mana", does("AddMana"), cost=cost, produces=produces)


def gadget(*abilities: Json, **face: object) -> Cards:
    return Cards({"Gadget": card(*abilities, **face)})


def loyalty(i: int, amount: int) -> Json:
    return ability(
        i, "Loyalty", cost={"parts": [{"action": "Loyalty", "amount": amount}]}
    )


def test_a_loyalty_abilitys_text_starts_with_its_loyalty_change() -> None:
    cards = gadget(loyalty(0, 2), loyalty(1, 0), loyalty(2, -3))
    texts = [cards.ability_text("Gadget", "Own", i) for i in range(3)]
    assert texts == ["+2:", "0:", "-3:"]


def test_an_activated_abilitys_text_is_its_cost_in_xmages_order() -> None:
    sacrifice = {"mana": "{1}", "parts": [TAP, SACRIFICE]}
    counter = {"action": "RemoveCounter", "counter": "Story", "of": "This"}
    story = {"mana": "{W}", "parts": [TAP, counter]}
    cards = gadget(
        ability(0, "Activated", does("Destroy"), cost=sacrifice),
        ability(1, "Activated", does("Draw"), cost=story),
    )
    texts = [cards.ability_text("Gadget", "Own", i) for i in range(2)]
    assert texts == ["{1}, {T}, Sacrifice", "{W}, {T}, Remove a story counter"]


def test_a_full_cost_goes_on_with_the_first_words_of_its_oracle_text() -> None:
    # A land's "{T}: Add" would match a bare "{T}:" too.
    oracle = "{T}: Add {U}.\n{2}, {T}: Draw a card, then discard a card."
    described: Json = {
        "Gadget": card(
            ability(0, "Activated", does("Draw"), cost={"mana": "{2}", "parts": [TAP]})
        )
    }
    cards = Cards(described, {"Gadget": oracle})
    assert cards.ability_text("Gadget", "Own", 0) == "{2}, {T}: Draw a card,"
    assert Cards(described, {}).ability_text("Gadget", "Own", 0) is None


def test_keyword_costs_are_written_as_their_keyword() -> None:
    discard = {"mana": "{2}", "parts": [{"action": "Discard", "of": "This"}]}
    bend = {"action": "Tap", "of": "Chosen", "for_generic": 4}
    cards = gadget(
        ability(0, "Activated", does("Draw", count=1), cost=discard),
        ability(1, "Activated", does("Scry"), cost={"mana": "{4}", "parts": [bend]}),
    )
    assert cards.ability_text("Gadget", "Own", 0) == "Cycling"
    assert cards.ability_text("Gadget", "Own", 1) == "waterbend {4}"


def test_a_granted_ability_is_found_on_the_one_card_that_grants() -> None:
    crew = {"action": "Tap", "of": "Chosen", "other": True, "total_power": 3}
    grant: Json = {"index": "Grant 0", "kind": "Activated", "cost": {"parts": [crew]}}
    granter = {"faces": [{"name": "Rigger", "abilities": [], "grants": [grant]}]}
    cards = Cards({"Gadget": card(), "Rigger": granter})
    assert cards.ability_text("Gadget", "Granted", 0) == "Crew 3"
    both = Cards({"Gadget": card(), "Rigger": granter, "Other": granter})
    assert both.ability_text("Gadget", "Granted", 0) is None


def test_a_source_with_one_mana_ability_taps_by_its_text() -> None:
    assert (
        gadget(mana(0, "Colorless", amount=2)).mana_text("Gadget") == "{T}: Add {C}{C}"
    )
    assert not gadget(mana(0, "Colorless")).mana_choice("Gadget")


def test_a_source_of_several_kinds_of_mana_is_left_to_xmage() -> None:
    dual = gadget(mana(0, "White"), mana(1, "Blue"))
    assert dual.mana_text("Gadget") is None
    assert dual.mana_choice("Gadget")
    back = {"name": "Spring", "abilities": [mana(0, "Blue")]}
    modal = Cards({"Gadget": {"faces": [{"name": "Gadget", "abilities": []}, back]}})
    assert modal.mana_choice("Spring")
    assert modal.mana_text("Spring") is None


def test_a_source_sacrificed_for_any_color_cannot_be_replayed() -> None:
    treasure = mana(0, "White", "Blue", parts=[TAP, SACRIFICE])
    assert gadget(treasure).sacrificed_for_mana_of_any_color("Gadget")
    assert not gadget(mana(0, "White")).sacrificed_for_mana_of_any_color("Gadget")


def test_casting_options_are_asked_by_their_kind_and_cost() -> None:
    impending: Json = {"bit": 0, "alternative": True, "keyword": "Impending"}
    impending["cost"] = {"mana": "{2}"}
    evoke: Json = {"bit": 1, "alternative": True, "keyword": "Evoke"}
    evoke["cost"] = {"parts": [{"action": "Exile", "of": "Chosen", "zone": "Hand"}]}
    kicker = {
        "bit": 2,
        "alternative": False,
        "keyword": "Kicker",
        "cost": {"mana": "{1}"},
    }
    cards = gadget(casting=[kicker, evoke, impending])
    assert cards.cast_answers("Gadget") == [
        ("Cast with Impending", "Cast with no"),
        ("Cast with Evoke", None),
        ("Yes", "No"),
    ]
    assert cards.overload_bit("Gadget") is None
    overload = {"bit": 0, "alternative": True, "keyword": "Overload", "cost": {}}
    assert gadget(casting=[overload]).overload_bit("Gadget") == 0


def test_what_resolving_a_card_does_picks_the_questions_xmage_asks() -> None:
    target: Json = {"kind": "SpellOrPermanent", "count": 1}
    pays = does("Unless", cost={"mana": "{2}"}, player="ControllerOf Target 0")
    cards = gadget(
        ability(0, "Spell", pays, does("Counter"), targets=[target]),
        ability(1, "Triggered", does("Scry", count=1), trigger="Enters"),
    )
    assert cards.targets_in_cast("Gadget")
    assert cards.unless_pays("Gadget")
    assert cards.scries("Gadget")
    assert not cards.untaps_up_to("Gadget")
    assert cards.may_choose("Gadget") is None
    assert cards.takes("Gadget") is None
    assert not cards.choices_as_targets("Gadget")


def test_choices_among_cards_go_to_the_queue_their_instruction_says() -> None:
    choose = gadget(ability(0, "Triggered", does("Choose", optional=True)))
    assert choose.may_choose("Gadget") == (False, "setChoice")
    reveal = gadget(ability(0, "Triggered", does("Dig", count=4, take="MayReveal")))
    assert reveal.may_choose("Gadget") == (True, "addTarget")
    assert reveal.takes("Gadget") is None
    take = gadget(ability(0, "Spell", does("Dig", count=5, take=2)))
    assert take.takes("Gadget") == 2
    put_back = gadget(ability(0, "Loyalty", does("PutBack", count=2)))
    assert put_back.choices_as_targets("Gadget")


def test_searches_are_told_apart_by_who_searches_and_whether_they_may() -> None:
    may = gadget(
        ability(0, "Triggered", does("Search", may=True, player="You", count=1))
    )
    assert may.may_search("Gadget")
    assert may.choices_as_targets("Gadget")
    assert not may.search_by_each_player("Gadget")
    each = does("Search", player="EachPlayer", count="ThoseOf")
    losers = gadget(ability(0, "Spell", does("Exile"), each))
    assert losers.search_by_each_player("Gadget")
    assert not losers.may_search("Gadget")


def test_a_coin_flip_each_turn_is_a_step_trigger_that_flips() -> None:
    flip = ability(0, "Triggered", does("FlipCoin"), trigger="Step")
    assert gadget(flip).flips_a_coin_each_turn("Gadget")
    enters = ability(0, "Triggered", does("FlipCoin"), trigger="Enters")
    assert not gadget(enters).flips_a_coin_each_turn("Gadget")


def test_two_triggers_on_one_event_are_told_apart_by_the_written_ones_text() -> None:
    oracle = "Flying\nWhen this creature enters, tap target creature.\nEvoke {1}"
    evoke = {"bit": 0, "alternative": True, "keyword": "Evoke", "cost": {"mana": "{1}"}}
    written = ability(0, "Triggered", does("Tap"), trigger="Enters")
    keyword = ability(1, "Triggered", does("Sacrifice"), trigger="Enters")
    described: Json = {"Gadget": card(written, keyword, casting=[evoke])}
    cards = Cards(described, {"Gadget": oracle})
    assert cards.first_trigger_text("Gadget") == "When {this} enters, tap"
    # Beryl's first is the keyword's trigger: the card's name answers.
    first: Json = {"Gadget": card(keyword, written, casting=[evoke])}
    assert Cards(first, {"Gadget": oracle}).first_trigger_text("Gadget") is None
    # One trigger per event needs no text; two with no written one cannot be told apart.
    assert Cards({"Gadget": card(written)}, {}).first_trigger_text("Gadget") is None
    with pytest.raises(Unsupported):
        Cards({"Gadget": card(written, written)}, {}).first_trigger_text("Gadget")
