"""data/catalogs.json (catalogs.py), held to the pool's type lines."""

from typing import cast

from beryl_tools import card_store, catalogs


def test_every_pool_type_line_is_in_the_catalogs() -> None:
    found = cast(dict[str, list[str]], catalogs.load()["catalogs"])
    types = set(found["supertypes"]) | set(found["card-types"])
    subtypes = {s for name in catalogs.SUBTYPES for s in found[name]}
    unknown: set[str] = set()
    with card_store.Store() as store:
        for kind, record in store:
            if kind != "card":
                continue
            for face in cast(list[card_store.Json], record["faces"]):
                line = str(face.get("type_line", ""))
                unknown |= set(line.partition(" — ")[0].split()) - types
                unknown |= set(catalogs.subtypes(line, subtypes)[1])
    assert not unknown


def test_subtypes_read_the_longest_entry() -> None:
    known = {"Time Lord", "Human", "Lord"}
    line = "Legendary Creature — Time Lord Human Rogue"
    assert catalogs.subtypes(line, known) == (["Time Lord", "Human"], ["Rogue"])


def test_counter_kinds_follow_a_quantity() -> None:
    texts = [
        "Put a +1/+1 counter on target creature. Counter target spell.",
        "Put three time counters on it. Remove a first strike counter from it.",
        "Double the number of each kind of counter. Put that many oil counters on it.",
        "Choose two different counters. Whenever you put one or more counters on it, "
        "put an everything counter on it.",
    ]
    kinds = catalogs.counter_kinds(texts, ["First strike", "Flying"])
    assert kinds == ["+1/+1", "everything", "first strike", "oil", "time"]
    assert "+1/+1" in cast(list[str], catalogs.load()["counter_kinds"])
