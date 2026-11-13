from beryl_tools import oracle_selection

COUNTERSPELL = "rust/cards/cards_1993_1994/lea/counterspell.rs"
EVERY: list[str] = [f"Card {i}" for i in range(40)]


def test_a_card_change_plays_that_card_and_the_commits_sample() -> None:
    chosen = oracle_selection.choose([COUNTERSPELL, "rust/x.rs"], "abc123", EVERY)
    assert chosen is not None
    assert chosen.pop("Counterspell") == oracle_selection.CHANGED
    assert set(chosen.values()) == {oracle_selection.SAMPLED}
    assert len(chosen) == oracle_selection.SAMPLE
    # The same commit samples the same cards, so a failure reproduces from its SHA.
    again = oracle_selection.choose([COUNTERSPELL], "abc123", EVERY)
    assert again is not None and set(again) - {"Counterspell"} == set(chosen)


def test_a_rules_change_plays_only_the_sample() -> None:
    rules = ["rust/crates/beryl_rules/src/combat.rs"]
    chosen = oracle_selection.choose(rules, "abc123", EVERY)
    assert chosen is not None and len(chosen) == oracle_selection.SAMPLE
    other = oracle_selection.choose(rules, "def456", EVERY)
    assert other is not None and set(other) != set(chosen)


def test_an_exporter_or_harness_change_plays_every_card() -> None:
    for path in (
        "tools/src/beryl_tools/forge.py",
        "tools/src/beryl_tools/xmage_java.py",
        "tools/src/beryl_tools/card_facts.py",
        "rust/crates/beryl_scenario/src/facts.rs",
        "tools/src/beryl_tools/oracles.py",
        "tools/oracles/BerylForgeHarness.java",
        "data/oracle-divergences.json",
    ):
        assert oracle_selection.choose([COUNTERSPELL, path], "abc123", EVERY) is None
    assert oracle_selection.choose(None, "abc123", EVERY) is None
