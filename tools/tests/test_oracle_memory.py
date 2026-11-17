import json
from pathlib import Path

from beryl_tools import oracle_exports, oracle_memory, paths, xmage_java
from beryl_tools.oracle_memory import FAIL, GENERATED, NEW, PASS, Memory

SAMPLED = "sample"


def entry(card: str, name: str, snapshot: str = "life 20") -> dict[str, object]:
    return {"card": card, "name": name, "final": snapshot}


def memory(
    entries: list[dict[str, object]],
    outcomes: dict[str, str] | None = None,
    always: set[str] | None = None,
) -> Memory:
    # These selection tests stand in for the exporters with the entry's JSON.
    keys = {oracle_memory.label(e): json.dumps(e, sort_keys=True) for e in entries}
    return Memory("side", outcomes or {}, always or set(), keys)


def remember(m: Memory, e: dict[str, object], outcome: str) -> None:
    m.outcomes[m.key(e)] = outcome


def names(entries: list[dict[str, object]]) -> list[str]:
    return [oracle_memory.label(e) for e in entries]


def test_the_fast_check_plays_new_generated_scenarios_and_unpassed_cards() -> None:
    passed, failed, unknown = (entry(GENERATED, f"Generated {i}") for i in (1, 2, 3))
    gap = entry(GENERATED, "Generated 4")
    mine, mine_passed = entry("Counterspell", "counters"), entry(
        "Counterspell", "fizzles"
    )
    other = entry("Opt", "scry")
    entries = [passed, failed, unknown, gap, mine, mine_passed, other]
    m = memory(entries, always={oracle_memory.label(gap)})
    remember(m, passed, PASS)
    # A failure another run recorded is already on staging: slow CI owns it.
    remember(m, failed, FAIL)
    remember(m, mine_passed, PASS)
    kept = m.select(entries, None, ["Counterspell"], True, SAMPLED)
    assert names(kept) == ["Generated / Generated 3", "Counterspell / counters"]


def test_a_rules_change_rewrites_a_generated_scenario_and_it_plays_again() -> None:
    original = entry(GENERATED, "Generated 1")
    m = memory([original])
    remember(m, original, PASS)
    rewritten = entry(GENERATED, "Generated 1", snapshot="life 17")
    changed = memory([rewritten], outcomes=m.outcomes)
    assert changed.select([rewritten], None, [], True, SAMPLED) == [rewritten]
    # So does every scenario when the oracles' side changes.
    other_side = Memory("other side", m.outcomes, set(), m.exports)
    assert other_side.seen(original) is None


def test_slow_ci_plays_its_sample_whole_and_everything_not_yet_passed() -> None:
    sample, changed = entry("Opt", "scry"), entry("Counterspell", "counters")
    passed, failed = entry(GENERATED, "Generated 1"), entry(GENERATED, "Generated 2")
    listed, derived = entry(GENERATED, "Generated 3"), entry("Island", "generated tap")
    entries = [sample, changed, passed, failed, listed, derived]
    m = memory(entries, always={oracle_memory.label(listed)})
    for e in (sample, changed, passed):
        remember(m, e, PASS)
    remember(m, failed, FAIL)
    chosen = {"Opt": SAMPLED, "Counterspell": "card file changed"}
    kept = m.select(entries, chosen, [], False, SAMPLED)
    assert names(kept) == [
        "Opt / scry",
        "Generated / Generated 2",
        "Island / generated tap",
    ]
    assert chosen[GENERATED] == NEW and chosen["Island"] == NEW


def test_without_flags_every_scenario_or_the_named_cards_play() -> None:
    entries = [entry(GENERATED, "Generated 1"), entry("Opt", "scry")]
    m = memory(entries)
    for e in entries:
        remember(m, e, PASS)
    assert m.select(entries, None, [], False, SAMPLED) == entries
    assert names(m.select(entries, None, ["Opt"], False, SAMPLED)) == ["Opt / scry"]


def test_outcomes_are_recorded_and_the_last_one_wins(tmp_path: Path) -> None:
    store = tmp_path / "outcomes.txt"
    ok, bad, stale = (entry("Opt", n) for n in ("scry", "draw", "loot"))
    results = {
        "xmage": {
            "scenarios": [
                {"card": "Opt", "name": "scry", "status": "pass"},
                {"card": "Opt", "name": "draw", "status": "fail"},
                {"card": "Opt", "name": "loot", "status": "pass"},
            ]
        },
        "forge": {
            "scenarios": [
                {"card": "Opt", "name": "scry", "status": "unsupported"},
                {"card": "Opt", "name": "draw", "status": "pass"},
                # A pass the allowlist explains as a divergence leaves that entry stale.
                {
                    "card": "Opt",
                    "name": "loot",
                    "status": "pass",
                    "stale_allowlist": True,
                },
            ]
        },
    }
    m = memory([ok, bad, stale])
    m.record([ok, bad, stale], results, store)
    assert [m.seen(e) for e in (ok, bad, stale)] == [PASS, FAIL, FAIL]
    lines = store.read_text().splitlines()
    assert len(lines) == 3
    store.write_text(store.read_text() + f"{m.key(bad)} {PASS}\n")
    loaded = Memory.load([], m.exports, store)
    loaded.side = m.side
    assert loaded.seen(bad) == PASS and loaded.seen(ok) == PASS


def test_only_harnesses_and_pins_invalidate_every_export(tmp_path: Path) -> None:
    tools = tmp_path / "tools/src/beryl_tools"
    tools.mkdir(parents=True)
    exporter = tools / "xmage_java.py"
    exporter.write_text("one")
    before = oracle_memory.side(["pin"], tmp_path)
    exporter.write_text("two")
    (tmp_path / "data").mkdir()
    (tmp_path / "data/card-store.tsv").write_text("another card")
    assert oracle_memory.side(["pin"], tmp_path) == before
    assert oracle_memory.side(["other pin"], tmp_path) != before
    for harness in (
        tools / "BerylScenarioTest.java",
        tmp_path / "tools/oracles/BerylForgeHarness.java",
    ):
        harness.parent.mkdir(parents=True, exist_ok=True)
        harness.write_text("one")
        before = oracle_memory.side(["pin"], tmp_path)
        harness.write_text("two")
        assert oracle_memory.side(["pin"], tmp_path) != before


def test_export_changes_select_only_the_affected_generated_scenarios() -> None:
    entries = [entry(GENERATED, f"Generated {i}") for i in (1, 2)]
    java = [
        xmage_java.Export(GENERATED, str(e["name"]), f"s{i}", ["execute();"])
        for i, e in enumerate(entries)
    ]
    forge = [{**e, "steps": []} for e in entries]
    exports = oracle_exports.Exports(java, forge, [])
    m = Memory("side", {}, set(), exports.keys())
    for e in entries:
        remember(m, e, PASS)
    assert m.select(entries, {}, [], False, SAMPLED) == []
    # A translation change to one method leaves its sibling's remembered pass intact.
    java[0].body.append("assertLife(playerA, 17);")
    changed = Memory("side", m.outcomes, set(), exports.keys())
    assert changed.select(entries, {}, [], False, SAMPLED) == [entries[0]]
    assert changed.select(entries, None, [], True, SAMPLED) == [entries[0]]
    # Forge's input and unsupported-to-supported transitions also invalidate that scenario.
    for e in entries:
        remember(changed, e, PASS)
    forge[1]["steps"] = [{"op": "pass"}]
    changed = Memory("side", changed.outcomes, set(), exports.keys())
    assert changed.select(entries, {}, [], False, SAMPLED) == [entries[1]]
    remember(changed, entries[1], PASS)
    java[1].unsupported = "a new refusal"
    refused = Memory("side", changed.outcomes, set(), exports.keys())
    assert refused.seen(entries[1]) is None
    remember(refused, entries[1], PASS)
    java[1].unsupported = None
    assert (
        Memory("side", refused.outcomes, set(), exports.keys()).seen(entries[1]) == PASS
    )


def test_only_changes_that_reach_the_scenarios_call_for_the_lane() -> None:
    def package(name: str, *deps: str) -> dict[str, object]:
        manifest = paths.REPO / "rust" / "crates" / name / "Cargo.toml"
        return {
            "name": name,
            "manifest_path": str(manifest),
            "dependencies": [{"name": d, "path": f"rust/crates/{d}"} for d in deps],
        }

    packages = [
        package("beryl_rules"),
        package("beryl_cards", "beryl_rules"),
        package("beryl_cli", "beryl_cards"),
        package("beryl_mobile"),
    ]
    assert oracle_memory.reached(["rust/crates/beryl_rules/src/combat.rs"], packages)
    assert not oracle_memory.reached(["rust/crates/beryl_cli/src/main.rs"], packages)
    assert not oracle_memory.reached(["tools/x.py", "docs/checks.md"], packages)
    for path in (
        "tools/src/beryl_tools/card_facts.py",
        "tools/src/beryl_tools/BerylScenarioTest.java",
        "tools/oracles/BerylForgeHarness.java",
    ):
        assert oracle_memory.reached([path], packages)
    assert oracle_memory.reached(["data/card-store.tsv"], packages)
